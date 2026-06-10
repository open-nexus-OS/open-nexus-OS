// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(unused_imports)] // os-lite markers only used in OS cfg

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::command::buffer::{Command, CommittedBuffer, RgbaColor};
use nexus_gfx::core::fence::Fence;
use nexus_gfx::core::types::PixelFormat;

use crate::error::GpuDriverError;
use crate::markers::{
    GPUD_RESOURCE_ATTACH_CMD_FAIL, GPUD_RESOURCE_CAP_QUERY_FAIL, GPUD_RESOURCE_CREATED,
    GPUD_RESOURCE_CREATE_CMD_FAIL, GPUD_RESOURCE_VMO_CREATE_FAIL, GPUD_RESOURCE_VMO_MAP_FAIL,
};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

/// Wraps a virtio-gpu MMIO device and implements GfxBackend.
/// On real hardware, this would be replaced by a different GfxBackend impl
/// (e.g., MaliGpuBackend, ImaginationGpuBackend) — same trait, different hardware.
pub struct VirtioGpuBackend {
    mmio_base: usize,
    _mmio_len: usize,
    next_resource_id: u32,
    probed: bool,
    resources: alloc::vec::Vec<ResourceRecord>,
    scanout_resource: Option<ResourceId>,
    /// Fragment uniform storage for SetFragmentBytes commands.
    /// Phase 6c: stores shader parameters (animation state) pushed by windowd.
    fragment_data: [u8; 64],
    /// Software cursor sprite: the real Mocu SVG cursor (premultiplied BGRA),
    /// uploaded once by windowd. BlendCursor composites this onto the display
    /// plane each frame. Empty until uploaded → procedural arrow fallback.
    cursor_sprite: alloc::vec::Vec<u8>,
    cursor_sprite_w: u32,
    cursor_sprite_h: u32,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    ctrlq: Option<CtrlQueue>,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct ResourceRecord {
    pub(crate) id: ResourceId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: PixelFormat,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) backing_va: usize,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) backing_pa: u64,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) backing_len: usize,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) backing_vmo: u32,
}

impl VirtioGpuBackend {
    /// Create a new backend. Does NOT probe — call probe() separately.
    pub fn new(mmio_base: usize, mmio_len: usize) -> Self {
        Self {
            mmio_base,
            _mmio_len: mmio_len,
            next_resource_id: 1,
            probed: false,
            resources: alloc::vec::Vec::new(),
            scanout_resource: None,
            fragment_data: [0u8; 64],
            cursor_sprite: alloc::vec::Vec::new(),
            cursor_sprite_w: 0,
            cursor_sprite_h: 0,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            ctrlq: None,
        }
    }

    /// Probe the MMIO region for a virtio-gpu device.
    /// Returns Ok if the device is found and initialized.
    pub fn probe(&mut self) -> Result<(), GpuDriverError> {
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let _ = self.mmio_base;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.probe_os()?;
        self.probed = true;

        // Query available displays. Without this, QEMU may not report
        // scanout dimensions correctly.
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        {
            let info_cmd = protocol::VirtioGpuCtrlHdr {
                type_: protocol::VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
                flags: 0,
                fence_id: 0,
                ctx_id: 0,
                _padding: 0,
            };
            // GET_DISPLAY_INFO has no additional payload beyond the header.
            if self.ctrlq.is_some() {
                let _ = self.ctrl_submit_struct(&info_cmd);
            }
        }

        Ok(())
    }

    pub fn is_probed(&self) -> bool {
        self.probed
    }

    /// Attach an externally-owned VMO as the display scanout backing.
    /// Zero-copy: the same VMO that windowd composes into becomes the GPU scanout.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn attach_external_framebuffer(
        &mut self,
        vmo_slot: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(vmo_slot, &mut info).map_err(|e| {
            let _ = nexus_abi::debug_println("gpud: ERROR cap_query failed");
            let _ = e;
            GfxError::MmioFault
        })?;
        if info.kind_tag != 1 {
            let _ = nexus_abi::debug_println("gpud: ERROR attach: kind_tag != VMO");
            return Err(GfxError::InvalidArgument);
        }
        if info.len < (width as u64 * height as u64 * 4) {
            let _ = nexus_abi::debug_println("gpud: ERROR attach: VMO too small");
            return Err(GfxError::InvalidArgument);
        }
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;

