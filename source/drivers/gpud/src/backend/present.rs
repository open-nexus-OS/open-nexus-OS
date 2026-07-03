// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The present/compositing path: external-framebuffer attach, scanout damage
//! present (host + os variants), committed-buffer present + command execution,
//! the retained render-target layer composite, and the scanout VMO accessor.

use super::resources::{map_nexus_error, validate_rect};
use super::{ResourceRecord, VirtioGpuBackend};
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
use super::{MAX_PENDING_RT_LAYERS, PendingRtLayer};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::command::buffer::{Command, CommittedBuffer, RgbaColor};
use nexus_gfx::core::fence::Fence;
use nexus_gfx::core::types::PixelFormat;

#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::cursor::blend_cursor_vmo;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::raster::{
    blit_blend_vmo, blit_vmo, blur_backdrop_separable_vmo, blur_backdrop_vmo, fill_rect_solid_vmo,
    fill_sdf_rounded_vmo,
};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::transport::{
    align_page, ctrl_hdr, DISPLAY_PLANE_HEIGHT, DISPLAY_PLANE_ROW, GPU_RESOURCE_BASE_VA,
    GPU_RESOURCE_STRIDE,
};
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(unused_imports)]
use crate::markers::{
    GPUD_DROPSHADOW_OK, GPUD_GL_SCANOUT_FALLBACK, GPUD_LAYER_COMPOSITE_LIVE,
    GPUD_RESOURCE_VMO_MAP_FAIL, GPUD_SDF_GRAD_OK,
};