        // Map the external VMO into gpud's VA space for direct framebuffer write access.
        // Phase 6c: this enables gpud to execute rendering commands directly into the
        // scanout framebuffer without vmo_write syscalls from windowd.
        let resource_index = self.resources.len();
        let backing_va = GPU_RESOURCE_BASE_VA + resource_index * GPU_RESOURCE_STRIDE;
        let backing_len_aligned = align_page((width * height * 4) as usize);
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len_aligned).step_by(4096) {
            nexus_abi::vmo_map_page(vmo_slot, backing_va + offset, offset, flags).map_err(|_e| {
                let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_MAP_FAIL);
                GfxError::MmioFault
            })?;
        }

        let create = protocol::VirtioGpuResourceCreate2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_CREATE_RESOURCE_2D),
            resource_id: id.0,
            format: Self::to_gpu_format(PixelFormat::Bgra8888),
            width,
            height,
        };
        self.ctrl_submit_struct(&create).map_err(|_| GfxError::CommandRejected)?;

        let attach = protocol::VirtioGpuResourceAttachBacking {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: id.0,
            nr_entries: 1,
        };
        let entry = protocol::VirtioGpuMemEntry {
            addr: info.base,
            length: (width * height * 4) as u32,
            _padding: 0,
        };
        self.ctrl_submit_pair(&attach, &entry).map_err(|_| GfxError::CommandRejected)?;

        // Activate the scanout first, then commit the initial framebuffer contents.
        // This matches the visible-bootstrap contract more closely: QEMU first learns
        // the target mode/scanout, then receives the content transfer + flush.
        // Phase 3: scanout displays frame ring slot A (rows 1600..2399) in 4-plane VMO.
        let scanout = protocol::VirtioGpuSetScanout {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect { x: 0, y: height / 2, width, height: height / 4 },
            scanout_id: 0,
            resource_id: id.0,
        };
        self.ctrl_submit_struct(&scanout).map_err(|_| GfxError::CommandRejected)?;
        let _ = nexus_abi::debug_println("gpud: set_scanout ok");
        let _ = nexus_abi::debug_println("gpud: scanout ok");
        let _ = nexus_abi::debug_println("gpud: scanout 1280x800 bgra8888");

        let record = ResourceRecord {
            id,
            width,
            height,
            format: PixelFormat::Bgra8888,
            backing_va,
            backing_pa: info.base,
            backing_len: (width * height * 4) as usize,
            backing_vmo: 0, // external VMO from windowd; page mapping persists independent of cap lifetime
        };

        // Transfer only the display half (rows 800..1599) to host.
        let display_half = height / 2;
        self.transfer_to_host_os(record, Rect { x: 0, y: display_half, width, height: display_half })
            .map_err(|e| {
                let _ = nexus_abi::debug_println("gpud: ERROR transfer_to_host for initial frame failed");
                e
            })?;
        let _ = nexus_abi::debug_println("gpud: transfer_to_host ok");

        let flush = protocol::VirtioGpuResourceFlush {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect { x: 0, y: display_half, width, height: display_half },
            resource_id: id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&flush).map_err(|_| {
            let _ = nexus_abi::debug_println("gpud: ERROR resource flush failed");
            GfxError::CommandRejected
        })?;
        let _ = nexus_abi::debug_println("gpud: resource flush ok");

        self.resources.push(record);
        self.scanout_resource = Some(id);
        Ok(())
    }

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn present_scanout_damage(&mut self, rect: Rect) -> Result<(), GfxError> {
        let scanout = self.scanout_resource.ok_or_else(|| {
            let _ = nexus_abi::debug_println("gpud: backend present_scanout_damage: no scanout_resource");
            GfxError::InvalidArgument
        })?;
        let record = self.find_resource(scanout).ok_or(GfxError::InvalidArgument)?;
        // Phase 6c: display is at top half — offset coords by display_height (record.height/2).
        let display_rect = Rect { x: rect.x, y: rect.y + record.height / 2, width: rect.width, height: rect.height };
        validate_rect(record, display_rect).map_err(|_| {
            let _ = nexus_abi::debug_println("gpud: backend validate_rect FAIL");
            GfxError::InvalidArgument
        })?;
        self.transfer_to_host_os(record, display_rect).map_err(|e| {
            let _ = nexus_abi::debug_println("gpud: backend transfer_to_host_os FAIL");
            e
        })?;
        self.flush_rect_os(record, display_rect).map_err(|e| {
            let _ = nexus_abi::debug_println("gpud: backend flush_rect_os FAIL");
            e
        })?;
        Ok(())
    }

    #[cfg(not(all(feature = "os-lite", target_os = "none")))]
    pub fn present_scanout_damage(&mut self, rect: Rect) -> Result<(), GfxError> {
        let scanout = self.scanout_resource.ok_or(GfxError::InvalidArgument)?;
        self.transfer_to_host(scanout, rect)
    }

    /// Create and present a static solid-color scanout as an early bootstrap
    /// frame before windowd hands over its composed framebuffer VMO.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn attach_bootstrap_solid_scanout(
        &mut self,
        width: u32,
        height: u32,
        bgra: [u8; 4],
    ) -> Result<(), GfxError> {
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let pixel_len = width as usize * height as usize * 4;
        if pixel_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, pixel_len) };
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&bgra);
        }
        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Create and present a static BGRA scanout frame as early bootstrap.
    /// `pixels` must be exactly `width * height * 4` bytes.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn attach_bootstrap_bgra_scanout(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), GfxError> {
        let expected_len = width as usize * height as usize * 4;
        if pixels.len() != expected_len {
            return Err(GfxError::InvalidArgument);
        }
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        if expected_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let dst =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, expected_len) };
        dst.copy_from_slice(pixels);
        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Create and present a static BGRA scanout from a smaller source image.
    /// Source pixels are nearest-neighbor upscaled to `(width,height)`.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn attach_bootstrap_scaled_bgra_scanout(
        &mut self,
        width: u32,
        height: u32,
        source_width: u32,
        source_height: u32,
        source_pixels: &[u8],
    ) -> Result<(), GfxError> {
        if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let source_len = source_width as usize * source_height as usize * 4;
        if source_pixels.len() != source_len {
            return Err(GfxError::InvalidArgument);
        }
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let dst_len = width as usize * height as usize * 4;
        if dst_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let dst = unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, dst_len) };

        let src_w = source_width as usize;
        let src_h = source_height as usize;
        let out_w = width as usize;
        let out_h = height as usize;
        for y in 0..out_h {
            let src_y = y * src_h / out_h;
            for x in 0..out_w {
                let src_x = x * src_w / out_w;
                let src_idx = (src_y * src_w + src_x) * 4;
                let dst_idx = (y * out_w + x) * 4;
                dst[dst_idx..dst_idx + 4].copy_from_slice(&source_pixels[src_idx..src_idx + 4]);
            }
        }

        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Create and present a black bootstrap scanout with centered text.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn attach_bootstrap_text_scanout(&mut self, width: u32, height: u32) -> Result<(), GfxError> {
        let resource = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(resource).ok_or(GfxError::InvalidArgument)?;
        let pixel_len = width as usize * height as usize * 4;
        if pixel_len > record.backing_len {
            return Err(GfxError::ResourceExhausted);
        }
        let pixels =
            unsafe { core::slice::from_raw_parts_mut(record.backing_va as *mut u8, pixel_len) };
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.copy_from_slice(&[0, 0, 0, 255]);
        }

        draw_centered_bootstrap_line(
            pixels,
            width,
            height,
            (height as i32 / 2) - 80,
            "open nexus OS",
            12,
            [240, 240, 240, 255],
        );
        draw_centered_bootstrap_line(
            pixels,
            width,
            height,
            (height as i32 / 2) + 20,
            "One OS. Many Devices.",
            6,
            [190, 190, 190, 255],
        );
        draw_centered_bootstrap_line(
            pixels,
            width,
            height,
            (height as i32) - 70,
            "Powered by Risc-V",
            4,
            [150, 150, 150, 255],
        );

        self.set_scanout_os(record)?;
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        self.flush_rect_os(record, full)?;
        self.scanout_resource = Some(resource);
        Ok(())
    }

    /// Convert PixelFormat to virtio-gpu format constant.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn to_gpu_format(fmt: PixelFormat) -> u32 {
        match fmt {
            PixelFormat::Bgra8888 => protocol::VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            PixelFormat::Rgba8888 => protocol::VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM,
        }
    }

    /// Clone the backing VMO of a resource so another service (windowd)
    /// can write composed frames into the scanout framebuffer.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn clone_scanout_vmo(&self, res: ResourceId) -> Option<u32> {
        let record = self.find_resource(res)?;
        nexus_abi::cap_clone(record.backing_vmo).ok()
    }

    pub(crate) fn find_resource(&self, res: ResourceId) -> Option<ResourceRecord> {
        self.resources.iter().copied().find(|record| record.id == res)
    }

    /// Present a borrowed `CommittedBuffer` (validate + execute) without taking
    /// ownership. Mirrors [`GfxBackend::submit`] but borrows, so the caller can
    /// hold one reusable buffer and `reload_from` it every frame — avoiding the
    /// per-frame `Vec<Command>` that `submit(CommittedBuffer)` would require.
    /// gpud runs on a non-freeing bump allocator, so that per-frame Vec would
    /// otherwise exhaust the heap and crash mid-animation.
    pub(crate) fn present_committed(&mut self, cmd: &CommittedBuffer) -> Result<Fence, GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        cmd.validate().map_err(map_nexus_error)?;
        let mut fence = Fence::new_unsignaled();
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        {
            self.execute_commands(cmd.commands())?;
        }
        fence.signal();
        Ok(fence)
    }

    // ── Phase 6c: Command execution on OS (direct VMO write) ──────────────

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn execute_commands(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        if record.backing_va == 0 {
            return Err(GfxError::MmioFault);
        }
        let fb = record.backing_va as *mut u8;
        let fb_len = record.backing_len;
        let fb_w = record.width as usize;
        let display_y_offset = record.height / 2;
        for cmd in cmds {
            match cmd {
                Command::SetFragmentBytes { offset, data } => {
                    let end = offset.saturating_add(data.len());
                    if end > self.fragment_data.len() {
                        return Err(GfxError::CommandRejected);
                    }
                    self.fragment_data[*offset..end].copy_from_slice(data);
                }
                Command::DrawTiles { tiles } => {
                    let color = self.tile_color_from_fragment();
                    for t in tiles {
                        fill_rect_solid_vmo(
                            fb,
                            fb_len,
                            fb_w,
                            t.x,
                            t.y.saturating_add(display_y_offset),
                            t.width,
                            t.height,
                            color,
                        );
                    }
                }
                Command::FillSdfRoundedRect { rect, radius, color } => {
                    fill_sdf_rounded_vmo(
                        fb, fb_len, fb_w,
                        rect.x,
                        rect.y.saturating_add(display_y_offset),
                        rect.width,
                        rect.height,
                        *radius, *color,
                    );
                }
                Command::BlurBackdrop { rect, radius, saturation_percent } => {
                    blur_backdrop_vmo(
                        fb, fb_len, fb_w,
                        rect.x,
                        rect.y.saturating_add(display_y_offset),
                        rect.width,
                        rect.height,
                        *radius, *saturation_percent,
                    )?;
                }
                Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height } => {
                    // Retained-surface composite: src_y is an absolute VMO row
                    // (windowd points it at the retained plane, rows 800..1599).
                    // dst_y is screen-relative; add display_y_offset so the copy
                    // lands in the display plane (Plane 2, rows 1600..2399).
                    blit_vmo(
                        fb, fb_len, fb_w,
                        *src_x, *src_y,
                        *dst_x, dst_y.saturating_add(display_y_offset),
                        *width, *height,
                    )?;
                }
                Command::BlendCursor { x, y, width, height } => {
                    blend_cursor_vmo(
                        fb,
                        fb_len,
                        fb_w,
                        *x,
                        y.saturating_add(display_y_offset),
                        *width,
                        *height,
                        &self.cursor_sprite,
                        self.cursor_sprite_w,
                        self.cursor_sprite_h,
                    )?;
                }
                Command::BlitAbsolute { src_x, src_y_abs, dst_x, dst_y_abs, width, height } => {
                    // Raw VMO blit — no display_y_offset added; caller passes absolute rows.
                    blit_vmo(fb, fb_len, fb_w, *src_x, *src_y_abs, *dst_x, *dst_y_abs, *width, *height)?;
                }
            }
        }
        Ok(())
    }

    /// Store the software cursor sprite (premultiplied BGRA) for BlendCursor.
    /// No hardware cursor resource, no UPDATE_CURSOR — avoids the QEMU virtio-gpu
    /// quirk. The sprite is composited into the display plane each frame.
    pub fn store_cursor_sprite(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<(), GfxError> {
        let needed = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if needed == 0 || bgra.len() < needed {
            return Err(GfxError::InvalidArgument);
        }
        self.cursor_sprite.clear();
        self.cursor_sprite.extend_from_slice(&bgra[..needed]);
        self.cursor_sprite_w = width;
        self.cursor_sprite_h = height;
        Ok(())
    }

    /// Phase 6: Upload cursor bitmap as a hardware cursor resource.
    /// Creates a small resource, uploads the bitmap, and calls UPDATE_CURSOR.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn upload_cursor(&mut self, bgra: &[u8], width: u32, height: u32, hot_x: u32, hot_y: u32) -> Result<(), GfxError> {
        if bgra.len() < (width * height * 4) as usize {
            return Err(GfxError::InvalidArgument);
        }
        // Create cursor resource
        let rid = self.create_resource(width, height, PixelFormat::Bgra8888)?;
        let record = self.find_resource(rid).ok_or(GfxError::InvalidArgument)?;
        // Upload bitmap via TRANSFER_TO_HOST_2D
        let full = Rect { x: 0, y: 0, width, height };
        self.transfer_to_host_os(record, full)?;
        // Copy bitmap into the resource's backing memory
        unsafe {
            let dst = core::slice::from_raw_parts_mut(
                record.backing_va as *mut u8,
                (width * height * 4) as usize,
            );
            dst.copy_from_slice(&bgra[..(width * height * 4) as usize]);
        }
        // Call UPDATE_CURSOR
        let cmd = protocol::VirtioGpuUpdateCursor {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_UPDATE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x: 0, y: 0, _padding: 0 },
            resource_id: rid.0,
            hot_x,
            hot_y,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)?;
        Ok(())
    }

    /// Phase 6: Move the hardware cursor to a new position.
    /// Uses the cursor resource uploaded via upload_cursor().
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn move_hw_cursor(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuCursorPos {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x, y, _padding: 0 },
            resource_id: 1, // use resource id 1 (cursor resource)
            hot_x: 0,
            hot_y: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn tile_color_from_fragment(&self) -> [u8; 4] {
        let sidebar_opacity = f32::from_le_bytes([
            self.fragment_data[12], self.fragment_data[13],
            self.fragment_data[14], self.fragment_data[15],
        ]);
        let alpha = (sidebar_opacity.clamp(0.0, 1.0) * 192.0) as u8;
        if alpha > 0 { [200, 220, 255, alpha] } else { [0, 0, 0, 0] }
    }
}

impl GfxBackend for VirtioGpuBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        cmd.validate().map_err(map_nexus_error)?;
        // Phase 6d: honest fence lifecycle — pending until commands complete.
        let mut fence = Fence::new_unsignaled();
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        {
            self.execute_commands(cmd.commands())?;
        }
        fence.signal();
        Ok(fence)
    }

    fn create_resource(
        &mut self,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        if self.resources.len() >= 4 {
            return Err(GfxError::ResourceExhausted);
        }
        let _byte_len = resource_byte_len(w, h)?;
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        let (backing_va, backing_pa, backing_len, backing_vmo) =
            self.create_resource_os(id, w, h, fmt, _byte_len)?;
        self.resources.push(ResourceRecord {
            id,
            width: w,
            height: h,
            format: fmt,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_va,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_pa,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_len,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_vmo,
        });
        Ok(id)
    }

    fn transfer_to_host(&mut self, res: ResourceId, rect: Rect) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        let record = self.find_resource(res).ok_or(GfxError::InvalidArgument)?;
        validate_rect(record, rect)?;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.transfer_to_host_os(record, rect)?;
        Ok(())
    }

    fn set_scanout(&mut self, res: ResourceId) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        let record = self.find_resource(res).ok_or(GfxError::InvalidArgument)?;
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let _ = record.format;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.set_scanout_os(record)?;
        Ok(())
    }

    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        if x < 0 || y < 0 {
            return Err(GfxError::InvalidArgument);
        }
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.move_cursor_os(x as u32, y as u32)?;
        Ok(())
    }
}