impl VirtioGpuBackend {
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
        // windowd's framebuffer replaces the bootstrap scanout — stop the 2D
        // splash pulse (the GL hold phase takes the breathing over from here)
        // and remember the dead splash resource: its 4MB one-shot backing goes
        // back to the kernel arena once the new scanout is live (task #124).
        let dead_splash = if self.bootstrap_splash_live { self.scanout_resource } else { None };
        self.bootstrap_splash_live = false;
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
        let resource_index = self.alloc_resource_va_index()?;
        let backing_va = GPU_RESOURCE_BASE_VA + resource_index * GPU_RESOURCE_STRIDE;
        let backing_len_aligned = align_page((width * height * 4) as usize);
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len_aligned).step_by(4096) {
            nexus_abi::vmo_map_page(vmo_slot, backing_va + offset, offset, flags).map_err(
                |_e| {
                    let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_MAP_FAIL);
                    GfxError::MmioFault
                },
            )?;
        }

        // The VMO needs a virtio 2D resource ONLY for the non-virgl 2D scanout
        // path. On the virgl path the VMO is read solely as the 3D texture 0xF8
        // (an alias of the same physical pages); creating a 2D resource on that
        // same memory is the "mixing 3D rendering and 2D scanout on one resource"
        // that blacks out the gl device (see the comment below). So skip it when
        // virgl drives the scanout — leaving 0xF8 (3D) as the sole resource on
        // that memory. The non-virgl/mmio build keeps the clean 2D path.
        #[cfg(feature = "virgl")]
        let use_virgl_scanout = self.virgl_capable && self.virgl_draw_ok;
        #[cfg(not(feature = "virgl"))]
        let use_virgl_scanout = false;
        if !use_virgl_scanout {
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
        }

        // GL-presented scanout (G0/G1): on a virgl-capable device the display
        // is a GPU render target and presents are GPU blits of this VMO — the
        // 2D SET_SCANOUT/transfer/flush below stays the non-virgl path. Mixing
        // 3D rendering and 2D scanout on one resource is what blacked out the
        // gl device.
        #[cfg(feature = "virgl")]
        {
            let record = ResourceRecord {
                id,
                width,
                height,
                format: PixelFormat::Bgra8888,
                backing_va,
                backing_pa: info.base,
                backing_len: (width * height * 4) as usize,
                backing_vmo: 0,
            };
            if self.virgl_capable && self.virgl_draw_ok {
                self.resources.push(record);
                self.scanout_resource = Some(id);
                match self.gl_scanout_init() {
                    Ok(()) => {
                        let _ = nexus_abi::trace_line("gpud: set_scanout ok");
                        let _ = nexus_abi::trace_line("gpud: scanout ok");
                        let _ = nexus_abi::trace_line("gpud: scanout 1280x800 bgra8888");
                        // Absorb the one-time virgl texture-sampling stall (~500ms)
                        // HERE, at boot, so the user's first present/scroll is fast
                        // instead of frozen for half a second.
                        let _ = self.gl_pipeline_warmup();
                        self.gl_present_damage(Rect {
                            x: 0,
                            y: 0,
                            width,
                            height: DISPLAY_PLANE_HEIGHT,
                        })?;
                        let _ = nexus_abi::trace_line("gpud: transfer_to_host ok");
                        let _ = nexus_abi::trace_line("gpud: resource flush ok");
                        // GL scanout is live — the bootstrap splash resource
                        // and its 4MB backing are dead weight now (task #124).
                        if let Some(dead) = dead_splash {
                            self.release_resource(dead);
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        // Name the failing class at the source (RFC-0066 clean
                        // errors): a silent fallback cost a debug cycle once.
                        let _ = nexus_abi::debug_println(match e {
                            GfxError::DeviceNotFound => "gpud: gl init err device-not-found",
                            GfxError::MmioFault => "gpud: gl init err mmio-fault",
                            GfxError::CommandRejected => "gpud: gl init err command-rejected",
                            GfxError::ResourceExhausted => "gpud: gl init err resource-exhausted",
                            GfxError::Unsupported => "gpud: gl init err unsupported",
                            GfxError::InvalidArgument => "gpud: gl init err invalid-argument",
                        });
                        // virgl scanout failed: create the 2D resource that was
                        // skipped above (use_virgl_scanout) so the proven 2D
                        // scanout path below can take over.
                        let create = protocol::VirtioGpuResourceCreate2d {
                            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_CREATE_RESOURCE_2D),
                            resource_id: id.0,
                            format: Self::to_gpu_format(PixelFormat::Bgra8888),
                            width,
                            height,
                        };
                        let _ = self.ctrl_submit_struct(&create);
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
                        let _ = self.ctrl_submit_pair(&attach, &entry);
                        let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_SCANOUT_FALLBACK);
                        self.resources.pop();
                        self.scanout_resource = None;
                    }
                }
            }
        }

        // Activate the scanout first, then commit the initial framebuffer contents.
        // This matches the visible-bootstrap contract more closely: QEMU first learns
        // the target mode/scanout, then receives the content transfer + flush.
        // Phase 3: scanout displays frame ring slot A (rows 1600..2399) in 4-plane VMO.
        let scanout = protocol::VirtioGpuSetScanout {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect {
                x: 0,
                y: DISPLAY_PLANE_ROW,
                width,
                height: DISPLAY_PLANE_HEIGHT,
            },
            scanout_id: 0,
            resource_id: id.0,
        };
        self.ctrl_submit_struct(&scanout).map_err(|_| GfxError::CommandRejected)?;
        let _ = nexus_abi::trace_line("gpud: set_scanout ok");
        let _ = nexus_abi::trace_line("gpud: scanout ok");
        let _ = nexus_abi::trace_line("gpud: scanout 1280x800 bgra8888");

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

        // Transfer the display plane (rows 1600..2399) to host for the first frame.
        self.transfer_to_host_os(
            record,
            Rect { x: 0, y: DISPLAY_PLANE_ROW, width, height: DISPLAY_PLANE_HEIGHT },
        )
        .map_err(|e| {
            let _ =
                nexus_abi::debug_println("gpud: ERROR transfer_to_host for initial frame failed");
            e
        })?;
        let _ = nexus_abi::trace_line("gpud: transfer_to_host ok");

        let flush = protocol::VirtioGpuResourceFlush {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect {
                x: 0,
                y: DISPLAY_PLANE_ROW,
                width,
                height: DISPLAY_PLANE_HEIGHT,
            },
            resource_id: id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&flush).map_err(|_| {
            let _ = nexus_abi::debug_println("gpud: ERROR resource flush failed");
            GfxError::CommandRejected
        })?;
        let _ = nexus_abi::trace_line("gpud: resource flush ok");

        self.resources.push(record);
        self.scanout_resource = Some(id);
        // The 2D scanout switched to windowd's framebuffer — free the dead
        // bootstrap splash resource + its one-shot backing (task #124).
        if let Some(dead) = dead_splash {
            self.release_resource(dead);
        }
        Ok(())
    }

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn present_scanout_damage(&mut self, rect: Rect) -> Result<(), GfxError> {
        // GL-presented scanout (G1): present = VMO upload + GPU blit + flip.
        #[cfg(feature = "virgl")]
        if self.gl_scanout_active {
            return self.gl_present_damage(rect);
        }
        let scanout = self.scanout_resource.ok_or_else(|| {
            let _ = nexus_abi::debug_println(
                "gpud: backend present_scanout_damage: no scanout_resource",
            );
            GfxError::InvalidArgument
        })?;
        let record = self.find_resource(scanout).ok_or(GfxError::InvalidArgument)?;
        // Display plane is at the fixed row DISPLAY_PLANE_ROW (not height/2 — the
        // resource grew to host the atlas, the display plane did not move).
        let display_rect = Rect {
            x: rect.x,
            y: rect.y + DISPLAY_PLANE_ROW,
            width: rect.width,
            height: rect.height,
        };
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

    /// Present a borrowed `CommittedBuffer` (validate + execute) without taking
    /// ownership. Mirrors [`GfxBackend::submit`] but borrows, so the caller can
    /// hold one reusable buffer and `reload_from` it every frame — avoiding the
    /// per-frame `Vec<Command>` that `submit(CommittedBuffer)` would require.
    /// gpud runs on a non-freeing bump allocator, so that per-frame Vec would
    /// otherwise exhaust the heap and crash mid-animation.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
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

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn execute_commands(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        // In the virgl build-up compositor the scanout is the GL render target
        // (`compositor_buildup_present` draws the full frame there each present), so
        // this CPU/VMO command stream is never presented. Several commands (glass
        // blur, drop shadow) also need a per-frame TRANSFER_TO_HOST_3D that
        // intermittently stalls QEMU's virgl renderer (used-ring never advances →
        // the present-damage chain wedges before G4). Skip the whole VMO stream; the
        // GL-RT build-up owns the output. (mmio scans out the VMO, so it still runs.)
        #[cfg(feature = "virgl")]
        if crate::gl_scanout::COMPOSITOR_BUILDUP {
            // The GL-RT build-up owns the scanout, so the CPU/VMO draw stream is
            // never presented (and its per-frame transfers stall virgl). But we
            // STILL collect CompositeLayer ops into `pending_rt_layers` so the
            // build-up present composites the real UI layers (content + shadow +
            // glass backdrop-blur via `blur_rt_backdrop`) straight onto the RT
            // via `composite_pending_rt_layers`.
            // Cursor-move presents carry a minimal command buffer with NO layer
            // commands. The build-up re-renders the whole frame every present, so
            // if we cleared the layer set on those frames the UI would flicker.
            // Replace the retained set only when this frame actually brings layers;
            // otherwise keep the previous set so the UI stays stable.
            if self.gl_scanout_active
                && cmds.iter().any(|c| matches!(c, Command::CompositeLayer { .. }))
            {
                self.pending_rt_count = 0;
                self.rt_layers_dirty = true; // content changed → re-upload once
                                             // A fresh full layer set carries the authoritative scroll offset in
                                             // the scrollable layers' `src_row_abs`, so drop any fast-path overrides.
                self.scroll_src_rows = [None; crate::backend::MAX_SCROLL_IDS];
                for cmd in cmds {
                    if let Command::CompositeLayer {
                        src_row_abs,
                        src_x,
                        width,
                        height,
                        dst_x,
                        dst_y,
                        opacity,
                        corner_radius,
                        shadow_blur,
                        shadow_offset_y,
                        shadow_alpha,
                        backdrop_blur,
                        scroll_id,
                    } = cmd
                    {
                        if self.pending_rt_count < MAX_PENDING_RT_LAYERS {
                            self.pending_rt_layers[self.pending_rt_count] = PendingRtLayer {
                                src_row_abs: *src_row_abs,
                                src_x: *src_x,
                                width: *width,
                                height: *height,
                                dst_x: *dst_x,
                                dst_y: *dst_y,
                                opacity: *opacity,
                                corner_radius: *corner_radius,
                                shadow_blur: *shadow_blur,
                                shadow_offset_y: *shadow_offset_y,
                                shadow_alpha: *shadow_alpha,
                                backdrop_blur: *backdrop_blur,
                                scroll_id: *scroll_id,
                            };
                            self.pending_rt_count += 1;
                        }
                    }
                }
            }
            return Ok(());
        }
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        if record.backing_va == 0 {
            return Err(GfxError::MmioFault);
        }
        let fb = record.backing_va as *mut u8;
        let fb_len = record.backing_len;
        let fb_w = record.width as usize;
        let display_y_offset = DISPLAY_PLANE_ROW;
        for cmd in cmds {
            match cmd {
                Command::SetFragmentBytes { offset, data } => {
                    let end = offset.saturating_add(data.len());
                    if end > self.fragment_data.len() {
                        return Err(GfxError::CommandRejected);
                    }
                    self.fragment_data[*offset..end].copy_from_slice(data);
                }
                Command::DrawTiles { tiles, color } => {
                    let c = color.as_array();
                    for t in tiles {
                        fill_rect_solid_vmo(
                            fb,
                            fb_len,
                            fb_w,
                            t.x,
                            t.y.saturating_add(display_y_offset),
                            t.width,
                            t.height,
                            c,
                        );
                    }
                }
                Command::FillSdfRoundedRect { rect, radius, color } => {
                    fill_sdf_rounded_vmo(
                        fb,
                        fb_len,
                        fb_w,
                        rect.x,
                        rect.y.saturating_add(display_y_offset),
                        rect.width,
                        rect.height,
                        *radius,
                        *color,
                    );
                }
                Command::BlurBackdrop { rect, radius, saturation_percent } => {
                    // In the virgl build-up compositor the scanout is the GL render
                    // target (`compositor_buildup_present`, which does its own pure-GL
                    // Stage-3 glass blur by sampling a persistent texture — NO transfer),
                    // NOT this CPU/VMO plane. Blurring the VMO here is therefore wasted
                    // work, and `submit_virgl_blur`'s per-frame TRANSFER_TO_HOST_3D
                    // intermittently stalls QEMU's virgl renderer (used-ring never
                    // advances → the present-damage chain never reaches G4). Skip it and
                    // let the GL-RT build-up own the blurred output.
                    #[cfg(feature = "virgl")]
                    let buildup_owns_scanout = crate::gl_scanout::COMPOSITOR_BUILDUP;
                    #[cfg(not(feature = "virgl"))]
                    let buildup_owns_scanout = false;
                    if !buildup_owns_scanout {
                        // GPU-accelerated shader when virgl is compiled in and the
                        // context exists; otherwise separable gaussian → box-blur.
                        #[cfg(feature = "virgl")]
                        let virgl_ok = self.virgl_capable
                            && self
                                .submit_virgl_blur(
                                    fb,
                                    fb_len,
                                    fb_w,
                                    rect.x,
                                    rect.y.saturating_add(display_y_offset),
                                    rect.width,
                                    rect.height,
                                    *radius,
                                    true,
                                )
                                .is_ok();
                        #[cfg(not(feature = "virgl"))]
                        let virgl_ok = false;

                        if !virgl_ok {
                            #[cfg(feature = "virgl")]
                            let use_separable = self.virgl_capable;
                            #[cfg(not(feature = "virgl"))]
                            let use_separable = false;

                            if use_separable {
                                blur_backdrop_separable_vmo(
                                    fb,
                                    fb_len,
                                    fb_w,
                                    rect.x,
                                    rect.y.saturating_add(display_y_offset),
                                    rect.width,
                                    rect.height,
                                    *radius,
                                    *saturation_percent,
                                )?;
                            } else {
                                blur_backdrop_vmo(
                                    fb,
                                    fb_len,
                                    fb_w,
                                    rect.x,
                                    rect.y.saturating_add(display_y_offset),
                                    rect.width,
                                    rect.height,
                                    *radius,
                                    *saturation_percent,
                                )?;
                            }
                        }
                    }
                }
                Command::BlitSurface { src_x, src_y, dst_x, dst_y, width, height } => {
                    // Retained-surface composite: src_y is an absolute VMO row
                    // (windowd points it at the retained plane, rows 800..1599).
                    // dst_y is screen-relative; add display_y_offset so the copy
                    // lands in the display plane (Plane 2, rows 1600..2399).
                    blit_vmo(
                        fb,
                        fb_len,
                        fb_w,
                        *src_x,
                        *src_y,
                        *dst_x,
                        dst_y.saturating_add(display_y_offset),
                        *width,
                        *height,
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
                    blit_vmo(
                        fb, fb_len, fb_w, *src_x, *src_y_abs, *dst_x, *dst_y_abs, *width, *height,
                    )?;
                }
                Command::FillSdfGradient { rect, radius, color_top, color_bottom } => {
                    let y_abs = rect.y.saturating_add(display_y_offset);
                    #[cfg(feature = "virgl")]
                    let gpu_ok = self.virgl_capable
                        && self
                            .submit_virgl_sdf_gradient(
                                rect.x,
                                y_abs,
                                rect.width,
                                rect.height,
                                *radius,
                                *color_top,
                                *color_bottom,
                            )
                            .is_ok();
                    #[cfg(not(feature = "virgl"))]
                    let gpu_ok = false;
                    if gpu_ok {
                        #[cfg(feature = "virgl")]
                        if !self.virgl_grad_marker_done {
                            self.virgl_grad_marker_done = true;
                            let _ = nexus_abi::debug_println(crate::markers::GPUD_SDF_GRAD_OK);
                        }
                    } else {
                        crate::cpu_vector::fill_sdf_gradient_vmo(
                            fb,
                            fb_len,
                            fb_w,
                            rect.x,
                            y_abs,
                            rect.width,
                            rect.height,
                            *radius,
                            *color_top,
                            *color_bottom,
                        );
                    }
                }
                Command::DropShadow { rect, radius, blur, offset_x, offset_y, color } => {
                    let y_abs = rect.y.saturating_add(display_y_offset);
                    #[cfg(feature = "virgl")]
                    let gpu_ok = self.virgl_capable
                        && self
                            .submit_virgl_drop_shadow(
                                rect.x,
                                y_abs,
                                rect.width,
                                rect.height,
                                *radius,
                                *blur,
                                *offset_x,
                                *offset_y,
                                *color,
                            )
                            .is_ok();
                    #[cfg(not(feature = "virgl"))]
                    let gpu_ok = false;
                    if gpu_ok {
                        #[cfg(feature = "virgl")]
                        if !self.virgl_shadow_marker_done {
                            self.virgl_shadow_marker_done = true;
                            let _ = nexus_abi::debug_println(crate::markers::GPUD_DROPSHADOW_OK);
                        }
                    } else {
                        crate::cpu_vector::drop_shadow_vmo(
                            fb,
                            fb_len,
                            fb_w,
                            rect.x,
                            y_abs,
                            rect.width,
                            rect.height,
                            *radius,
                            *blur,
                            *offset_x,
                            *offset_y,
                            *color,
                            DISPLAY_PLANE_ROW,
                            DISPLAY_PLANE_HEIGHT,
                        );
                    }
                }
                Command::CompositeLayer {
                    src_row_abs,
                    src_x,
                    width,
                    height,
                    dst_x,
                    dst_y,
                    opacity,
                    corner_radius,
                    shadow_blur,
                    shadow_offset_y,
                    shadow_alpha,
                    backdrop_blur,
                    scroll_id,
                } => {
                    // `opacity` is honoured by the GPU path; the CPU fallback
                    // relies on the content's own alpha (translucent panel bg).
                    #[cfg(not(feature = "virgl"))]
                    let _ = opacity;
                    // `scroll_id` only drives the virgl RT-direct fast path below.
                    #[cfg(not(all(feature = "virgl", feature = "os-lite", target_os = "none")))]
                    let _ = scroll_id;
                    // RT-direct (Increment 1): defer non-glass layers and
                    // composite them straight onto the scanout RT after the base
                    // upload — no VMO render + re-upload. Glass (backdrop_blur>0)
                    // still uses the VMO path below until the RT-backdrop lands.
                    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
                    if self.rt_direct_layers
                        && self.gl_scanout_active
                        && *backdrop_blur == 0
                        && self.pending_rt_count < MAX_PENDING_RT_LAYERS
                    {
                        self.pending_rt_layers[self.pending_rt_count] = PendingRtLayer {
                            src_row_abs: *src_row_abs,
                            src_x: *src_x,
                            width: *width,
                            height: *height,
                            dst_x: *dst_x,
                            dst_y: *dst_y,
                            opacity: *opacity,
                            corner_radius: *corner_radius,
                            shadow_blur: *shadow_blur,
                            shadow_offset_y: *shadow_offset_y,
                            shadow_alpha: *shadow_alpha,
                            backdrop_blur: *backdrop_blur,
                            scroll_id: *scroll_id,
                        };
                        self.pending_rt_count += 1;
                        continue; // composited onto the RT in gl_present, not here
                    }
                    // GPU layer compositor op (G2). On virgl the layer is
                    // composited on the GPU into the display-plane surface
                    // (shadow + optional backdrop blur + content texture +
                    // rounded mask + opacity); on the 2D path it falls back to
                    // CPU shadow + optional backdrop blur + an alpha-blended
                    // (or opaque) content blit.
                    let dst_y_abs = dst_y.saturating_add(display_y_offset);
                    #[cfg(feature = "virgl")]
                    let gpu_ok = self.virgl_capable
                        && self.gl_scanout_active
                        && self
                            .composite_layer_gpu(
                                *src_row_abs,
                                *src_x,
                                *width,
                                *height,
                                *dst_x,
                                *dst_y,
                                *opacity,
                                *corner_radius,
                                *shadow_blur,
                                *shadow_offset_y,
                                *shadow_alpha,
                                *backdrop_blur,
                            )
                            .is_ok();
                    #[cfg(not(feature = "virgl"))]
                    let gpu_ok = false;
                    if gpu_ok {
                        #[cfg(feature = "virgl")]
                        if !self.virgl_layer_marker_done {
                            self.virgl_layer_marker_done = true;
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_LIVE);
                        }
                    } else {
                        if *shadow_blur > 0 {
                            crate::cpu_vector::drop_shadow_vmo(
                                fb,
                                fb_len,
                                fb_w,
                                *dst_x,
                                dst_y_abs,
                                *width,
                                *height,
                                *corner_radius,
                                *shadow_blur,
                                0,
                                *shadow_offset_y,
                                RgbaColor::from_u32(((*shadow_alpha).min(255)) << 24),
                                DISPLAY_PLANE_ROW,
                                DISPLAY_PLANE_HEIGHT,
                            );
                        }
                        // Glass: when backdrop_blur>0 the backdrop is blurred
                        // inline here; when 0 the caller already placed a
                        // (cached) blurred backdrop in the display region. Either
                        // way the content is ALPHA-BLENDED over it — opaque
                        // content (alpha 255) blends to opaque, so this is
                        // correct for both glass and solid layers.
                        if *backdrop_blur > 0 {
                            let _ = blur_backdrop_vmo(
                                fb,
                                fb_len,
                                fb_w,
                                *dst_x,
                                dst_y_abs,
                                *width,
                                *height,
                                *backdrop_blur,
                                0,
                            );
                        }
                        let _ = blit_blend_vmo(
                            fb,
                            fb_len,
                            fb_w,
                            *src_x,
                            *src_row_abs,
                            *dst_x,
                            dst_y_abs,
                            *width,
                            *height,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Composite all layers deferred this frame (RT-direct, Increment 1) straight
    /// onto the scanout RT. Called by gl_present AFTER the base upload and the
    /// one-shot parity readback, BEFORE the flush — so the base is on the RT and
    /// parity still compares the clean base. Drains the pending buffer.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn composite_pending_rt_layers(&mut self) {
        let n = self.pending_rt_count;
        let buildup = crate::gl_scanout::COMPOSITOR_BUILDUP;
        // The build-up re-composites every present (cursor moves re-render the
        // whole frame), so the layer set is RETAINED across presents and only
        // replaced when a new frame brings layers; the non-build-up path drains.
        if !buildup {
            self.pending_rt_count = 0;
        }
        // Re-upload the atlas content to the GL texture only when it changed
        // (non-build-up always uploads since it drains per present). A retained
        // cursor-move present composites from the already-uploaded texture — no
        // per-frame transfer, which is what made mouse-move slow.
        let upload = if buildup { self.rt_layers_dirty } else { true };
        for i in 0..n {
            let l = self.pending_rt_layers[i];
            // Scroll fast path: a scrollable layer (non-zero scroll_id) is
            // re-sampled at its id's override row when set — no CPU re-render,
            // just a different source offset into the already-uploaded atlas
            // texture.
            let src_row_abs = match l
                .scroll_id
                .checked_sub(1)
                .and_then(|i| self.scroll_src_rows.get(i as usize).copied().flatten())
            {
                Some(row) => row,
                None => l.src_row_abs,
            };
            // Frosted glass: blur what is beneath this layer's rect (destination-
            // so-far — layers composite back-to-front, so lower windows/chrome are
            // already on the RT) into the glass RT first; the layer's translucent
            // tint + content composite over the blurred backdrop = real frosted
            // glass on the virgl scanout.
            if l.backdrop_blur > 0 {
                let _ = self.blur_rt_backdrop(l.dst_x, l.dst_y, l.width, l.height, l.backdrop_blur);
            }
            let ok = self
                .composite_layer_rt(
                    src_row_abs,
                    l.src_x,
                    l.width,
                    l.height,
                    l.dst_x,
                    l.dst_y,
                    l.opacity,
                    l.corner_radius,
                    l.shadow_blur,
                    l.shadow_offset_y,
                    l.shadow_alpha,
                    upload,
                )
                .is_ok();
            if ok && !self.virgl_layer_marker_done {
                self.virgl_layer_marker_done = true;
                let _ = nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_LIVE);
            }
        }
        if buildup {
            self.rt_layers_dirty = false;
        }
    }

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn scanout_fb(&self) -> Option<(*mut u8, usize, usize, u32)> {
        let scanout = self.scanout_resource?;
        let record = self.find_resource(scanout)?;
        if record.backing_va == 0 {
            return None;
        }
        Some((
            record.backing_va as *mut u8,
            record.backing_len,
            record.width as usize,
            DISPLAY_PLANE_ROW,
        ))
    }
}