fn resource_byte_len(w: u32, h: u32) -> Result<usize, GfxError> {
    let pixels = u64::from(w).checked_mul(u64::from(h)).ok_or(GfxError::ResourceExhausted)?;
    let bytes = pixels.checked_mul(4).ok_or(GfxError::ResourceExhausted)?;
    if bytes == 0 || bytes > 16 * 1024 * 1024 {
        return Err(GfxError::ResourceExhausted);
    }
    Ok(bytes as usize)
}

fn validate_rect(record: ResourceRecord, rect: Rect) -> Result<(), GfxError> {
    let end_x = rect.x.checked_add(rect.width).ok_or(GfxError::InvalidArgument)?;
    let end_y = rect.y.checked_add(rect.height).ok_or(GfxError::InvalidArgument)?;
    if rect.width == 0 || rect.height == 0 || end_x > record.width || end_y > record.height {
        return Err(GfxError::InvalidArgument);
    }
    Ok(())
}

fn map_nexus_error(err: nexus_gfx::GfxError) -> GfxError {
    match err {
        nexus_gfx::GfxError::DeviceNotFound => GfxError::DeviceNotFound,
        nexus_gfx::GfxError::CommandRejected => GfxError::CommandRejected,
        nexus_gfx::GfxError::ResourceExhausted => GfxError::ResourceExhausted,
        nexus_gfx::GfxError::Unsupported => GfxError::Unsupported,
        nexus_gfx::GfxError::InvalidArgument => GfxError::InvalidArgument,
        nexus_gfx::GfxError::MmioFault => GfxError::MmioFault,
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const BOOT_FONT_W: i32 = 5;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const BOOT_FONT_SPACING: i32 = 1;

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn draw_centered_bootstrap_line(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    top_y: i32,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    if scale == 0 {
        return;
    }
    let scale_i = scale as i32;
    let text_w = bootstrap_text_width(text, scale_i);
    let start_x = (width as i32 - text_w) / 2;
    draw_bootstrap_text(pixels, width, height, start_x, top_y, text, scale_i, color);
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn bootstrap_text_width(text: &str, scale: i32) -> i32 {
    let count = text.chars().count() as i32;
    if count <= 0 {
        return 0;
    }
    count * (BOOT_FONT_W + BOOT_FONT_SPACING) * scale - BOOT_FONT_SPACING * scale
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn draw_bootstrap_text(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    scale: i32,
    color: [u8; 4],
) {
    let mut pen_x = x;
    for ch in text.chars() {
        draw_bootstrap_char(pixels, width, height, pen_x, y, ch, scale, color);
        pen_x += (BOOT_FONT_W + BOOT_FONT_SPACING) * scale;
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn draw_bootstrap_char(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    ch: char,
    scale: i32,
    color: [u8; 4],
) {
    let glyph = bootstrap_glyph(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..BOOT_FONT_W {
            let mask = 1u8 << (BOOT_FONT_W - 1 - col);
            if bits & mask == 0 {
                continue;
            }
            let px = x + col * scale;
            let py = y + row as i32 * scale;
            for dy in 0..scale {
                for dx in 0..scale {
                    put_bootstrap_pixel(pixels, width, height, px + dx, py + dy, color);
                }
            }
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn put_bootstrap_pixel(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: [u8; 4],
) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let idx = ((y as usize * width as usize) + x as usize) * 4;
    if idx + 4 <= pixels.len() {
        pixels[idx..idx + 4].copy_from_slice(&color);
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn bootstrap_glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const CTRL_QUEUE_INDEX: u32 = 0;
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(dead_code)]
const CURSOR_QUEUE_INDEX: u32 = 1;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const QUEUE_LEN: usize = 4;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_QUEUE_VA: usize = 0x2030_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_CMD_VA: usize = 0x2031_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESP_VA: usize = 0x2031_1000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESOURCE_BASE_VA: usize = 0x2040_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESOURCE_STRIDE: usize = 0x0100_0000;

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
#[derive(Clone, Copy)]
struct VqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
struct VqAvail<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [u16; N],
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
#[derive(Clone, Copy)]
struct VqUsedElem {
    id: u32,
    len: u32,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
struct VqUsed<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [VqUsedElem; N],
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
struct CtrlQueue {
    _queue_vmo: u32,
    _cmd_vmo: u32,
    _resp_vmo: u32,
    desc: *mut VqDesc,
    avail: *mut VqAvail<QUEUE_LEN>,
    used: *mut VqUsed<QUEUE_LEN>,
    cmd_va: usize,
    cmd_pa: u64,
    resp_va: usize,
    resp_pa: u64,
    last_used: u16,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl VirtioGpuBackend {
    fn probe_os(&mut self) -> Result<(), GpuDriverError> {
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_MAGIC_VALUE)
            != protocol::VIRTIO_MMIO_MAGIC
        {
            return Err(GpuDriverError::DeviceNotFound);
        }
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_ID)
            != protocol::VIRTIO_GPU_DEVICE_ID
        {
            return Err(GpuDriverError::DeviceNotFound);
        }
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, 0);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, 1 | 2);
        write_reg(self.mmio_base, 0x024, 0);
        write_reg(self.mmio_base, 0x020, 0);
        write_reg(self.mmio_base, 0x024, 1);
        write_reg(self.mmio_base, 0x020, 0);
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 8);
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS) & 8 == 0 {
            return Err(GpuDriverError::CommandRejected);
        }
        let ctrlq = CtrlQueue::new(self.mmio_base, CTRL_QUEUE_INDEX)?;
        self.ctrlq = Some(ctrlq);
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 4);
        Ok(())
    }

    fn create_resource_os(
        &mut self,
        id: ResourceId,
        w: u32,
        h: u32,
        fmt: PixelFormat,
        byte_len: usize,
    ) -> Result<(usize, u64, usize, u32), GfxError> {
        let resource_index = self.resources.len();
        let backing_len = align_page(byte_len);
        let backing_va = GPU_RESOURCE_BASE_VA + resource_index * GPU_RESOURCE_STRIDE;
        let backing_vmo = nexus_abi::vmo_create(backing_len).map_err(|_e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_CREATE_FAIL);
            GfxError::ResourceExhausted
        })?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len).step_by(4096) {
            nexus_abi::vmo_map_page(backing_vmo, backing_va + offset, offset, flags).map_err(
                |_e| {
                    let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_MAP_FAIL);
                    GfxError::MmioFault
                },
            )?;
        }
        unsafe { core::ptr::write_bytes(backing_va as *mut u8, 0, backing_len) };
        let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(backing_vmo, &mut info).map_err(|_e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_CAP_QUERY_FAIL);
            GfxError::MmioFault
        })?;

        let gpu_format = Self::to_gpu_format(fmt);
        // Debug: emit format and resource_id values
        match gpu_format {
            1 => {
                let _ = nexus_abi::debug_println("gpud: dbg fmt=B8G8R8A8");
            }
            67 => {
                let _ = nexus_abi::debug_println("gpud: dbg fmt=R8G8B8A8");
            }
            _ => {
                let _ = nexus_abi::debug_println("gpud: dbg fmt=UNKNOWN");
            }
        }
        match id.0 {
            0 => {
                let _ = nexus_abi::debug_println("gpud: dbg rid=0");
            }
            1 => {
                let _ = nexus_abi::debug_println("gpud: dbg rid=1");
            }
            _ => {
                let _ = nexus_abi::debug_println("gpud: dbg rid=OTHER");
            }
        }
        let create = protocol::VirtioGpuResourceCreate2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_CREATE_RESOURCE_2D),
            resource_id: id.0,
            format: gpu_format,
            width: w,
            height: h,
        };
        self.ctrl_submit_struct(&create).map_err(|e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_CREATE_CMD_FAIL);
            e
        })?;

        let attach = protocol::VirtioGpuResourceAttachBacking {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: id.0,
            nr_entries: 1,
        };
        let entry =
            protocol::VirtioGpuMemEntry { addr: info.base, length: byte_len as u32, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry).map_err(|e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_ATTACH_CMD_FAIL);
            e
        })?;
        let _ = nexus_abi::debug_println(GPUD_RESOURCE_CREATED);
        Ok((backing_va, info.base, backing_len, backing_vmo))
    }

    fn transfer_to_host_os(&mut self, record: ResourceRecord, rect: Rect) -> Result<(), GfxError> {
        let bytes_per_pixel = 4u64;
        let row_stride = u64::from(record.width)
            .checked_mul(bytes_per_pixel)
            .ok_or(GfxError::ResourceExhausted)?;
        let row_offset = u64::from(rect.y)
            .checked_mul(row_stride)
            .ok_or(GfxError::ResourceExhausted)?;
        let col_offset = u64::from(rect.x)
            .checked_mul(bytes_per_pixel)
            .ok_or(GfxError::ResourceExhausted)?;
        let offset = row_offset.checked_add(col_offset).ok_or(GfxError::ResourceExhausted)?;
        let cmd = protocol::VirtioGpuTransferToHost2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            offset,
            resource_id: record.id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn flush_rect_os(&mut self, record: ResourceRecord, rect: Rect) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuResourceFlush {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            resource_id: record.id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn set_scanout_os(&mut self, record: ResourceRecord) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuSetScanout {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect { x: 0, y: 0, width: record.width, height: record.height },
            scanout_id: 0,
            resource_id: record.id.0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn move_cursor_os(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuCursorPos {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x, y, _padding: 0 },
            resource_id: 0,
            hot_x: 0,
            hot_y: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn ctrl_submit_struct<T>(&mut self, cmd: &T) -> Result<(), GfxError> {
        let bytes = unsafe {
            core::slice::from_raw_parts((cmd as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        self.ctrl_submit_bytes(bytes)
    }

    fn ctrl_submit_pair<A, B>(&mut self, a: &A, b: &B) -> Result<(), GfxError> {
        let a_bytes = unsafe {
            core::slice::from_raw_parts((a as *const A).cast::<u8>(), core::mem::size_of::<A>())
        };
        let b_bytes = unsafe {
            core::slice::from_raw_parts((b as *const B).cast::<u8>(), core::mem::size_of::<B>())
        };
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        queue.submit_two(self.mmio_base, a_bytes, b_bytes)
    }

    fn ctrl_submit_bytes(&mut self, bytes: &[u8]) -> Result<(), GfxError> {
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        queue.submit(self.mmio_base, bytes)
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl CtrlQueue {
    fn new(mmio_base: usize, queue_index: u32) -> Result<Self, GpuDriverError> {
        let q_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let cmd_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let resp_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        nexus_abi::vmo_map_page(q_vmo, GPU_QUEUE_VA, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::vmo_map_page(cmd_vmo, GPU_CMD_VA, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::vmo_map_page(resp_vmo, GPU_RESP_VA, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        let mut q_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        let mut cmd_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        let mut resp_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(q_vmo, &mut q_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(cmd_vmo, &mut cmd_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(resp_vmo, &mut resp_info).map_err(|_| GpuDriverError::MmioFault)?;
        unsafe {
            core::ptr::write_bytes(GPU_QUEUE_VA as *mut u8, 0, 4096);
            core::ptr::write_bytes(GPU_CMD_VA as *mut u8, 0, 4096);
            core::ptr::write_bytes(GPU_RESP_VA as *mut u8, 0, 4096);
        }

        let desc_bytes = core::mem::size_of::<VqDesc>() * QUEUE_LEN;
        let avail_bytes = core::mem::size_of::<VqAvail<QUEUE_LEN>>();
        let used_off = align4(desc_bytes + avail_bytes);
        let desc_va = GPU_QUEUE_VA;
        let avail_va = GPU_QUEUE_VA + desc_bytes;
        let used_va = GPU_QUEUE_VA + used_off;

        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_SEL, queue_index);
        let max = read_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NUM_MAX);
        if max < QUEUE_LEN as u32 {
            return Err(GpuDriverError::ResourceExhausted);
        }
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NUM, QUEUE_LEN as u32);
        write_u64_pair(mmio_base, protocol::VIRTIO_MMIO_QUEUE_DESC_LOW, q_info.base);
        write_u64_pair(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_DRIVER_LOW,
            q_info.base + desc_bytes as u64,
        );
        write_u64_pair(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_DEVICE_LOW,
            q_info.base + used_off as u64,
        );
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_READY, 1);

        Ok(Self {
            _queue_vmo: q_vmo,
            _cmd_vmo: cmd_vmo,
            _resp_vmo: resp_vmo,
            desc: desc_va as *mut VqDesc,
            avail: avail_va as *mut VqAvail<QUEUE_LEN>,
            used: used_va as *mut VqUsed<QUEUE_LEN>,
            cmd_va: GPU_CMD_VA,
            cmd_pa: cmd_info.base,
            resp_va: GPU_RESP_VA,
            resp_pa: resp_info.base,
            last_used: 0,
        })
    }

    fn submit(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<(), GfxError> {
        self.submit_two(mmio_base, bytes, &[])
    }

    fn submit_two(
        &mut self,
        mmio_base: usize,
        first: &[u8],
        second: &[u8],
    ) -> Result<(), GfxError> {
        let total = first.len().checked_add(second.len()).ok_or(GfxError::ResourceExhausted)?;
        if total == 0 || total > 4096 || core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() > total
        {
            return Err(GfxError::CommandRejected);
        }
        unsafe {
            core::ptr::write_bytes(self.cmd_va as *mut u8, 0, 4096);
            core::ptr::write_bytes(self.resp_va as *mut u8, 0, 4096);
            core::ptr::copy_nonoverlapping(first.as_ptr(), self.cmd_va as *mut u8, first.len());
            if !second.is_empty() {
                core::ptr::copy_nonoverlapping(
                    second.as_ptr(),
                    (self.cmd_va + first.len()) as *mut u8,
                    second.len(),
                );
            }
            core::ptr::write_volatile(
                self.desc.add(0),
                VqDesc { addr: self.cmd_pa, len: total as u32, flags: 1, next: 1 },
            );
            core::ptr::write_volatile(
                self.desc.add(1),
                VqDesc {
                    addr: self.resp_pa,
                    len: core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() as u32,
                    flags: 2,
                    next: 0,
                },
            );
            let idx = core::ptr::read_volatile(&(*self.avail).idx);
            core::ptr::write_volatile(&mut (*self.avail).ring[(idx as usize) % QUEUE_LEN], 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*self.avail).idx, idx.wrapping_add(1));
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NOTIFY, CTRL_QUEUE_INDEX);
        // Log command type: first[0..4] = virtio-gpu cmd type u32le.
        // Allocation-free: this runs for every GPU command on the per-frame
        // submit path, and gpud's bump allocator never frees — a `format!` here
        // would leak a String per command and exhaust the heap mid-animation.
        if first.len() >= 16 {
            let cmd_type = u32::from_le_bytes([first[0], first[1], first[2], first[3]]);
            const PREFIX: &[u8] = b"gpud: submitting cmd=0x";
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut line = [0u8; PREFIX.len() + 4];
            line[..PREFIX.len()].copy_from_slice(PREFIX);
            for i in 0..4 {
                let nibble = (cmd_type >> ((3 - i) * 4)) & 0xf;
                line[PREFIX.len() + i] = HEX[nibble as usize];
            }
            if let Ok(s) = core::str::from_utf8(&line) {
                let _ = nexus_abi::debug_println(s);
            }
        }
        self.wait_complete()
    }

    fn wait_complete(&mut self) -> Result<(), GfxError> {
        self.wait_complete_labeled("ctrl")
    }

    fn wait_complete_labeled(&mut self, label: &str) -> Result<(), GfxError> {
        let start = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
        let deadline = start.saturating_add(500_000_000);
        loop {
            let used_idx = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
            if used_idx != self.last_used {
                self.last_used = used_idx;
                let hdr = unsafe {
                    core::ptr::read_volatile(self.resp_va as *const protocol::VirtioGpuCtrlHdr)
                };
                if hdr.type_ == protocol::VIRTIO_GPU_RESP_OK_NODATA {
                    return Ok(());
                }
                // Debug: classify the error response from QEMU
                match hdr.type_ {
                    0x1200 => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_UNSPEC");
                        // Log which command was rejected
                        let _ = nexus_abi::debug_println(label);
                    }
                    0x1201 => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_OUT_OF_MEMORY");
                    }
                    0x1202 => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_SCANOUT_ID");
                    }
                    0x1203 => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_RESOURCE_ID");
                    }
                    0x1204 => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_CONTEXT_ID");
                    }
                    0x1205 => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=ERR_INVALID_PARAMETER");
                    }
                    _ => {
                        let _ = nexus_abi::debug_println("gpud: dbg resp=UNKNOWN");
                    }
                }
                return Err(GfxError::CommandRejected);
            }
            let now = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
            if now >= deadline {
                return Err(GfxError::MmioFault);
            }
            let _ = nexus_abi::yield_();
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn ctrl_hdr(type_: u32) -> protocol::VirtioGpuCtrlHdr {
    protocol::VirtioGpuCtrlHdr { type_, flags: 0, fence_id: 0, ctx_id: 0, _padding: 0 }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn read_reg(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn write_reg(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn write_u64_pair(base: usize, low_reg: usize, value: u64) {
    write_reg(base, low_reg, value as u32);
    write_reg(base, low_reg + 4, (value >> 32) as u32);
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

// ── Phase 6c: VMO-backed rendering primitives ──────────────────────
// These operate directly on the framebuffer VMO backing memory via raw
// pointers. They are the OS equivalent of CpuMockBackend's rendering
// methods, with the same deterministic semantics.

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn fill_rect_solid_vmo(
    fb: *mut u8, fb_len: usize, fb_w: usize,
    x: u32, y: u32, w: u32, h: u32, color: [u8; 4],
) {
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    for py in y..end_y {
        let row_base = py as usize * fb_w;
        for px in x..end_x {
            let idx = (row_base + px as usize) * 4;
            if idx + 4 <= fb_len {
                unsafe {
                    core::ptr::write_volatile(fb.add(idx), color[0]);
                    core::ptr::write_volatile(fb.add(idx + 1), color[1]);
                    core::ptr::write_volatile(fb.add(idx + 2), color[2]);
                    core::ptr::write_volatile(fb.add(idx + 3), color[3]);
                }
            }
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn fill_sdf_rounded_vmo(
    fb: *mut u8, fb_len: usize, fb_w: usize,
    x: u32, y: u32, w: u32, h: u32, radius: u32, color: RgbaColor,
) {
    let rgba = color.as_array();
    if rgba[3] == 0 { return; }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius.min(w / 2).min(h / 2) as i32;
    let cx = x as i32 + r;
    let cy = y as i32 + r;
    let cx2 = x as i32 + w as i32 - r - 1;
    let cy2 = y as i32 + h as i32 - r - 1;
    for py in y..end_y {
        let row_base = py as usize * fb_w;
        for px in x..end_x {
            let idx = (row_base + px as usize) * 4;
            if idx + 4 > fb_len { continue; }
            let inside = if r <= 0 {
                true
            } else {
                let px_i = px as i32;
                let py_i = py as i32;
                let d = if px_i <= cx && py_i <= cy {
                    corner_dist_i32(px_i, py_i, cx, cy, r)
                } else if px_i >= cx2 && py_i <= cy {
                    corner_dist_i32(px_i, py_i, cx2, cy, r)
                } else if px_i <= cx && py_i >= cy2 {
                    corner_dist_i32(px_i, py_i, cx, cy2, r)
                } else if px_i >= cx2 && py_i >= cy2 {
                    corner_dist_i32(px_i, py_i, cx2, cy2, r)
                } else {
                    0
                };
                d <= 0
            };
            if inside {
                blend_pixel_vmo(fb, idx, &rgba);
            }
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn blur_backdrop_vmo(
    fb: *mut u8, fb_len: usize, fb_w: usize,
    x: u32, y: u32, w: u32, h: u32, radius: u32, _saturation_pct: u32,
) -> Result<(), GfxError> {
    if radius == 0 { return Ok(()); }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius as usize;
    let pixels = (end_x - x) as usize;
    if pixels == 0 { return Ok(()); }
    // Horizontal pass: box-blur each row in-place with a scratch buffer.
    // Allocate on stack — worst case 1280*4 = 5120 bytes for a full-width row.
    let mut scratch: [u8; 5120] = [0u8; 5120];
    let row_bytes = pixels * 4;
    if row_bytes > scratch.len() { return Err(GfxError::ResourceExhausted); }
    for py in y..end_y {
        let row_start = (py as usize * fb_w + x as usize) * 4;
        if row_start + row_bytes > fb_len { continue; }
        unsafe { core::ptr::copy_nonoverlapping(fb.add(row_start), scratch.as_mut_ptr(), row_bytes); }
        let mut sums: [u64; 4] = [0; 4];
        let mut left: usize = 0;
        let mut right = r.min(pixels.saturating_sub(1));
        for j in left..=right {
            let bi = j * 4;
            for c in 0..4 { sums[c] += scratch[bi + c] as u64; }
        }
        for i in 0..pixels {
            let count = (right - left + 1) as u64;
            let di = row_start + i * 4;
            for c in 0..4 {
                unsafe { core::ptr::write_volatile(fb.add(di + c), (sums[c] / count.max(1)).min(255) as u8); }
            }
            if i + 1 < pixels {
                let next_left = (i + 1).saturating_sub(r);
                if next_left > left {
                    let bi = left * 4;
                    for c in 0..4 { sums[c] = sums[c].saturating_sub(scratch[bi + c] as u64); }
                    left = next_left;
                }
                let next_right = (i + 1 + r).min(pixels.saturating_sub(1));
                if next_right > right {
                    right = next_right;
                    let bi = right * 4;
                    for c in 0..4 { sums[c] += scratch[bi + c] as u64; }
                }
            }
        }
    }
    // Vertical pass
    let col_h = (end_y - y) as usize;
    let mut col_buf: [u8; 3200] = [0u8; 3200]; // 800 rows * 4 bytes
    if col_h * 4 > col_buf.len() { return Err(GfxError::ResourceExhausted); }
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..col_h {
            let src = (y as usize + row_i) * fb_w + col_off;
            if src + 4 <= fb_len {
                unsafe { core::ptr::copy_nonoverlapping(fb.add(src), col_buf.as_mut_ptr().add(row_i * 4), 4); }
            }
        }
        let mut sums: [u64; 4] = [0; 4];
        let mut top: usize = 0;
        let mut bot = r.min(col_h.saturating_sub(1));
        for j in top..=bot {
            let bi = j * 4;
            for c in 0..4 { sums[c] += col_buf[bi + c] as u64; }
        }
        for i in 0..col_h {
            let count = (bot - top + 1) as u64;
            let dst = (y as usize + i) * fb_w + col_off;
            for c in 0..4 {
                unsafe { core::ptr::write_volatile(fb.add(dst + c), (sums[c] / count.max(1)).min(255) as u8); }
            }
            if i + 1 < col_h {
                let ntop = (i + 1).saturating_sub(r);
                if ntop > top {
                    let bi = top * 4;
                    for c in 0..4 { sums[c] = sums[c].saturating_sub(col_buf[bi + c] as u64); }
                    top = ntop;
                }
                let nbot = (i + 1 + r).min(col_h.saturating_sub(1));
                if nbot > bot {
                    bot = nbot;
                    let bi = bot * 4;
                    for c in 0..4 { sums[c] += col_buf[bi + c] as u64; }
                }
            }
        }
    }
    Ok(())
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn blit_vmo(
    fb: *mut u8, fb_len: usize, fb_w: usize,
    src_x: u32, src_y: u32, dst_x: u32, dst_y: u32, w: u32, h: u32,
) -> Result<(), GfxError> {
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(dst_x)).min(fb_w_u.saturating_sub(src_x));
    let copy_h = h.min(fb_h.saturating_sub(dst_y)).min(fb_h.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 { return Ok(()); }
    // Use stack scratch for row copy to handle overlapping regions safely.
    let row_bytes = copy_w as usize * 4;
    let mut buf: [u8; 5120] = [0u8; 5120];
    if row_bytes > buf.len() { return Err(GfxError::ResourceExhausted); }
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = (sy as usize * fb_w + src_x as usize) * 4;
        let dst_off = (dy as usize * fb_w + dst_x as usize) * 4;
        if src_off + row_bytes > fb_len || dst_off + row_bytes > fb_len { continue; }
        unsafe { core::ptr::copy_nonoverlapping(fb.add(src_off), buf.as_mut_ptr(), row_bytes); }
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), fb.add(dst_off), row_bytes); }
    }
    Ok(())
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(clippy::too_many_arguments)]
fn blend_cursor_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    sprite: &[u8],
    sprite_w: u32,
    sprite_h: u32,
) -> Result<(), GfxError> {
    if w == 0 || h == 0 {
        return Ok(());
    }
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(x));
    let copy_h = h.min(fb_h.saturating_sub(y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }

    // Prefer the real uploaded cursor sprite (premultiplied BGRA from the Mocu
    // SVG). Fall back to the procedural arrow only until the sprite is uploaded.
    let use_sprite = !sprite.is_empty()
        && sprite_w > 0
        && sprite_h > 0
        && sprite.len() >= (sprite_w as usize * sprite_h as usize * 4);

    for py in 0..copy_h {
        for px in 0..copy_w {
            let idx = ((y as usize + py as usize) * fb_w + (x as usize + px as usize)) * 4;
            if idx + 4 > fb_len {
                continue;
            }
            if use_sprite {
                if px >= sprite_w || py >= sprite_h {
                    continue;
                }
                let s = (py as usize * sprite_w as usize + px as usize) * 4;
                let a = sprite[s + 3];
                if a == 0 {
                    continue;
                }
                // Source is premultiplied: out = src + dst*(255-a)/255.
                blend_premultiplied_vmo(fb, idx, &[sprite[s], sprite[s + 1], sprite[s + 2], a]);
            } else {
                let color = cursor_pixel_bgra(px, py, w, h);
                if color[3] == 0 {
                    continue;
                }
                blend_pixel_vmo(fb, idx, &color);
            }
        }
    }
    Ok(())
}

/// Composite a premultiplied-alpha BGRA pixel over the destination:
/// out_channel = src_channel + dst_channel * (255 - alpha) / 255.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn blend_premultiplied_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    let inv = 255u32 - src[3] as u32;
    unsafe {
        let b = core::ptr::read_volatile(fb.add(idx)) as u32;
        let g = core::ptr::read_volatile(fb.add(idx + 1)) as u32;
        let r = core::ptr::read_volatile(fb.add(idx + 2)) as u32;
        // (x*257)>>16 ≈ x/255 with rounding (+32768), matching blend_pixel_vmo.
        let out_b = src[0] as u32 + ((inv * b * 257 + 32768) >> 16);
        let out_g = src[1] as u32 + ((inv * g * 257 + 32768) >> 16);
        let out_r = src[2] as u32 + ((inv * r * 257 + 32768) >> 16);
        core::ptr::write_volatile(fb.add(idx), out_b.min(255) as u8);
        core::ptr::write_volatile(fb.add(idx + 1), out_g.min(255) as u8);
        core::ptr::write_volatile(fb.add(idx + 2), out_r.min(255) as u8);
        core::ptr::write_volatile(fb.add(idx + 3), 255);
    }
}

/// Classic left-pointer arrow sprite, 12×19, tip at (0,0).
/// `B` = dark border, `W` = white fill, space = transparent. This is a fixed
/// crisp shape so the cursor reads as a normal pointer regardless of the 32×32
/// footprint windowd reserves — the opaque arrow occupies only the top-left.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const CURSOR_ARROW: [&[u8; 12]; 19] = [
    b"B           ",
    b"BB          ",
    b"BWB         ",
    b"BWWB        ",
    b"BWWWB       ",
    b"BWWWWB      ",
    b"BWWWWWB     ",
    b"BWWWWWWB    ",
    b"BWWWWWWWB   ",
    b"BWWWWWWWWB  ",
    b"BWWWWWBBBBB ",
    b"BWWBWWB     ",
    b"BWB BWWB    ",
    b"BB  BWWB    ",
    b"B    BWWB   ",
    b"     BWWB   ",
    b"      BWWB  ",
    b"      BWWB  ",
    b"       BB   ",
];

/// Sample the arrow sprite at (px, py). Pixels outside the 12×19 shape (or in a
/// space cell) are fully transparent, so the cursor never fills its whole box.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn cursor_pixel_bgra(px: u32, py: u32, _w: u32, _h: u32) -> [u8; 4] {
    if py >= CURSOR_ARROW.len() as u32 || px >= 12 {
        return [0, 0, 0, 0];
    }
    match CURSOR_ARROW[py as usize][px as usize] {
        b'B' => [40, 40, 40, 255],     // soft dark border
        b'W' => [255, 255, 255, 255],  // white fill
        _ => [0, 0, 0, 0],             // transparent
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn blend_pixel_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    let alpha = src[3] as u32;
    if alpha == 0 { return; }
    if alpha >= 255 {
        unsafe {
            core::ptr::write_volatile(fb.add(idx), src[0]);
            core::ptr::write_volatile(fb.add(idx + 1), src[1]);
            core::ptr::write_volatile(fb.add(idx + 2), src[2]);
            core::ptr::write_volatile(fb.add(idx + 3), src[3]);
        }
    } else {
        let inv = 255 - alpha;
        unsafe {
            let b = core::ptr::read_volatile(fb.add(idx)) as u32;
            let g = core::ptr::read_volatile(fb.add(idx + 1)) as u32;
            let r = core::ptr::read_volatile(fb.add(idx + 2)) as u32;
            // Phase 6e: fixed-point blend — (x*257)>>16 replaces /255.
            // 257/65536 ≈ 1/255 with <0.002% error. Multiplies by 257 with
            // rounding (+32768 before shift) for 8-bit color accuracy.
            let blend_b = ((alpha * src[0] as u32 + inv * b) * 257 + 32768) >> 16;
            let blend_g = ((alpha * src[1] as u32 + inv * g) * 257 + 32768) >> 16;
            let blend_r = ((alpha * src[2] as u32 + inv * r) * 257 + 32768) >> 16;
            core::ptr::write_volatile(fb.add(idx), blend_b as u8);
            core::ptr::write_volatile(fb.add(idx + 1), blend_g as u8);
            core::ptr::write_volatile(fb.add(idx + 2), blend_r as u8);
            let dst_alpha = core::ptr::read_volatile(fb.add(idx + 3)) as u32;
            core::ptr::write_volatile(fb.add(idx + 3), src[3].saturating_add(
                (((inv * dst_alpha) * 257 + 32768) >> 16) as u8,
            ));
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn corner_dist_i32(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy - r * r
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const fn align_page(value: usize) -> usize {
    (value + 4095) & !4095
}