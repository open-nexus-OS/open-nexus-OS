// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: virtio-gpu `GfxBackend` — device probe, resources, scanout, and the
//! multi-entry control-queue command ring (per-slot lifecycle: `enqueue_*` /
//! `harvest` / `alloc_free_slot` / `wait_slot`). The virgl GL compositor present
//! drives this ring in pipelined (enqueue-only) mode; init + mmio drive it
//! synchronously. A future real-GPU backend reimplements `GfxBackend`, not this.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! ADR: docs/adr/0032-gpu-command-ring-and-pipelined-present.md
//! ARCH: docs/architecture/gpud-command-ring-and-present-pipeline.md
//! TESTS: `cargo test -p gpud` (protocol size + Submit3d byte-format goldens);
//!   `tools/nx` `chain_gpu_scanout.rs` (hop-order chain); `scripts/qemu-test.sh`
//!   (`GPU_MODE=virgl` + mmio boot proof: uniform present `max`, 0 alloc-fail).

#![allow(unused_imports)] // os-lite markers only used in OS cfg

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::command::buffer::{Command, CommittedBuffer, RgbaColor};
use nexus_gfx::core::fence::Fence;
use nexus_gfx::core::types::PixelFormat;

use crate::error::GpuDriverError;
use crate::markers::{
    GPUD_CPU_FALLBACK, GPUD_RESOURCE_ATTACH_CMD_FAIL, GPUD_RESOURCE_CAP_QUERY_FAIL,
    GPUD_RESOURCE_CREATED, GPUD_RESOURCE_CREATE_CMD_FAIL, GPUD_RESOURCE_VMO_CREATE_FAIL,
    GPUD_RESOURCE_VMO_MAP_FAIL, GPUD_VIRGL_READY,
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
    /// True when virgl GPU acceleration is detected at probe time.
    /// Requires `virgl` feature + QEMU `-device virtio-gpu-pci,virgl=on`.
    #[allow(dead_code)]
    pub(crate) virgl_capable: bool,
    /// Virgl rendering context ID (0 = not created).
    #[allow(dead_code)]
    pub(crate) virgl_ctx_id: u32,
    resources: alloc::vec::Vec<ResourceRecord>,
    pub(crate) scanout_resource: Option<ResourceId>,
    /// Fragment uniform storage for SetFragmentBytes commands.
    /// Phase 6c: stores shader parameters (animation state) pushed by windowd.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fragment_data: [u8; 64],
    /// Software cursor sprite: the real Mocu SVG cursor (premultiplied BGRA),
    /// uploaded once by windowd. BlendCursor composites this onto the display
    /// plane each frame. Empty until uploaded → procedural arrow fallback.
    pub(crate) cursor_sprite: alloc::vec::Vec<u8>,
    pub(crate) cursor_sprite_w: u32,
    pub(crate) cursor_sprite_h: u32,
    /// Hardware cursor resource (64×64, cursor queue). `None` until a
    /// successful `upload_cursor` arms the overlay. Unused on display backends
    /// where the overlay is not composited into the captured/shown scanout —
    /// there the save-under software cursor below is the live path.
    cursor_resource_id: Option<ResourceId>,
    cursor_hot: (u32, u32),
    /// Save-under software cursor (composited into the scanout, so it is visible
    /// on every display backend). `cursor_ox/oy` are the screen-space top-left of
    /// the drawn sprite; `cursor_saveunder` holds the scene pixels it covers.
    cursor_owned: bool,
    cursor_drawn: bool,
    cursor_suspended: bool,
    pub(crate) cursor_ox: i32,
    pub(crate) cursor_oy: i32,
    cursor_dw: u32,
    cursor_dh: u32,
    /// Frame counter for the build-up spin-blur demo animation (incremented each
    /// build-up present; drives a circular panel offset so the blur re-computes
    /// per frame — a reactive GPU/blur performance test, no input needed). Read
    /// only by the virgl build-up present; inert on the mmio path.
    #[allow(dead_code)]
    pub(crate) buildup_frame: u64,
    /// When set, the control-queue submit helpers ENQUEUE (no per-command wait)
    /// instead of submit-and-drain. A present sets it, enqueues all its SUBMIT_3D
    /// draws + the flush, then drains once — so a textured draw whose completion
    /// QEMU defers no longer blocks the next command. Inert (false) on every other
    /// path, so mmio/init keep the exact synchronous behaviour.
    #[allow(dead_code)]
    ctrl_batch: bool,
    cursor_saveunder: alloc::vec::Vec<u8>,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    ctrlq: Option<CtrlQueue>,
    /// virtio-gpu cursor virtqueue (index 1) — carries UPDATE_CURSOR / MOVE_CURSOR
    /// so the host composites the pointer as a hardware overlay (the GPU hot path).
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    cursorq: Option<CtrlQueue>,
    /// Number of virgl backing VMOs allocated (VA slot allocator).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_backing_count: usize,
    /// True after the boot draw self-test verified a full GPU draw by readback.
    /// Gates the blur pipeline (which reuses the self-test's state objects).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_draw_ok: bool,
    /// True once the blur resources (fb-alias texture, tmp RT, quad, shader)
    /// are created. Lazy: the fb VMO only exists after windowd's handoff.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_blur_ready: bool,
    /// One-shot GPU-vs-CPU blur parity check on first real blur.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_parity_done: bool,
    /// First GPU blur executed (marker bookkeeping, independent of init site).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_blur_first_done: bool,
    /// Vector pipeline objects created (SDF gradient/shadow shaders + alpha blend).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_vector_ready: bool,
    /// Layer compositor objects created (FS_LAYER + alpha blend + sampler).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_composite_ready: bool,
    /// Atlas texture (rows 3200..6399) aliased as a GPU sampler view for layer content.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_atlas_ready: bool,
    /// Cursor sprite uploaded into its own GL sampler texture so the cursor can be
    /// composited as a proper layer (`submit_layer_pass`) instead of a procedural
    /// rect. Backing VA + dimensions latched at the first upload.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) cursor_tex_va: usize,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) cursor_tex_w: u32,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) cursor_tex_h: u32,
    /// First GPU layer composited (marker bookkeeping).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_layer_marker_done: bool,
    /// One-shot markers: first GPU-executed gradient fill / drop shadow.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_grad_marker_done: bool,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_shadow_marker_done: bool,
    /// True once the GL scanout RT owns the display (gl_scanout module, G0).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_scanout_active: bool,
    /// One-shot GL present parity readback done (gl_scanout module, G1).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_present_parity_done: bool,
    /// Guest backing VA of the GL scanout RT (parity readback only).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_scanout_backing_va: usize,
    /// Guest backing VA of the NON-ALIASED display texture (own backing, not a
    /// VMO alias). The present copies windowd's VMO frame here, uploads it, and
    /// blits it to the scanout RT — avoiding the 0xF8 VMO-alias that QEMU's GL
    /// scanout refuses to present (see RFC / the black-screen investigation).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_display_tex_va: usize,
    /// Backing VA of the build-up wallpaper texture (`H_WALLPAPER_TEX`). Lets the
    /// build-up present upload the real wallpaper (windowd's decoded JPEG in
    /// shared-VMO Plane 0) into the GL texture once, replacing the boot bands.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_wallpaper_tex_va: usize,
    /// One-shot latch: the real wallpaper has been copied from VMO Plane 0 into
    /// `H_WALLPAPER_TEX`. Deferred to the first present so windowd has written
    /// Plane 0 (it does so at boot, independent of GPU mode).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) wallpaper_from_vmo_uploaded: bool,
    /// RT-direct layer compositing (true GPU compositing, Increment 1): when set,
    /// `backdrop_blur == 0` CompositeLayer ops are deferred and composited
    /// straight onto the scanout RT after the base upload, instead of rendered
    /// into the VMO and re-uploaded. Reversible kill-switch.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    rt_direct_layers: bool,
    /// Layers deferred this frame for RT-direct compositing (no per-frame alloc:
    /// fixed stack capacity; overflow falls back to the VMO path).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pending_rt_layers: [PendingRtLayer; MAX_PENDING_RT_LAYERS],
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pending_rt_count: usize,
}

/// A CompositeLayer op deferred for RT-direct compositing after the base upload.
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
#[derive(Clone, Copy, Default)]
struct PendingRtLayer {
    src_row_abs: u32,
    src_x: u32,
    width: u32,
    height: u32,
    dst_x: u32,
    dst_y: u32,
    opacity: u32,
    corner_radius: u32,
    shadow_blur: u32,
    shadow_offset_y: i32,
    shadow_alpha: u32,
}

#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
const MAX_PENDING_RT_LAYERS: usize = 8;

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
            virgl_capable: false,
            virgl_ctx_id: 0,
            resources: alloc::vec::Vec::new(),
            scanout_resource: None,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            fragment_data: [0u8; 64],
            cursor_sprite: alloc::vec::Vec::new(),
            cursor_sprite_w: 0,
            cursor_sprite_h: 0,
            cursor_resource_id: None,
            cursor_hot: (0, 0),
            cursor_owned: false,
            cursor_drawn: false,
            cursor_suspended: false,
            cursor_ox: 0,
            cursor_oy: 0,
            cursor_dw: 0,
            buildup_frame: 0,
            ctrl_batch: false,
            cursor_dh: 0,
            cursor_saveunder: alloc::vec::Vec::new(),
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            ctrlq: None,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            cursorq: None,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_backing_count: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_draw_ok: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_blur_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_parity_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_blur_first_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_vector_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_composite_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            cursor_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            cursor_tex_w: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            cursor_tex_h: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_atlas_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_layer_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_grad_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_shadow_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_scanout_active: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_present_parity_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_scanout_backing_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_display_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_wallpaper_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            wallpaper_from_vmo_uploaded: false,
            // RT-direct layer compositing on by default for the virgl path; the
            // field is the kill-switch if a regression shows up in the thumbnail.
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            rt_direct_layers: true,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            pending_rt_layers: [PendingRtLayer::default(); MAX_PENDING_RT_LAYERS],
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            pending_rt_count: 0,
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

        // Virgl capability detection.
        // When the `virgl` feature is compiled in, probe for GPU acceleration.
        // On QEMU with `-device virtio-gpu-pci,virgl=on`, the device reports
        // virgl capability in its config space. Without the feature or when
        // virgl is not detected, CPU fallback is used for blur operations.
        // `self.virgl_capable` is set during `probe_os()` feature negotiation:
        // true iff the device offered (and we acked) VIRTIO_GPU_F_VIRGL. Create
        // the 3D context; emit `virgl ready` ONLY on success, `cpu fallback`
        // otherwise — exactly one of the two markers, never both.
        #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
        {
            if self.virgl_capable && self.create_virgl_context().is_ok() {
                let _ = nexus_abi::debug_println(GPUD_VIRGL_READY);
                // Validate the SUBMIT_3D wire format against virglrenderer.
                if self.submit3d_selftest().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_SUBMIT3D_OK);
                }
                // Validate the draw-state path (resource → surface → fb → clear).
                if self.virgl_rt_clear_test().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_RT_CLEAR_OK);
                }
                // Validate TGSI shader creation (vertex + fragment).
                if self.virgl_shader_test().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_SHADER_OK);
                    // Full-pipeline draw proof with readback pixel verification.
                    // Solid-red FS over a blue clear: center pixel (BGRA bytes)
                    // tells us exactly how far the pipeline got.
                    match self.virgl_draw_selftest() {
                        Ok([0, 0, 255, 255]) => {
                            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_OK);
                            self.virgl_draw_ok = true;
                        }
                        Ok([255, 0, 0, 255]) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_NOOP);
                        }
                        Ok(_) => {
                            let _ = nexus_abi::debug_println(
                                crate::markers::GPUD_VIRGL_DRAW_MISMATCH,
                            );
                        }
                        Err(_) => {
                            let _ = nexus_abi::debug_println("gpud: virgl draw submit fail");
                        }
                    }
                    // M1a: GPU vector pipeline — per-pixel gradient proof.
                    match self.virgl_gradient_selftest() {
                        Ok(true) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_GRADIENT_OK);
                        }
                        Ok(false) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_GRADIENT_FLAT);
                        }
                        Err(_) => {
                            let _ =
                                nexus_abi::debug_println("gpud: virgl gradient submit fail");
                        }
                    }
                    // G2: GPU layer compositor primitive proof (textured layer +
                    // rounded mask + opacity composited into an RT, readback).
                    match self.virgl_composite_selftest() {
                        Ok(true) => {
                            let _ = nexus_abi::debug_println(
                                crate::markers::GPUD_LAYER_COMPOSITE_OK,
                            );
                        }
                        Ok(false) => {
                            let _ = nexus_abi::debug_println(
                                crate::markers::GPUD_LAYER_COMPOSITE_OFF,
                            );
                        }
                        Err(_) => {
                            let _ =
                                nexus_abi::debug_println("gpud: virgl composite submit fail");
                        }
                    }
                }
            } else {
                self.virgl_capable = false;
                let _ = nexus_abi::debug_println(GPUD_CPU_FALLBACK);
            }
        }
        #[cfg(not(all(feature = "virgl", feature = "os-lite", target_os = "none")))]
        {
            // Host fallback: no virgl possible, always CPU fallback.
            // Marker emitted via println! (host) or debug_println (OS).
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            let _ = nexus_abi::debug_println(GPUD_CPU_FALLBACK);
            #[cfg(not(all(feature = "os-lite", target_os = "none")))]
            let _ = GPUD_CPU_FALLBACK;
        }

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
                        let _ = nexus_abi::debug_println("gpud: set_scanout ok");
                        let _ = nexus_abi::debug_println("gpud: scanout ok");
                        let _ = nexus_abi::debug_println("gpud: scanout 1280x800 bgra8888");
                        self.gl_present_damage(Rect {
                            x: 0,
                            y: 0,
                            width,
                            height: DISPLAY_PLANE_HEIGHT,
                        })?;
                        let _ = nexus_abi::debug_println("gpud: transfer_to_host ok");
                        let _ = nexus_abi::debug_println("gpud: resource flush ok");
                        return Ok(());
                    }
                    Err(_) => {
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
                        let _ =
                            nexus_abi::debug_println(crate::markers::GPUD_GL_SCANOUT_FALLBACK);
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
        let _ = nexus_abi::debug_println("gpud: transfer_to_host ok");

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
        let _ = nexus_abi::debug_println("gpud: resource flush ok");

        self.resources.push(record);
        self.scanout_resource = Some(id);
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
    pub fn attach_bootstrap_text_scanout(
        &mut self,
        width: u32,
        height: u32,
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

    // ── Phase 6c: Command execution on OS (direct VMO write) ──────────────

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn execute_commands(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        // In the virgl build-up compositor the scanout is the GL render target
        // (`compositor_buildup_present` draws the full frame there each present), so
        // this CPU/VMO command stream is never presented. Several commands (glass
        // blur, drop shadow) also need a per-frame TRANSFER_TO_HOST_3D that
        // intermittently stalls QEMU's virgl renderer (used-ring never advances →
        // the present-damage chain wedges before G4). Skip the whole VMO stream; the
        // GL-RT build-up owns the output. (mmio scans out the VMO, so it still runs.)
        #[cfg(feature = "virgl")]
        if crate::gl_scanout::COMPOSITOR_BUILDUP {
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
                                    fb, fb_len, fb_w,
                                    rect.x, rect.y.saturating_add(display_y_offset),
                                    rect.width, rect.height, *radius, *saturation_percent,
                                )?;
                            } else {
                                blur_backdrop_vmo(
                                    fb, fb_len, fb_w,
                                    rect.x, rect.y.saturating_add(display_y_offset),
                                    rect.width, rect.height, *radius, *saturation_percent,
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
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_SDF_GRAD_OK);
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
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_DROPSHADOW_OK);
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
                } => {
                    // `opacity` is honoured by the GPU path; the CPU fallback
                    // relies on the content's own alpha (translucent panel bg).
                    #[cfg(not(feature = "virgl"))]
                    let _ = opacity;
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
                            let _ = nexus_abi::debug_println(
                                crate::markers::GPUD_LAYER_COMPOSITE_LIVE,
                            );
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
                                fb, fb_len, fb_w, *dst_x, dst_y_abs, *width, *height,
                                *backdrop_blur, 0,
                            );
                        }
                        let _ = blit_blend_vmo(
                            fb, fb_len, fb_w, *src_x, *src_row_abs, *dst_x, dst_y_abs,
                            *width, *height,
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
        self.pending_rt_count = 0;
        for i in 0..n {
            let l = self.pending_rt_layers[i];
            let ok = self
                .composite_layer_rt(
                    l.src_row_abs,
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
                )
                .is_ok();
            if ok && !self.virgl_layer_marker_done {
                self.virgl_layer_marker_done = true;
                let _ = nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_LIVE);
            }
        }
    }

    /// Store the software cursor sprite (premultiplied BGRA) for BlendCursor.
    /// No hardware cursor resource, no UPDATE_CURSOR — avoids the QEMU virtio-gpu
    /// quirk. The sprite is composited into the display plane each frame.
    pub fn store_cursor_sprite(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
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

    // ── Save-under software cursor (composited into the scanout) ──────────────
    //
    // The virtio-gpu cursor-queue overlay is not composited into the captured /
    // displayed scanout on this MMIO+GTK setup, so the pointer was invisible.
    // Instead gpud composites the cursor directly into the display plane with a
    // classic save-under: save the pixels the sprite covers, blend the sprite,
    // and restore on move. Driven by the same 9-byte OP_MOVE_CURSOR — windowd's
    // hot path is unchanged (no scene rebuild per move). Presents are wrapped
    // with `cursor_before_present` / `cursor_after_present` so scene blits never
    // fight the cursor.

    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn scanout_fb(&self) -> Option<(*mut u8, usize, usize, u32)> {
        let scanout = self.scanout_resource?;
        let record = self.find_resource(scanout)?;
        if record.backing_va == 0 {
            return None;
        }
        Some((record.backing_va as *mut u8, record.backing_len, record.width as usize, DISPLAY_PLANE_ROW))
    }

    /// Emit an ASCII thumbnail of what we actually render — the windowd-composited
    /// source plane, plus (on the virgl path) the GPU scanout readback — to the
    /// serial console. Headless pipeline-bisection instrument (no host display);
    /// see the `debug_thumbnail` module. Driven by the service present loop.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn emit_debug_thumbnail(&mut self) {
        #[cfg(feature = "virgl")]
        if self.gl_scanout_active {
            self.gl_emit_thumbnails();
            return;
        }
        if let Some((fb, fb_len, fb_w, display_row)) = self.scanout_fb() {
            if fb_w == 0 {
                return;
            }
            let total_rows = fb_len / 4 / fb_w;
            // Display plane height: screen rows only — the atlas lives above it.
            let h = total_rows.saturating_sub(display_row as usize).min(800);
            if h == 0 {
                return;
            }
            unsafe {
                crate::debug_thumbnail::emit_ascii_thumbnail(
                    "cpu-src",
                    fb as *const u8,
                    fb_len,
                    fb_w,
                    0,
                    display_row as usize,
                    fb_w,
                    h,
                );
            }
        }
    }

    /// Mark gpud as the cursor compositor and store the sprite/hotspot. The
    /// sprite stays the BlendCursor source; the first move paints it.
    pub fn cursor_take_ownership(&mut self, hot_x: u32, hot_y: u32) {
        self.cursor_owned = true;
        self.cursor_hot = (hot_x, hot_y);
        // Size the save-under for whichever sprite we'll paint: the uploaded SVG
        // sprite OR the procedural arrow fallback (so the fallback can erase its
        // own region on move without trailing).
        let sprite_px = self.cursor_sprite_w as usize * self.cursor_sprite_h as usize;
        let fallback_px = CURSOR_FALLBACK_W as usize * CURSOR_FALLBACK_H as usize;
        let cap = (sprite_px.max(fallback_px) * 4).max(4);
        if self.cursor_saveunder.len() < cap {
            self.cursor_saveunder.resize(cap, 0);
        }
    }

    /// Remove the cursor from the display plane (restore saved pixels). In-place,
    /// no flush — the caller flushes (or a present covers the region).
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn cursor_unpaint(&mut self) {
        if !self.cursor_drawn {
            return;
        }
        let (ox, oy, w, h) = (self.cursor_ox, self.cursor_oy, self.cursor_dw, self.cursor_dh);
        if let Some((fb, fb_len, fb_w, dyoff)) = self.scanout_fb() {
            for py in 0..h as usize {
                let sy = dyoff as usize + oy as usize + py;
                let dst = (sy * fb_w + ox as usize) * 4;
                let src = py * w as usize * 4;
                let n = w as usize * 4;
                if dst + n <= fb_len && src + n <= self.cursor_saveunder.len() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            self.cursor_saveunder.as_ptr().add(src),
                            fb.add(dst),
                            n,
                        );
                    }
                }
            }
        }
        self.cursor_drawn = false;
    }

    /// Save the scene pixels at (ox,oy) into the save-under buffer, then blend the
    /// sprite over them. In-place, no flush. Sets the drawn rect.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn cursor_paint(&mut self, ox: i32, oy: i32) {
        if !self.cursor_owned {
            return;
        }
        let Some((fb, fb_len, fb_w, dyoff)) = self.scanout_fb() else {
            return;
        };
        let fb_h = (fb_len / (fb_w * 4)) as i32 - dyoff as i32;
        let ox = ox.clamp(0, (fb_w as i32 - 1).max(0));
        let oy = oy.clamp(0, (fb_h - 1).max(0));
        // Use the uploaded SVG sprite if present; otherwise paint the procedural
        // arrow fallback (blend_cursor_vmo draws CURSOR_ARROW when the sprite is
        // empty). This keeps a visible pointer before/without the SVG cursor.
        let (sprite_w, sprite_h) = if self.cursor_sprite.is_empty() {
            (CURSOR_FALLBACK_W, CURSOR_FALLBACK_H)
        } else {
            (self.cursor_sprite_w, self.cursor_sprite_h)
        };
        let w = (sprite_w as i32).min(fb_w as i32 - ox).max(0) as u32;
        let h = (sprite_h as i32).min(fb_h - oy).max(0) as u32;
        if w == 0 || h == 0 {
            return;
        }
        // Save-under: copy current scene pixels into the buffer.
        for py in 0..h as usize {
            let sy = dyoff as usize + oy as usize + py;
            let src = (sy * fb_w + ox as usize) * 4;
            let dst = py * w as usize * 4;
            let n = w as usize * 4;
            if src + n <= fb_len && dst + n <= self.cursor_saveunder.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(fb.add(src), self.cursor_saveunder.as_mut_ptr().add(dst), n);
                }
            }
        }
        // Blend the sprite over the display plane (premultiplied BGRA).
        let _ = blend_cursor_vmo(
            fb,
            fb_len,
            fb_w,
            ox as u32,
            dyoff + oy as u32,
            w,
            h,
            &self.cursor_sprite,
            self.cursor_sprite_w,
            self.cursor_sprite_h,
        );
        self.cursor_ox = ox;
        self.cursor_oy = oy;
        self.cursor_dw = w;
        self.cursor_dh = h;
        self.cursor_drawn = true;
    }

    /// Move the composited cursor to pointer position (px, py). Restores the old
    /// region, paints the new one, and flushes both to the display.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn cursor_move(&mut self, px: i32, py: i32) -> Result<(), GfxError> {
        if !self.cursor_owned {
            return Ok(());
        }
        let old = if self.cursor_drawn {
            Some((self.cursor_ox, self.cursor_oy, self.cursor_dw, self.cursor_dh))
        } else {
            None
        };
        self.cursor_unpaint();
        let ox = px - self.cursor_hot.0 as i32;
        let oy = py - self.cursor_hot.1 as i32;
        self.cursor_paint(ox, oy);
        // Flush the union of old and new cursor rects (screen-relative).
        let (nx, ny, nw, nh) = (self.cursor_ox, self.cursor_oy, self.cursor_dw, self.cursor_dh);
        match old {
            Some((oxo, oyo, owo, oho)) => {
                let x0 = oxo.min(nx).max(0);
                let y0 = oyo.min(ny).max(0);
                let x1 = (oxo + owo as i32).max(nx + nw as i32);
                let y1 = (oyo + oho as i32).max(ny + nh as i32);
                let _ = self.present_scanout_damage(Rect {
                    x: x0 as u32,
                    y: y0 as u32,
                    width: (x1 - x0).max(0) as u32,
                    height: (y1 - y0).max(0) as u32,
                });
            }
            None => {
                if nw > 0 && nh > 0 {
                    let _ = self.present_scanout_damage(Rect {
                        x: nx as u32,
                        y: ny as u32,
                        width: nw,
                        height: nh,
                    });
                }
            }
        }
        Ok(())
    }

    /// Before a windowd present: lift the cursor off the display so scene blits
    /// land on a cursor-free plane. Re-applied by `cursor_after_present`.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn cursor_before_present(&mut self) {
        if self.cursor_owned && self.cursor_drawn {
            self.cursor_unpaint();
            self.cursor_suspended = true;
        }
    }

    /// After a windowd present: re-save the (now current) scene under the cursor,
    /// blend the sprite back on top, and flush just the cursor rect.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn cursor_after_present(&mut self) {
        if !self.cursor_owned || !self.cursor_suspended {
            return;
        }
        self.cursor_suspended = false;
        self.cursor_paint(self.cursor_ox, self.cursor_oy);
        if self.cursor_dw > 0 && self.cursor_dh > 0 {
            let _ = self.present_scanout_damage(Rect {
                x: self.cursor_ox as u32,
                y: self.cursor_oy as u32,
                width: self.cursor_dw,
                height: self.cursor_dh,
            });
        }
    }

    /// Upload the cursor bitmap as a hardware cursor resource and arm the
    /// cursor-queue overlay (UPDATE_CURSOR).
    ///
    /// The virtio-gpu spec requires cursor resources to be exactly 64×64; QEMU
    /// silently ignores cursor data of any other size (the cursor shows as
    /// invisible — the historical "UPDATE_CURSOR quirk" was this, combined with
    /// transferring the resource BEFORE the bitmap was copied into its backing,
    /// so the host always sampled zeros). The sprite is copied into the top-left
    /// of a transparent 64×64 resource, transferred, and only then armed.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn upload_cursor(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        hot_x: u32,
        hot_y: u32,
    ) -> Result<(), GfxError> {
        const CURSOR_DIM: u32 = 64;
        if width == 0 || height == 0 || width > CURSOR_DIM || height > CURSOR_DIM {
            return Err(GfxError::InvalidArgument);
        }
        if bgra.len() < (width * height * 4) as usize {
            return Err(GfxError::InvalidArgument);
        }
        if self.cursorq.is_none() {
            return Err(GfxError::DeviceNotFound);
        }
        // Reuse the existing cursor resource on re-upload instead of leaking one.
        let rid = match self.cursor_resource_id {
            Some(rid) => rid,
            None => self.create_resource(CURSOR_DIM, CURSOR_DIM, PixelFormat::Bgra8888)?,
        };
        let record = self.find_resource(rid).ok_or(GfxError::InvalidArgument)?;
        // 1. Copy the sprite into the top-left of the 64×64 backing. The backing
        //    was zeroed at create, so the remainder stays fully transparent.
        let stride = (CURSOR_DIM * 4) as usize;
        let src_stride = (width * 4) as usize;
        unsafe {
            let dst = core::slice::from_raw_parts_mut(
                record.backing_va as *mut u8,
                stride * CURSOR_DIM as usize,
            );
            for row in 0..height as usize {
                let s = row * src_stride;
                let d = row * stride;
                dst[d..d + src_stride].copy_from_slice(&bgra[s..s + src_stride]);
            }
        }
        // 2. Transfer guest backing → host resource (must follow the copy).
        let full = Rect { x: 0, y: 0, width: CURSOR_DIM, height: CURSOR_DIM };
        self.transfer_to_host_os(record, full)?;
        // 3. Arm the hardware cursor overlay on the cursor queue.
        let cmd = protocol::VirtioGpuUpdateCursor {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_UPDATE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x: 0, y: 0, _padding: 0 },
            resource_id: rid.0,
            hot_x,
            hot_y,
            _padding: 0,
        };
        self.cursor_submit_struct(&cmd)?;
        self.cursor_resource_id = Some(rid);
        self.cursor_hot = (hot_x, hot_y);
        Ok(())
    }

    /// Move the hardware cursor overlay. Requires a prior `upload_cursor`.
    /// Host repositions the overlay — no scanout re-render, no guest composite.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn move_hw_cursor(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let rid = self.cursor_resource_id.ok_or(GfxError::DeviceNotFound)?;
        let (hot_x, hot_y) = self.cursor_hot;
        let cmd = protocol::VirtioGpuCursorPos {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x, y, _padding: 0 },
            resource_id: rid.0,
            hot_x,
            hot_y,
            _padding: 0,
        };
        self.cursor_submit_struct(&cmd)
    }

    /// True once the hardware cursor overlay is armed.
    pub fn hw_cursor_active(&self) -> bool {
        self.cursor_resource_id.is_some()
    }

    /// Records the current pointer position for the GL-scanout fallback cursor
    /// (the Stage-4 build-up draws the procedural arrow at `cursor_ox/oy` each
    /// present). Transfer-free, so it is safe on the virgl GL scanout.
    pub fn set_pointer_pos(&mut self, x: i32, y: i32) {
        self.cursor_ox = x;
        self.cursor_oy = y;
    }

    /// Arms the hardware-cursor overlay with the procedural [`CURSOR_ARROW`] so a
    /// pointer is visible WITHOUT an uploaded SVG sprite — a testing fallback that
    /// is independent of windowd's BlendCursor, the scanout, and the build-up
    /// (the overlay is a QEMU-composited plane). Tip at (0,0) = hot spot.
    ///
    /// NOTE: `upload_cursor` issues a `transfer_to_host` for the cursor resource,
    /// which blanks the virgl GL-scanout present — DO NOT call this on the virgl
    /// path; it is kept for the CPU/mmio scanout where the transfer is harmless.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    #[allow(dead_code)]
    pub fn install_fallback_hw_cursor(&mut self) -> Result<(), GfxError> {
        let w = CURSOR_FALLBACK_W;
        let h = CURSOR_FALLBACK_H;
        let mut sprite = alloc::vec::Vec::new();
        sprite.resize((w * h * 4) as usize, 0u8);
        for py in 0..h {
            for px in 0..w {
                let c = cursor_pixel_bgra(px, py, w, h);
                let i = ((py * w + px) * 4) as usize;
                sprite[i..i + 4].copy_from_slice(&c);
            }
        }
        self.upload_cursor(&sprite, w, h, 0, 0)?;
        let _ = nexus_abi::debug_println(crate::markers::GPUD_CURSOR_ON);
        Ok(())
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
fn put_bootstrap_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
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
/// Maximum in-flight commands on the control queue = command slots in the ring.
/// A present batches ~8 SUBMIT_3D draws + a flush; 16 gives headroom so a whole
/// present is enqueued without an intra-batch drain. The cursor queue passes
/// `slots = 1` (single-slot, unchanged behaviour).
#[cfg(all(feature = "os-lite", target_os = "none"))]
const RING_SLOTS: usize = 16;
/// virtqueue descriptor-table length. Each command slot uses a 2-descriptor chain
/// (cmd → resp), so the table holds `RING_SLOTS * 2` descriptors. `avail.ring` /
/// `used.ring` are sized to this; both queues share the length (the cursor queue
/// uses only the first pair).
#[cfg(all(feature = "os-lite", target_os = "none"))]
const QUEUE_LEN: usize = RING_SLOTS * 2;
/// Hard ceiling on any single GPU command wait. The used-ring advance normally
/// completes far sooner (spin or IRQ); this only bounds a lost/late IRQ so a
/// present can never hang — it degrades to the legacy timeout (matches the old
/// 500 ms spin deadline).
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_WAIT_DEADLINE_NS: u64 = 500_000_000;
// Completion is PURE REACTIVE: `wait_slot`/`alloc_free_slot` `harvest` the used-ring
// once at the top of the loop (an already-finished command returns immediately, no
// syscall), and otherwise BLOCK on the GPU ring-buffer IRQ via `block_on_irq` — never
// a busy yield-spin (a spin IS a poll, which we explicitly do not want). The pipelined
// present blocks on nothing at all; the next frame harvests. `GPU_WAIT_DEADLINE_NS` is
// only the safety net bounding a lost/late IRQ so a present can never hang.
/// Latches once the GPU ring-buffer IRQ first wakes a completion wait, so the
/// headless run can confirm the interrupt path is actually live (vs. silently
/// degrading to the spin fallback). One marker, not per-frame — no UART storm.
#[cfg(all(feature = "os-lite", target_os = "none"))]
static GPU_IRQ_WAKE_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Latches once `harvest` first reclaims a completed slot — proof (once) that the
/// pipelined completion path flows (frame N's commands complete and are observed
/// asynchronously, without the present ever blocking on them). One marker, not
/// per-frame.
#[cfg(all(feature = "os-lite", target_os = "none"))]
static PIPELINE_HARVEST_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Debug: trace the first GPU blur's step progression (xfer-in → submit → xfer-out)
/// to pinpoint where the intermittent virgl-blur G3 stall wedges. Latches after the
/// first blur completes so it never spams the per-frame spin presents.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_QUEUE_VA: usize = 0x2030_0000;
// Control queue command-buffer POOL: `RING_SLOTS` contiguous 4 KiB pages, one
// per in-flight command slot, starting here. The multi-entry ring batches a
// whole present's commands (one buffer each) then completes once — so a textured
// draw whose completion QEMU defers no longer blocks the next command. Pool ends
// at GPU_CMD_VA + RING_SLOTS*4096 = 0x2032_0000 (16 slots), just below the resp page.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_CMD_VA: usize = 0x2031_0000;
// Response POOL: one 4 KiB page holding `RING_SLOTS` × 256 B response sub-slots
// (a virtio-gpu response header is 24 B). Slot i's resp is at GPU_RESP_VA + i*256.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESP_VA: usize = 0x2032_0000;
// Cursor virtqueue (queue index 1) — separate VA region so it does not collide
// with the control queue's desc/cmd-pool/resp pages. The hardware cursor overlay is
// the GPU "hot path" for the pointer: MOVE_CURSOR repositions it host-side
// without re-rendering the scene. The cursor queue is single-slot (no batching).
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_CURSOR_QUEUE_VA: usize = 0x2034_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_CURSOR_CMD_VA: usize = 0x2035_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_CURSOR_RESP_VA: usize = 0x2035_1000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESOURCE_BASE_VA: usize = 0x2040_0000;
// 32 MB per resource VA slot. The external framebuffer is now 1280×6400×4 ≈ 31.3 MB
// (4 display planes + surface atlas), so the 16 MB stride would overflow into the
// next slot. 32 MB stride × ≤11 slots stays below GPU_VIRGL_BACKING_BASE_VA.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESOURCE_STRIDE: usize = 0x0200_0000;
/// Fixed display-plane location within the framebuffer resource. The 4-plane
/// layout is: wallpaper(0) / retained(800) / DISPLAY(1600) / blur-cache(2400),
/// with the surface atlas at 3200+. This is a FIXED row — NOT `height/2` — since
/// the resource grew to 6400 rows to host the atlas, but the display plane stays
/// at 1600. Must match windowd's `DISPLAY_ROW_OFFSET`.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const DISPLAY_PLANE_ROW: u32 = 1600;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const DISPLAY_PLANE_HEIGHT: u32 = 800;
/// VA region for virgl 3D resource backings (readback targets) — separate from
/// the 2D resource region so the two allocators never collide.
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
const GPU_VIRGL_BACKING_BASE_VA: usize = 0x3800_0000;
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
const GPU_VIRGL_BACKING_STRIDE: usize = 0x0100_0000;

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

/// A virtio control/cursor virtqueue with a multi-entry command ring.
///
/// Holds raw pointers into device-shared memory (descriptor table, avail/used
/// rings, command/response pools), so it is intentionally **not `Send`/`Sync`**:
/// the buffers live in gpud's address space and the ring is driven by gpud's
/// single cooperative thread (enqueue → notify → drain is one logical sequence).
/// There is no `unsafe impl Send` — the queue must never cross threads, and the
/// `!Send` default enforces that at compile time.
#[cfg(all(feature = "os-lite", target_os = "none"))]
struct CtrlQueue {
    queue_index: u32,
    _queue_vmo: u32,
    _cmd_vmo: u32,
    _resp_vmo: u32,
    desc: *mut VqDesc,
    avail: *mut VqAvail<QUEUE_LEN>,
    used: *mut VqUsed<QUEUE_LEN>,
    /// Command-buffer pool base (VA/PA). Slot `i`'s command buffer is at
    /// `cmd_va + i*4096` / `cmd_pa + i*4096` (the pool is physically contiguous).
    cmd_va: usize,
    cmd_pa: u64,
    /// Response-buffer pool base (VA/PA). Slot `i`'s response header is at
    /// `resp_va + i*RESP_SLOT_SIZE` / `resp_pa + i*RESP_SLOT_SIZE`.
    resp_va: usize,
    resp_pa: u64,
    /// Slot lifecycle — in-flight set, round-robin allocation, backpressure — provided by
    /// the shared DriverKit submit ring (RFC-0033 `nexus_driverkit::SubmitRing`, the lib that
    /// generalises this very ring). A slot is reserved on `try_alloc` and freed only when its
    /// used-ring entry is harvested (`complete`), so it is never reused while QEMU may still
    /// be reading its buffers — the pipelining safety invariant. The virtio specifics
    /// (descriptor pairs, cmd/resp pools, the `last_used` cursor) stay here in gpud.
    ring: nexus_driverkit::SubmitRing,
    /// Device `used.idx` already harvested — the consumer cursor into `used.ring`.
    last_used: u16,
    /// Device MMIO base — needed to drain/ACK InterruptStatus (0x60/0x64) on the
    /// GPU ring-buffer IRQ path so the level-triggered line de-asserts.
    mmio_base: usize,
    /// PLIC source bound to this queue's completion IRQ (0 = not bound → the
    /// legacy spin+yield wait is used, never a hang).
    irq_num: u32,
    /// Endpoint cap slot the kernel routes the GPU IRQ to (0 = not bound). When
    /// set, the wait path blocks here instead of busy-polling.
    irq_ep: u32,
}

/// Bytes reserved per response sub-slot in the response pool (a virtio-gpu
/// response header is 24 B; 256 B keeps slots cache-line-friendly and lets the
/// whole `RING_SLOTS` pool fit one 4 KiB page).
#[cfg(all(feature = "os-lite", target_os = "none"))]
const RESP_SLOT_SIZE: usize = 256;

/// A command slot in the multi-entry ring (`0..slots`). A newtype so it can't be
/// confused with a raw descriptor index (each slot owns the descriptor *pair*
/// `2*slot` / `2*slot+1`) or with an in-flight *count* — the three are different
/// quantities that all happen to be small integers, and mixing them in the
/// pointer/descriptor arithmetic would be a silent memory-safety bug.
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct RingSlot(u16);

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl RingSlot {
    /// Head (command) descriptor index for this slot's 2-descriptor chain.
    #[inline]
    fn head_desc(self) -> usize {
        2 * self.0 as usize
    }
    /// Response descriptor index (`head + 1`).
    #[inline]
    fn resp_desc(self) -> usize {
        2 * self.0 as usize + 1
    }
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

        // Feature negotiation. The non-virgl build acknowledges no features
        // (the long-proven 2D path); the virgl build reads the device feature
        // bits and acks VIRGL + CONTEXT_INIT + VERSION_1 when the device (a
        // `virtio-gpu-gl` model) offers them, enabling the 3D command path.
        #[cfg(feature = "virgl")]
        {
            self.negotiate_features_virgl();
        }
        #[cfg(not(feature = "virgl"))]
        {
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 0);
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, 0);
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 1);
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, 0);
        }

        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 8);
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS) & 8 == 0 {
            // FEATURES_OK refused: the device rejected our negotiated set.
            #[cfg(feature = "virgl")]
            {
                self.virgl_capable = false;
            }
            return Err(GpuDriverError::CommandRejected);
        }
        // Control queue: multi-slot ring (batches a whole present, completes once).
        let ctrlq = CtrlQueue::new(
            self.mmio_base,
            CTRL_QUEUE_INDEX,
            GPU_QUEUE_VA,
            GPU_CMD_VA,
            GPU_RESP_VA,
            RING_SLOTS,
        )?;
        self.ctrlq = Some(ctrlq);
        // Cursor virtqueue (index 1) — hardware-cursor overlay path. Best-effort:
        // if it can't be set up, cursor falls back and 2D still works. Single-slot
        // (cursor commands are submitted one at a time, no batching).
        if let Ok(cursorq) = CtrlQueue::new(
            self.mmio_base,
            CURSOR_QUEUE_INDEX,
            GPU_CURSOR_QUEUE_VA,
            GPU_CURSOR_CMD_VA,
            GPU_CURSOR_RESP_VA,
            1,
        ) {
            self.cursorq = Some(cursorq);
        }
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 4);
        Ok(())
    }

    /// Bind this GPU's virtio-mmio completion IRQ (PLIC source) to `irq_ep` so the
    /// command-completion wait can BLOCK on the interrupt instead of busy-polling
    /// the used-ring. Wires both the control and cursor queues — they share the one
    /// device IRQ. Best-effort: on a denied/failed bind the queues keep `irq_ep = 0`
    /// and the legacy spin+yield wait stays in force, so a wrong IRQ never hangs a
    /// present, it only forgoes the reactive wake. Returns true when bound.
    pub(crate) fn bind_gpu_irq(&mut self, irq_num: u32, irq_ep: u32) -> bool {
        if nexus_abi::irq_bind(irq_num, irq_ep).is_err() {
            return false;
        }
        if let Some(q) = self.ctrlq.as_mut() {
            q.set_gpu_irq(irq_num, irq_ep);
        }
        if let Some(q) = self.cursorq.as_mut() {
            q.set_gpu_irq(irq_num, irq_ep);
        }
        true
    }

    /// Create a virgl rendering context for GPU shader dispatch.
    /// Must be called after probe_os() (ctrlq is set up).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn create_virgl_context(&mut self) -> Result<(), GpuDriverError> {
        use crate::protocol::{
            VirtioGpuCtxCreate, VirtioGpuCtrlHdr, VIRTIO_GPU_CAPSET_VIRGL2,
            VIRTIO_GPU_CMD_CTX_CREATE,
        };
        let mut name = [0u8; 64];
        let label = b"gpud-virgl-ctx";
        let len = label.len().min(64);
        name[..len].copy_from_slice(&label[..len]);

        // The guest chooses the context id; subsequent 3D commands carry it in
        // their header's ctx_id field. We use a single context (id 1).
        const VIRGL_CTX_ID: u32 = 1;
        let cmd = VirtioGpuCtxCreate {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_CTX_CREATE,
                flags: 0,
                fence_id: 0,
                ctx_id: VIRGL_CTX_ID,
                _padding: 0,
            },
            nlen: len as u32,
            context_init: VIRTIO_GPU_CAPSET_VIRGL2,
            debug_name: name,
        };

        // `ctrl_submit_struct` writes the command, notifies the device, and
        // validates the response header (RESP_OK_NODATA → Ok, else Err).
        self.ctrl_submit_struct(&cmd).map_err(|_| GpuDriverError::CommandRejected)?;
        self.virgl_ctx_id = VIRGL_CTX_ID;
        Ok(())
    }

    /// Submit a `VirtioGpuSubmit3d` header followed by a hand-encoded virgl
    /// command stream on the control queue (one descriptor chain). The response
    /// is validated by `wait_complete` (RESP_OK_NODATA → Ok).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_submit_header_tail<T>(&mut self, hdr: &T, tail: &[u8]) -> Result<(), GfxError> {
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts((hdr as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        let mmio = self.mmio_base;
        let batch = self.ctrl_batch;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        if batch {
            queue.enqueue_pair(mmio, hdr_bytes, tail).map(|_| ())
        } else {
            queue.submit_two(mmio, hdr_bytes, tail)
        }
    }

    /// Begin a control-queue command batch: subsequent `ctrl_submit_*` calls
    /// enqueue without waiting. Pair with [`ctrl_batch_end`].
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_batch_begin(&mut self) {
        self.ctrl_batch = true;
    }

    /// End the batch — **pipelined: does NOT block**. Clears the batch flag and
    /// opportunistically `harvest`s completed slots (the next present would
    /// otherwise reclaim them). The present's own commands stay in flight and are
    /// reaped by the *next* frame's enqueue, so a textured draw whose completion
    /// QEMU defers never blocks this present. Emits the G3c "pipeline flowing"
    /// marker once the first prior-frame batch is reclaimed.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_batch_end(&mut self) -> Result<(), GfxError> {
        self.ctrl_batch = false;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        let before = queue.ring.in_flight();
        queue.harvest();
        if before != 0
            && queue.ring.in_flight() != before
            && !PIPELINE_HARVEST_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed)
        {
            let _ = nexus_abi::debug_println(crate::markers::GPUD_CHAIN_BATCH_OK);
        }
        Ok(())
    }

    /// End-to-end validation of the `SUBMIT_3D` path: emit a minimal NOP stream
    /// and confirm virglrenderer accepts it. This proves the 3D wire format and
    /// context routing work before the full blur pipeline is built; it does not
    /// touch the blur path (blur stays on the CPU separable gaussian until the
    /// GPU shader lands).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn submit3d_selftest(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuCtrlHdr, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        let mut stream = crate::virgl::Submit3d::new();
        stream.emit_nop();
        let bytes = stream.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_SUBMIT_3D,
                flags: 0,
                fence_id: 0,
                ctx_id: self.virgl_ctx_id,
                _padding: 0,
            },
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }

    /// A virtio-gpu control header carrying our virgl context id (3D commands
    /// are context-scoped, unlike the 2D path which uses ctx_id 0).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_hdr(&self, type_: u32) -> protocol::VirtioGpuCtrlHdr {
        protocol::VirtioGpuCtrlHdr {
            type_,
            flags: 0,
            fence_id: 0,
            ctx_id: self.virgl_ctx_id,
            _padding: 0,
        }
    }

    /// Create a 3D render-target texture and attach it to the virgl context.
    /// Bound as both RENDER_TARGET (draw destination) and SAMPLER_VIEW (so the
    /// blur shader can later read a source texture of the same shape).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_create_rt(&mut self, res_id: u32, w: u32, h: u32) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d,
            VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
        };
        use crate::virgl::{
            PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_FORMAT_B8G8R8A8_UNORM,
            PIPE_TEXTURE_2D,
        };
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: res_id,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: w,
            height: h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: res_id,
            _padding: 0,
        };
        self.ctrl_submit_struct(&attach)?;
        Ok(())
    }

    /// Increment A: create a render target, wrap it as a surface, bind it as
    /// the framebuffer, and clear it. Validates the draw-state pipeline
    /// (resource → surface → framebuffer → clear) end-to-end against
    /// virglrenderer before shaders/draw are introduced. Does not touch the
    /// blur path.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn virgl_rt_clear_test(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuCtrlHdr, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{Submit3d, PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM};
        const RT_RES_ID: u32 = 0xF0;
        const SURFACE_HANDLE: u32 = 1;

        self.virgl_create_rt(RT_RES_ID, 64, 64)?;

        let mut s = Submit3d::new();
        s.emit_create_surface(SURFACE_HANDLE, RT_RES_ID, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[SURFACE_HANDLE]);
        s.emit_clear(PIPE_CLEAR_COLOR0, [1.0, 0.0, 0.0, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_SUBMIT_3D,
                flags: 0,
                fence_id: 0,
                ctx_id: self.virgl_ctx_id,
                _padding: 0,
            },
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }

    /// Allocate a guest VMO, map it into the virgl backing VA region, and
    /// attach it as the backing store of `res_id` (then attach the resource to
    /// the virgl context). Returns the backing VA for CPU access after
    /// TRANSFER_FROM_HOST_3D.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_attach_backing(&mut self, res_id: u32, byte_len: usize) -> Result<usize, GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuMemEntry, VirtioGpuResourceAttachBacking,
            VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
        };
        let slot = self.virgl_backing_count;
        if slot >= 8 {
            return Err(GfxError::ResourceExhausted);
        }
        let backing_va = GPU_VIRGL_BACKING_BASE_VA + slot * GPU_VIRGL_BACKING_STRIDE;
        let backing_len = align_page(byte_len);
        let vmo = nexus_abi::vmo_create(backing_len).map_err(|_| GfxError::ResourceExhausted)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len).step_by(4096) {
            nexus_abi::vmo_map_page(vmo, backing_va + offset, offset, flags)
                .map_err(|_| GfxError::MmioFault)?;
        }
        unsafe { core::ptr::write_bytes(backing_va as *mut u8, 0, backing_len) };
        let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(vmo, &mut info).map_err(|_| GfxError::MmioFault)?;
        self.virgl_backing_count = slot + 1;

        let attach = VirtioGpuResourceAttachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: res_id,
            nr_entries: 1,
        };
        let entry = VirtioGpuMemEntry { addr: info.base, length: byte_len as u32, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: res_id,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        Ok(backing_va)
    }

    /// Issue TRANSFER_FROM_HOST_3D for a full-width box of `res_id` and wait
    /// for completion — host GPU contents land in the resource's guest backing.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_transfer_from_host(
        &mut self,
        res_id: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        stride: u32,
    ) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuBox, VirtioGpuTransferHost3d, VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D,
        };
        let cmd = VirtioGpuTransferHost3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D),
            box_: VirtioGpuBox { x, y, z: 0, w, h, d: 1 },
            offset: (y as u64) * (stride as u64) + (x as u64) * 4,
            resource_id: res_id,
            level: 0,
            stride,
            layer_stride: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    /// Full-pipeline GPU draw proof: state objects + fullscreen-triangle vertex
    /// buffer + the (already created) passthrough VS / solid-red FS + DRAW_VBO
    /// into a 64×64 render target, then TRANSFER_FROM_HOST_3D readback and a
    /// CPU pixel check. Returns the first pixel's BGRA bytes.
    ///
    /// This verifies the entire 3D path end-to-end on-device — no display
    /// needed: if the pixels read back red, the GPU really rasterized our draw.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn virgl_draw_selftest(&mut self) -> Result<[u8; 4], GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_BIND_VERTEX_BUFFER, PIPE_BUFFER, PIPE_CLEAR_COLOR0,
            PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R32G32B32A32_FLOAT, PIPE_FORMAT_R8_UNORM,
            PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX, VIRGL_OBJECT_BLEND,
            VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJECT_VERTEX_ELEMENTS,
        };
        const RT_RES: u32 = 0xF4;
        const VBO_RES: u32 = 0xF5;
        const RT_W: u32 = 64;
        const RT_H: u32 = 64;
        // Object handles (context-scoped namespace).
        const H_BLEND: u32 = 0x20;
        const H_DSA: u32 = 0x21;
        const H_RAST: u32 = 0x22;
        const H_VE: u32 = 0x23;
        const H_SURF: u32 = 0x24;
        const H_VS: u32 = 10; // created by virgl_shader_test
        const H_FS: u32 = 11;

        // Render target with guest backing for readback.
        self.virgl_create_rt(RT_RES, RT_W, RT_H)?;
        let backing_va = self.virgl_attach_backing(RT_RES, (RT_W * RT_H * 4) as usize)?;

        // Vertex buffer resource (host-side storage; filled via INLINE_WRITE).
        {
            use crate::protocol::{VirtioGpuResourceCreate3d, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D};
            let create = VirtioGpuResourceCreate3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
                resource_id: VBO_RES,
                target: PIPE_BUFFER,
                format: PIPE_FORMAT_R8_UNORM,
                bind: PIPE_BIND_VERTEX_BUFFER,
                width: 48, // 3 vertices × vec4 f32
                height: 1,
                depth: 1,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: 0,
                _padding: 0,
            };
            self.ctrl_submit_struct(&create)?;
            use crate::protocol::{VirtioGpuCtxAttachResource, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE};
            let attach = VirtioGpuCtxAttachResource {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                resource_id: VBO_RES,
                _padding: 0,
            };
            self.ctrl_submit_struct(&attach)?;
        }

        // Fullscreen triangle (clip space), vec4 positions.
        let verts: [f32; 12] = [-1.0, -1.0, 0.0, 1.0, 3.0, -1.0, 0.0, 1.0, -1.0, 3.0, 0.0, 1.0];
        let mut vbytes = [0u8; 48];
        for (i, v) in verts.iter().enumerate() {
            vbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(VBO_RES, &vbytes);
        s.emit_create_blend_default(H_BLEND);
        s.emit_bind_object(VIRGL_OBJECT_BLEND, H_BLEND);
        s.emit_create_dsa_default(H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_create_rasterizer_default(H_RAST);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RAST);
        s.emit_create_vertex_elements(H_VE, &[(0, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT)]);
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VE);
        s.emit_set_vertex_buffers(&[(16, 0, VBO_RES)]);
        s.emit_create_surface(H_SURF, RT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[H_SURF]);
        s.emit_set_viewport(RT_W as f32, RT_H as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 1.0, 1.0], 1.0, 0); // blue base
        s.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(H_FS, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 3, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Read the rendered pixels back into the guest backing and inspect.
        self.virgl_transfer_from_host(RT_RES, 0, 0, RT_W, RT_H, RT_W * 4)?;
        let center = (32 * RT_W as usize + 32) * 4;
        let px = unsafe {
            let p = (backing_va + center) as *const u8;
            [p.read_volatile(), p.add(1).read_volatile(), p.add(2).read_volatile(), p.add(3).read_volatile()]
        };
        Ok(px)
    }

    /// GPU vector pipeline (M1a): render a per-pixel **gradient** quad and read
    /// it back. Uses a colour vertex attribute interpolated by the rasterizer
    /// (the simplest, hardware-exact gradient) — a vertex shader that passes the
    /// colour through and a fragment shader that outputs it. Proves end-to-end
    /// GPU gradient fills before the SDF/analytic-AA fragment shader is added.
    ///
    /// Returns `true` if the read-back top and bottom rows differ (interpolation
    /// happened), `false` if the quad came out flat.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn virgl_gradient_selftest(&mut self) -> Result<bool, GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_BIND_VERTEX_BUFFER, PIPE_BUFFER, PIPE_CLEAR_COLOR0,
            PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R32G32B32A32_FLOAT, PIPE_FORMAT_R8_UNORM,
            PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX, VIRGL_OBJECT_BLEND,
            VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJECT_VERTEX_ELEMENTS,
        };
        const RT_RES: u32 = 0xF6;
        const VBO_RES: u32 = 0xF7;
        const RT_W: u32 = 64;
        const RT_H: u32 = 64;
        // Handle namespace: the virgl context has ONE object table — handles
        // MUST be unique across subsystems (collisions fail silently on the
        // wire). Allocations: 10/11 boot VS/FS, 0x20..0x24 draw-selftest state,
        // 13 blur FS, 14 gl_scanout blit FS, 16/17 gradient VS/FS,
        // 0x30..0x34 blur surfaces/samplers, 0x42 gl_scanout surface,
        // 0x50..0x54 gradient-selftest state.
        const H_BLEND: u32 = 0x50;
        const H_DSA: u32 = 0x51;
        const H_RAST: u32 = 0x52;
        const H_VE: u32 = 0x53;
        const H_SURF: u32 = 0x54;
        const H_VS: u32 = 16;
        const H_FS: u32 = 17;

        // Gradient shaders: VS passes position + a colour varying through, FS
        // emits the interpolated colour.
        const VS: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], GENERIC[0]\n\
MOV OUT[0], IN[0]\n\
MOV OUT[1], IN[1]\n\
END\n";
        const FS: &str = "FRAG\n\
DCL IN[0], GENERIC[0], PERSPECTIVE\n\
DCL OUT[0], COLOR\n\
MOV OUT[0], IN[0]\n\
END\n";
        {
            let mut s = Submit3d::new();
            s.emit_create_shader(H_VS, PIPE_SHADER_VERTEX, VS);
            s.emit_create_shader(H_FS, PIPE_SHADER_FRAGMENT, FS);
            let bytes = s.as_bytes();
            let hdr = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: bytes.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&hdr, bytes)?;
        }

        self.virgl_create_rt(RT_RES, RT_W, RT_H)?;
        let backing_va = self.virgl_attach_backing(RT_RES, (RT_W * RT_H * 4) as usize)?;

        // Vertex buffer: 6 vertices × (vec4 pos + vec4 colour) = 192 bytes.
        const VBYTES: usize = 6 * 8 * 4;
        {
            use crate::protocol::{VirtioGpuResourceCreate3d, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D};
            let create = VirtioGpuResourceCreate3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
                resource_id: VBO_RES,
                target: PIPE_BUFFER,
                format: PIPE_FORMAT_R8_UNORM,
                bind: PIPE_BIND_VERTEX_BUFFER,
                width: VBYTES as u32,
                height: 1,
                depth: 1,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: 0,
                _padding: 0,
            };
            self.ctrl_submit_struct(&create)?;
            use crate::protocol::{VirtioGpuCtxAttachResource, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE};
            let attach = VirtioGpuCtxAttachResource {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                resource_id: VBO_RES,
                _padding: 0,
            };
            self.ctrl_submit_struct(&attach)?;
        }

        // Top (y=+1) red, bottom (y=-1) blue → vertical gradient. Each vertex:
        // [x, y, z, w,  r, g, b, a].
        #[rustfmt::skip]
        let verts: [f32; 48] = [
            -1.0, -1.0, 0.0, 1.0,   0.0, 0.0, 1.0, 1.0,
             1.0, -1.0, 0.0, 1.0,   0.0, 0.0, 1.0, 1.0,
            -1.0,  1.0, 0.0, 1.0,   1.0, 0.0, 0.0, 1.0,
             1.0, -1.0, 0.0, 1.0,   0.0, 0.0, 1.0, 1.0,
             1.0,  1.0, 0.0, 1.0,   1.0, 0.0, 0.0, 1.0,
            -1.0,  1.0, 0.0, 1.0,   1.0, 0.0, 0.0, 1.0,
        ];
        let mut vbytes = [0u8; VBYTES];
        for (i, v) in verts.iter().enumerate() {
            vbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(VBO_RES, &vbytes);
        s.emit_create_blend_default(H_BLEND);
        s.emit_bind_object(VIRGL_OBJECT_BLEND, H_BLEND);
        s.emit_create_dsa_default(H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_create_rasterizer_default(H_RAST);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RAST);
        // Two attributes: position @0, colour @16; stride 32.
        s.emit_create_vertex_elements(
            H_VE,
            &[
                (0, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT),
                (16, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT),
            ],
        );
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VE);
        s.emit_set_vertex_buffers(&[(32, 0, VBO_RES)]);
        s.emit_create_surface(H_SURF, RT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[H_SURF]);
        s.emit_set_viewport(RT_W as f32, RT_H as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 0.0, 1.0], 1.0, 0);
        s.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(H_FS, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        self.virgl_transfer_from_host(RT_RES, 0, 0, RT_W, RT_H, RT_W * 4)?;
        // Sample a near-top and near-bottom pixel; the gradient must differ.
        let read = |row: u32| -> [u8; 4] {
            let off = (row as usize * RT_W as usize + (RT_W as usize / 2)) * 4;
            unsafe {
                let p = (backing_va + off) as *const u8;
                [
                    p.read_volatile(),
                    p.add(1).read_volatile(),
                    p.add(2).read_volatile(),
                    p.add(3).read_volatile(),
                ]
            }
        };
        let top = read(4);
        let bottom = read(RT_H - 5);
        // BGRA: R is byte 2. One row should be red-dominant, the other blue.
        let interpolated =
            (i32::from(top[2]) - i32::from(bottom[2])).abs() > 32
                || (i32::from(top[0]) - i32::from(bottom[0])).abs() > 32;
        Ok(interpolated)
    }

    /// Increment B: create a passthrough vertex shader and a solid-color
    /// fragment shader from TGSI text. virglrenderer parses the text at create
    /// time, so this validates the CREATE_OBJECT(SHADER) text encoding (the
    /// foundation for the gaussian blur fragment shader). Object handles 10/11
    /// are reserved for these shaders within the virgl context.
    ///
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn virgl_shader_test(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{Submit3d, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX};
        const VS: &str = "VERT\nDCL IN[0]\nDCL OUT[0], POSITION\nMOV OUT[0], IN[0]\nEND\n";
        const FS: &str = "FRAG\nDCL OUT[0], COLOR\nIMM[0] FLT32 { 1.0000, 0.0000, 0.0000, 1.0000}\nMOV OUT[0], IMM[0]\nEND\n";
        let mut s = Submit3d::new();
        s.emit_create_shader(10, PIPE_SHADER_VERTEX, VS);
        s.emit_create_shader(11, PIPE_SHADER_FRAGMENT, FS);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }

    /// Issue TRANSFER_TO_HOST_3D for a box of `res_id` (guest backing → host
    /// GL texture) and wait for completion.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_transfer_to_host(
        &mut self,
        res_id: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        stride: u32,
    ) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuBox, VirtioGpuTransferHost3d, VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D,
        };
        let cmd = VirtioGpuTransferHost3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D),
            box_: VirtioGpuBox { x, y, z: 0, w, h, d: 1 },
            offset: (y as u64) * (stride as u64) + (x as u64) * 4,
            resource_id: res_id,
            level: 0,
            stride,
            layer_stride: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    /// Lazily create the GPU blur pipeline. The source/destination texture
    /// ALIASES the framebuffer VMO's display planes (rows 1600..3199), so the
    /// blur is zero-copy: TRANSFER_TO_HOST syncs the region into the GL
    /// texture, two shader passes blur it (H into a scratch RT, V back), and
    /// TRANSFER_FROM_HOST lands the result directly in the scanned-out VMO.
    /// Reuses the boot self-test's blend/DSA/rasterizer/vertex-elements and
    /// vertex shader (context-persistent objects).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_blur_init(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuMemEntry, VirtioGpuResourceAttachBacking,
            VirtioGpuResourceCreate3d, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
            VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
            VIRTIO_GPU_CMD_SUBMIT_3D,
        };
        use crate::virgl::{
            Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_BIND_VERTEX_BUFFER,
            PIPE_BUFFER, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R8_UNORM, PIPE_SHADER_FRAGMENT,
            PIPE_TEXTURE_2D,
        };
        // Scanout record carries the fb VMO physical base for the alias.
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        let fb_pa = record.backing_pa;
        // Display planes: rows 1600..3199 of the 1280×3200 VMO.
        let alias_pa = fb_pa + 1600 * 5120;
        let alias_len = 1600u32 * 5120;

        // FBSRC: 1280×1600 texture aliasing the display planes.
        let create_src = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xF8,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: 1280,
            height: 1600,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_src)?;
        let attach = VirtioGpuResourceAttachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: 0xF8,
            nr_entries: 1,
        };
        let entry = VirtioGpuMemEntry { addr: alias_pa, length: alias_len, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xF8,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;

        // TMP: 1280×800 scratch render target (host-side only, no backing).
        let create_tmp = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xF9,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: 1280,
            height: 800,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_tmp)?;
        let ctx_attach_tmp = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xF9,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_tmp)?;

        // QUAD: exact −1..1 quad (two triangles) so rasterization covers the
        // viewport box exactly — no scissor needed.
        let create_quad = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xFA,
            target: PIPE_BUFFER,
            format: PIPE_FORMAT_R8_UNORM,
            bind: PIPE_BIND_VERTEX_BUFFER,
            width: 96,
            height: 1,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_quad)?;
        let ctx_attach_quad = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xFA,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_quad)?;

        let quad: [f32; 24] = [
            -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, // tri 1
            1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, // tri 2
        ];
        let mut qbytes = [0u8; 96];
        for (i, v) in quad.iter().enumerate() {
            qbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        // Separable gaussian fragment shader. CONST[0] = (inv_w, inv_h, radius,
        // k = -1/(2σ²·ln2)); CONST[1] = (dir_x, dir_y, origin_x, origin_y).
        // Per tap: weight = 2^(k·i²) (≡ exp(-i²/2σ²)); the weight sum
        // normalizes, matching the CPU reference in blur_backdrop_separable_vmo.
        const FS_BLUR: &str = "FRAG\n\
            DCL IN[0], POSITION, LINEAR\n\
            DCL OUT[0], COLOR\n\
            DCL SAMP[0]\n\
            DCL SVIEW[0], 2D, FLOAT\n\
            DCL CONST[0..1]\n\
            DCL TEMP[0..5]\n\
            DCL ADDR[0]\n\
            IMM[0] FLT32 { 0.0000, 1.0000, -1.0000, 0.5000}\n\
            MOV TEMP[0], IMM[0].xxxx\n\
            MOV TEMP[1].x, IMM[0].xxxx\n\
            MUL TEMP[2].x, CONST[0].zzzz, IMM[0].zzzz\n\
            BGNLOOP\n\
            SGT TEMP[3].x, TEMP[2].xxxx, CONST[0].zzzz\n\
            IF TEMP[3].xxxx\n\
            BRK\n\
            ENDIF\n\
            MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx\n\
            MUL TEMP[3].x, TEMP[3].xxxx, CONST[0].wwww\n\
            EX2 TEMP[3].x, TEMP[3].xxxx\n\
            MAD TEMP[4].xy, CONST[1].xyyy, TEMP[2].xxxx, IN[0].xyyy\n\
            ADD TEMP[4].xy, TEMP[4].xyyy, CONST[1].zwww\n\
            MUL TEMP[4].xy, TEMP[4].xyyy, CONST[0].xyyy\n\
            TEX TEMP[5], TEMP[4], SAMP[0], 2D\n\
            MAD TEMP[0], TEMP[5], TEMP[3].xxxx, TEMP[0]\n\
            ADD TEMP[1].x, TEMP[1].xxxx, TEMP[3].xxxx\n\
            ADD TEMP[2].x, TEMP[2].xxxx, IMM[0].yyyy\n\
            ENDLOOP\n\
            RCP TEMP[1].x, TEMP[1].xxxx\n\
            MUL OUT[0], TEMP[0], TEMP[1].xxxx\n\
            END\n";

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(0xFA, &qbytes);
        s.emit_create_surface(0x30, 0xF8, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(0x31, 0xF9, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(0x32, 0xF8, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(0x33, 0xF9, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_state_default(0x34);
        s.emit_create_shader(13, PIPE_SHADER_FRAGMENT, FS_BLUR);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;
        self.virgl_blur_ready = true;
        Ok(())
    }

    /// Two-pass separable gaussian blur on the GPU via virgl.
    ///
    /// `y` is the absolute framebuffer row (display offset already applied by
    /// the caller); the fb-alias texture covers rows 1600..3199. The CPU
    /// fallback in `blur_backdrop_separable_vmo` remains the parity reference —
    /// on the first GPU blur the result is compared against it (interior of
    /// the region, tolerance 2 LSB) and a parity marker is emitted.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_virgl_blur(
        &mut self,
        fb: *mut u8,
        fb_len: usize,
        fb_w: usize,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        // When false, the blurred result is left in the GL texture (0xF8) only
        // and NOT copied back to the VMO — the caller composites over 0xF8
        // directly (glass layer), saving a transfer per frame. The one-shot
        // boot parity check forces a writeback regardless.
        writeback: bool,
    ) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX,
        };
        if !self.virgl_capable || !self.virgl_draw_ok || self.virgl_ctx_id == 0 {
            return Err(GfxError::DeviceNotFound);
        }
        if radius == 0 || w == 0 || h == 0 || fb_w != 1280 {
            return Err(GfxError::InvalidArgument);
        }
        // The alias texture covers display rows 1600..3199.
        if y < 1600 || x.saturating_add(w) > 1280 || (y - 1600).saturating_add(h) > 1600 {
            return Err(GfxError::InvalidArgument);
        }
        let y_rel = y - 1600;
        if !self.virgl_blur_ready {
            self.virgl_blur_init()?;
        }
        // First GPU-executed blur (init may have happened earlier via the GL
        // scanout bringup — the marker tracks first USE, not init).
        if !self.virgl_blur_first_done {
            self.virgl_blur_first_done = true;
            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_BLUR_GPU_ON);
        }

        // One-shot parity: snapshot the region and CPU-blur it for comparison.
        let parity_buf: Option<(usize, usize)> = if !self.virgl_parity_done
            && (w as usize) * (h as usize) * 4 <= 1024 * 1024
        {
            self.virgl_parity_done = true;
            match self.virgl_alloc_scratch((w as usize) * (h as usize) * 4) {
                Ok(va) => {
                    // Tightly pack the region into the scratch and CPU-blur it.
                    for row in 0..h as usize {
                        let src_off = (y as usize + row) * fb_w * 4 + (x as usize) * 4;
                        if src_off + (w as usize) * 4 > fb_len {
                            break;
                        }
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                fb.add(src_off),
                                (va + row * (w as usize) * 4) as *mut u8,
                                (w as usize) * 4,
                            );
                        }
                    }
                    let _ = blur_backdrop_separable_vmo(
                        va as *mut u8,
                        (w as usize) * (h as usize) * 4,
                        w as usize,
                        0,
                        0,
                        w,
                        h,
                        radius,
                        0,
                    );
                    Some((va, (w as usize) * 4))
                }
                Err(_) => None,
            }
        } else {
            None
        };

        // Sync the region from the guest VMO into the GL texture.
        self.virgl_transfer_to_host(0xF8, x, y_rel, w, h, 5120)?;

        let sigma = (radius as f32) / 2.0;
        let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
        let r = radius as f32;

        let mut s = Submit3d::new();
        // Rebind pipeline state explicitly — other passes (gl_scanout blit,
        // selftests) bind their own objects and virgl state is context-global.
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        // Pass 1: horizontal blur, FBSRC region → TMP at (0,0,w,h).
        s.emit_set_framebuffer_state(0, &[0x31]);
        s.emit_set_viewport_box(0.0, 0.0, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[0x32]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[0x34]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / 1280.0, 1.0 / 1600.0, r, k, 1.0, 0.0, x as f32, y_rel as f32],
        );
        s.emit_bind_shader(10, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(13, PIPE_SHADER_FRAGMENT);
        s.emit_set_vertex_buffers(&[(16, 0, 0xFA)]);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        // Pass 2: vertical blur, TMP (0,0,w,h) → FBSRC region.
        s.emit_set_framebuffer_state(0, &[0x30]);
        s.emit_set_viewport_box(x as f32, y_rel as f32, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[0x33]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / 1280.0, 1.0 / 800.0, r, k, 0.0, 1.0, -(x as f32), -(y_rel as f32)],
        );
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Land the blurred pixels back in the scanned-out guest VMO — unless the
        // caller will composite over 0xF8 directly (glass) and only the boot
        // parity check (which reads the VMO) needs it.
        if writeback || parity_buf.is_some() {
            self.virgl_transfer_from_host(0xF8, x, y_rel, w, h, 5120)?;
        }

        // Compare GPU result vs CPU reference over the interior.
        if let Some((ref_va, ref_stride)) = parity_buf {
            let inset = (radius + 1) as usize;
            let mut max_diff: u8 = 0;
            if (w as usize) > 2 * inset && (h as usize) > 2 * inset {
                for row in inset..(h as usize - inset) {
                    for col in inset..(w as usize - inset) {
                        let gpu_off = (y as usize + row) * fb_w * 4 + (x as usize + col) * 4;
                        let ref_off = row * ref_stride + col * 4;
                        for c in 0..3 {
                            let g = unsafe { fb.add(gpu_off + c).read_volatile() };
                            let r8 = unsafe { ((ref_va + ref_off + c) as *const u8).read() };
                            let d = g.abs_diff(r8);
                            if d > max_diff {
                                max_diff = d;
                            }
                        }
                    }
                }
                let _ = nexus_abi::debug_println(if max_diff <= 2 {
                    crate::markers::GPUD_VIRGL_BLUR_PARITY_OK
                } else {
                    crate::markers::GPUD_VIRGL_BLUR_PARITY_OFF
                });
            }
        }
        Ok(())
    }

    /// Allocate and map a page-aligned scratch VMO in the virgl backing VA
    /// region (no resource attach). Returns the VA.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn virgl_alloc_scratch(&mut self, byte_len: usize) -> Result<usize, GfxError> {
        let slot = self.virgl_backing_count;
        if slot >= 8 {
            return Err(GfxError::ResourceExhausted);
        }
        let va = GPU_VIRGL_BACKING_BASE_VA + slot * GPU_VIRGL_BACKING_STRIDE;
        let len = align_page(byte_len);
        let vmo = nexus_abi::vmo_create(len).map_err(|_| GfxError::ResourceExhausted)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..len).step_by(4096) {
            nexus_abi::vmo_map_page(vmo, va + offset, offset, flags)
                .map_err(|_| GfxError::MmioFault)?;
        }
        self.virgl_backing_count = slot + 1;
        Ok(va)
    }

    /// Read the device feature bits and acknowledge the subset we support for
    /// virgl 3D. Must run after ACKNOWLEDGE|DRIVER and before FEATURES_OK.
    ///
    /// Sets `self.virgl_capable` iff the device offered `VIRTIO_GPU_F_VIRGL`.
    /// We ack VIRGL (3D), CONTEXT_INIT (so CTX_CREATE may select the VIRGL2
    /// capset), and VERSION_1 (modern virtio — the queue is already driven via
    /// the split DESC/DRIVER/DEVICE registers, so VERSION_1 is the correct mode).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    fn negotiate_features_virgl(&mut self) {
        // Low feature word (bits 0..31).
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES_SEL, 0);
        let dev_lo = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES);
        // High feature word (bits 32..63), where VERSION_1 lives.
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES_SEL, 1);
        let dev_hi = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES);

        let want_lo = protocol::VIRTIO_GPU_F_VIRGL | protocol::VIRTIO_GPU_F_CONTEXT_INIT;
        let drv_lo = dev_lo & want_lo;
        let drv_hi = dev_hi & protocol::VIRTIO_F_VERSION_1_HI;

        self.virgl_capable = (dev_lo & protocol::VIRTIO_GPU_F_VIRGL) != 0;

        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 0);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, drv_lo);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 1);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, drv_hi);
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
        let row_offset =
            u64::from(rect.y).checked_mul(row_stride).ok_or(GfxError::ResourceExhausted)?;
        let col_offset =
            u64::from(rect.x).checked_mul(bytes_per_pixel).ok_or(GfxError::ResourceExhausted)?;
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
            resource_id: 1,
            hot_x: 0,
            hot_y: 0,
            _padding: 0,
        };
        self.cursor_submit_struct(&cmd)
    }

    pub(crate) fn ctrl_submit_struct<T>(&mut self, cmd: &T) -> Result<(), GfxError> {
        let bytes = unsafe {
            core::slice::from_raw_parts((cmd as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        self.ctrl_submit_bytes(bytes)
    }

    /// Submit a cursor command (UPDATE_CURSOR / MOVE_CURSOR) on the cursor queue.
    /// Cursor-queue commands carry no response payload (see `submit_no_response`).
    fn cursor_submit_struct<T>(&mut self, cmd: &T) -> Result<(), GfxError> {
        let bytes = unsafe {
            core::slice::from_raw_parts((cmd as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        let queue = self.cursorq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        queue.submit_no_response(self.mmio_base, bytes)
    }

    pub(crate) fn ctrl_submit_pair<A, B>(&mut self, a: &A, b: &B) -> Result<(), GfxError> {
        let a_bytes = unsafe {
            core::slice::from_raw_parts((a as *const A).cast::<u8>(), core::mem::size_of::<A>())
        };
        let b_bytes = unsafe {
            core::slice::from_raw_parts((b as *const B).cast::<u8>(), core::mem::size_of::<B>())
        };
        let mmio = self.mmio_base;
        let batch = self.ctrl_batch;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        if batch {
            queue.enqueue_pair(mmio, a_bytes, b_bytes).map(|_| ())
        } else {
            queue.submit_two(mmio, a_bytes, b_bytes)
        }
    }

    fn ctrl_submit_bytes(&mut self, bytes: &[u8]) -> Result<(), GfxError> {
        let mmio = self.mmio_base;
        let batch = self.ctrl_batch;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        if batch {
            queue.enqueue_pair(mmio, bytes, &[]).map(|_| ())
        } else {
            queue.submit(mmio, bytes)
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl CtrlQueue {
    /// `slots` = number of in-flight command buffers (control = `RING_SLOTS`,
    /// cursor = 1). The command pool is `slots` contiguous 4 KiB pages; the
    /// response pool is one page (`slots` × `RESP_SLOT_SIZE`). `slots` must be
    /// ≤ `RING_SLOTS` so the descriptor table (`QUEUE_LEN`) and the single
    /// response page suffice.
    fn new(
        mmio_base: usize,
        queue_index: u32,
        queue_va: usize,
        cmd_va_base: usize,
        resp_va_base: usize,
        slots: usize,
    ) -> Result<Self, GpuDriverError> {
        debug_assert!(slots >= 1 && slots <= RING_SLOTS);
        debug_assert!(slots * RESP_SLOT_SIZE <= 4096);
        let cmd_pool_len = slots * 4096;
        let q_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let cmd_vmo = nexus_abi::vmo_create(cmd_pool_len).map_err(|_| GpuDriverError::MmioFault)?;
        let resp_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        nexus_abi::vmo_map_page(q_vmo, queue_va, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        // Map the whole command pool (one page per in-flight slot, contiguous).
        for i in 0..slots {
            nexus_abi::vmo_map_page(cmd_vmo, cmd_va_base + i * 4096, i * 4096, flags)
                .map_err(|_| GpuDriverError::MmioFault)?;
        }
        nexus_abi::vmo_map_page(resp_vmo, resp_va_base, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        let mut q_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        let mut cmd_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        let mut resp_info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(q_vmo, &mut q_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(cmd_vmo, &mut cmd_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(resp_vmo, &mut resp_info).map_err(|_| GpuDriverError::MmioFault)?;
        unsafe {
            core::ptr::write_bytes(queue_va as *mut u8, 0, 4096);
            core::ptr::write_bytes(cmd_va_base as *mut u8, 0, cmd_pool_len);
            core::ptr::write_bytes(resp_va_base as *mut u8, 0, 4096);
        }

        let desc_bytes = core::mem::size_of::<VqDesc>() * QUEUE_LEN;
        let avail_bytes = core::mem::size_of::<VqAvail<QUEUE_LEN>>();
        let used_off = align4(desc_bytes + avail_bytes);
        let desc_va = queue_va;
        let avail_va = queue_va + desc_bytes;
        let used_va = queue_va + used_off;

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
            queue_index,
            _queue_vmo: q_vmo,
            _cmd_vmo: cmd_vmo,
            _resp_vmo: resp_vmo,
            desc: desc_va as *mut VqDesc,
            avail: avail_va as *mut VqAvail<QUEUE_LEN>,
            used: used_va as *mut VqUsed<QUEUE_LEN>,
            cmd_va: cmd_va_base,
            cmd_pa: cmd_info.base,
            resp_va: resp_va_base,
            resp_pa: resp_info.base,
            ring: nexus_driverkit::SubmitRing::new(slots),
            last_used: 0,
            mmio_base,
            irq_num: 0,
            irq_ep: 0,
        })
    }

    fn submit(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<(), GfxError> {
        self.submit_two(mmio_base, bytes, &[])
    }

    /// Bind this queue to a GPU ring-buffer IRQ so the completion wait can BLOCK
    /// on the interrupt instead of busy-polling. `irq_ep` is the endpoint cap slot
    /// the kernel routes the PLIC source to (set via `irq_bind`); `irq_num` is that
    /// source. Both 0 keeps the legacy spin+yield path.
    fn set_gpu_irq(&mut self, irq_num: u32, irq_ep: u32) {
        self.irq_num = irq_num;
        self.irq_ep = irq_ep;
    }

    // ── slot addressing (the command/response buffer pools are contiguous) ──
    #[inline]
    fn cmd_slot_va(&self, slot: RingSlot) -> usize {
        self.cmd_va + slot.0 as usize * 4096
    }
    #[inline]
    fn cmd_slot_pa(&self, slot: RingSlot) -> u64 {
        self.cmd_pa + slot.0 as u64 * 4096
    }
    #[inline]
    fn resp_slot_va(&self, slot: RingSlot) -> usize {
        self.resp_va + slot.0 as usize * RESP_SLOT_SIZE
    }
    #[inline]
    fn resp_slot_pa(&self, slot: RingSlot) -> u64 {
        self.resp_pa + slot.0 as u64 * RESP_SLOT_SIZE as u64
    }

    /// Reap completed commands: walk the new `used.ring` entries and free their
    /// slots. The used element's `id` is the head descriptor (`2*slot`), so `id/2`
    /// maps a completion back to its slot. This is the consumer half of the
    /// pipeline — frame N's completion is observed here (typically during frame
    /// N+1's enqueue), so the present never blocks on its own completion.
    fn harvest(&mut self) {
        let used_idx = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
        while self.last_used != used_idx {
            let elem = unsafe {
                core::ptr::read_volatile(&(*self.used).ring[self.last_used as usize % QUEUE_LEN])
            };
            let slot = (elem.id / 2) as usize;
            if slot < self.ring.capacity() {
                // Free the slot. Idempotent: a spurious/duplicate completion for an
                // already-free slot is ignored (`complete` errors, no double-count) — same
                // as the old `busy &= !(1<<slot)` bitmask clear.
                let _ = self.ring.complete(nexus_driverkit::Slot(slot as u8));
            }
            self.last_used = self.last_used.wrapping_add(1);
        }
    }

    /// Round-robin reservation of a free slot (harvest first). `None` = ring full.
    fn find_free_slot(&mut self) -> Option<RingSlot> {
        self.harvest();
        // Reserve via the shared ring. Reserving here rather than at `publish` is
        // behaviour-equivalent: every `find_free_slot` / `alloc_free_slot` is unconditionally
        // followed by `publish` (no early return between), so a reserved slot is always
        // submitted — no leak. `RING_SLOTS ≤ 16` so the u8→u16 widen is lossless.
        self.ring.try_alloc().map(|(slot, _ticket)| RingSlot(slot.0 as u16))
    }

    /// Allocate a free slot, applying back-pressure if the ring is full: block on
    /// the GPU IRQ + harvest until one frees (deadline-bounded). On a (degraded)
    /// timeout, force-resync the in-flight set so the ring can never deadlock.
    fn alloc_free_slot(&mut self) -> Result<RingSlot, GfxError> {
        if let Some(slot) = self.find_free_slot() {
            return Ok(slot);
        }
        let start = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
        let deadline = start.saturating_add(GPU_WAIT_DEADLINE_NS);
        loop {
            self.block_on_irq(deadline);
            if let Some(slot) = self.find_free_slot() {
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                return Ok(slot);
            }
            if nexus_abi::nsec().map_err(|_| GfxError::MmioFault)? >= deadline {
                // Degraded recovery: abandon the stuck in-flight set + resync the
                // harvest cursor so we never wedge. Best-effort (a lost IRQ only).
                self.ring.reset();
                self.last_used = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                // `reset` emptied the ring, so this reservation always succeeds.
                return self
                    .ring
                    .try_alloc()
                    .map(|(slot, _)| RingSlot(slot.0 as u16))
                    .ok_or(GfxError::MmioFault);
            }
        }
    }

    /// Block once on the GPU ring-buffer IRQ (deadline-bounded) or yield if the
    /// queue isn't IRQ-bound. The reactive wait primitive shared by `wait_slot`
    /// and `alloc_free_slot`.
    fn block_on_irq(&self, deadline: u64) {
        if self.irq_ep != 0 {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 16];
            if nexus_abi::ipc_recv_v1(
                self.irq_ep,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_TRUNCATE,
                deadline,
            )
            .is_ok()
                && !GPU_IRQ_WAKE_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed)
            {
                // Proof (once): a real GPU ring-buffer IRQ woke a wait.
                let _ = nexus_abi::debug_println("gpud: gpu irq wake");
            }
        } else {
            let _ = nexus_abi::yield_();
        }
    }

    /// Make a written descriptor chain available to the device + notify, and mark
    /// the slot in-flight (freed later by `harvest` when its completion returns).
    #[inline]
    fn publish(&mut self, mmio_base: usize, slot: RingSlot) {
        let head = slot.head_desc();
        unsafe {
            let idx = core::ptr::read_volatile(&(*self.avail).idx);
            core::ptr::write_volatile(
                &mut (*self.avail).ring[(idx as usize) % QUEUE_LEN],
                head as u16,
            );
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*self.avail).idx, idx.wrapping_add(1));
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NOTIFY, self.queue_index);
        // The slot was already reserved (marked in-flight) at alloc by `ring.try_alloc`;
        // `harvest` frees it when the completion returns. (Was `self.busy |= 1<<slot` here.)
    }

    /// Enqueue a command that expects a device response (cmd → resp 2-descriptor
    /// chain) WITHOUT waiting for completion. The caller `drain`s the batch later
    /// and may inspect the response at the returned slot. `first`+`second` are
    /// concatenated into the slot's command buffer (`second` empty = single blob).
    fn enqueue_pair(
        &mut self,
        mmio_base: usize,
        first: &[u8],
        second: &[u8],
    ) -> Result<RingSlot, GfxError> {
        let total = first.len().checked_add(second.len()).ok_or(GfxError::ResourceExhausted)?;
        if total == 0 || total > 4096 || core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() > total {
            return Err(GfxError::CommandRejected);
        }
        let slot = self.alloc_free_slot()?;
        let head = slot.head_desc();
        let cmd_va = self.cmd_slot_va(slot);
        let cmd_pa = self.cmd_slot_pa(slot);
        let resp_pa = self.resp_slot_pa(slot);
        unsafe {
            core::ptr::write_bytes(cmd_va as *mut u8, 0, total);
            core::ptr::write_bytes(self.resp_slot_va(slot) as *mut u8, 0, RESP_SLOT_SIZE);
            core::ptr::copy_nonoverlapping(first.as_ptr(), cmd_va as *mut u8, first.len());
            if !second.is_empty() {
                core::ptr::copy_nonoverlapping(
                    second.as_ptr(),
                    (cmd_va + first.len()) as *mut u8,
                    second.len(),
                );
            }
            core::ptr::write_volatile(
                self.desc.add(head),
                VqDesc { addr: cmd_pa, len: total as u32, flags: 1, next: slot.resp_desc() as u16 },
            );
            core::ptr::write_volatile(
                self.desc.add(slot.resp_desc()),
                VqDesc {
                    addr: resp_pa,
                    len: core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() as u32,
                    flags: 2,
                    next: 0,
                },
            );
        }
        self.publish(mmio_base, slot);
        Ok(slot)
    }

    /// Enqueue a response-less command (single read-only descriptor) WITHOUT
    /// waiting. Used by the cursor queue (UPDATE/MOVE_CURSOR carry no response).
    fn enqueue_single(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<RingSlot, GfxError> {
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(GfxError::CommandRejected);
        }
        let slot = self.alloc_free_slot()?;
        let head = slot.head_desc();
        let cmd_va = self.cmd_slot_va(slot);
        let cmd_pa = self.cmd_slot_pa(slot);
        unsafe {
            core::ptr::write_bytes(cmd_va as *mut u8, 0, bytes.len());
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), cmd_va as *mut u8, bytes.len());
            core::ptr::write_volatile(
                self.desc.add(head),
                VqDesc { addr: cmd_pa, len: bytes.len() as u32, flags: 0, next: 0 },
            );
        }
        self.publish(mmio_base, slot);
        Ok(slot)
    }

    /// Synchronously wait for ONE slot's completion. Used by `submit_two` /
    /// `submit_no_response` for the init + 2D/mmio paths, where each command's
    /// response must be in hand before the next is issued. Harvest-driven +
    /// reactive (block on the GPU ring-buffer IRQ), bounded by `GPU_WAIT_DEADLINE_NS`
    /// so a lost/late IRQ degrades to a timeout, never a hang.
    ///
    /// The pipelined present does NOT call this — it enqueues and lets the next
    /// frame `harvest` the completion (so a deferred textured-draw completion never
    /// blocks the present).
    fn wait_slot(&mut self, slot: RingSlot) -> Result<(), GfxError> {
        let dk_slot = nexus_driverkit::Slot(slot.0 as u8);
        let start = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
        let deadline = start.saturating_add(GPU_WAIT_DEADLINE_NS);
        loop {
            self.harvest();
            if !self.ring.is_in_flight(dk_slot) {
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                return Ok(());
            }
            if nexus_abi::nsec().map_err(|_| GfxError::MmioFault)? >= deadline {
                // Abandon the stuck slot (degraded, lost-IRQ only): free it WITHOUT counting
                // a completion (the command never finished), so a fence can't jump past it.
                self.ring.abandon(dk_slot);
                if self.irq_ep != 0 {
                    self.ack_gpu_irq();
                }
                return Err(GfxError::MmioFault);
            }
            self.block_on_irq(deadline);
        }
    }

    /// De-assert + re-arm this queue's GPU IRQ. Order matters (same lesson as
    /// virtio-input): drain the queued notification, clear the device's
    /// InterruptStatus, THEN `irq_complete` so the source can't immediately storm.
    fn ack_gpu_irq(&self) {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        let _ = nexus_abi::ipc_recv_v1_nb(self.irq_ep, &mut hdr, &mut buf, true);
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_INTERRUPT_STATUS);
        if status != 0 {
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_INTERRUPT_ACK, status);
        }
        let _ = nexus_abi::irq_complete(self.irq_num);
    }

    /// Submit a command that the device completes WITHOUT writing a response
    /// payload. The virtio-gpu cursor queue is such a queue: QEMU processes
    /// UPDATE_CURSOR/MOVE_CURSOR and pushes the used element with len=0 and no
    /// response header. Posting a response descriptor and demanding
    /// RESP_OK_NODATA (like `submit`) therefore always "fails" — the historical
    /// reason the hardware cursor was abandoned. Single read-only descriptor,
    /// completion = used-ring advance.
    fn submit_no_response(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<(), GfxError> {
        // Single read-only command, no response payload to inspect — the used-ring
        // advance IS the completion. Enqueue then wait for that slot (synchronous).
        let slot = self.enqueue_single(mmio_base, bytes)?;
        self.wait_slot(slot)
    }

    /// Synchronous single command: enqueue one cmd→resp pair, wait for that slot,
    /// classify the response. Behaviour is identical to the pre-pipeline path —
    /// used by init, the 2D/mmio present, and every non-batched caller. The
    /// pipelined present instead `enqueue_pair`s every draw and never waits (the
    /// next frame `harvest`s the completion).
    fn submit_two(
        &mut self,
        mmio_base: usize,
        first: &[u8],
        second: &[u8],
    ) -> Result<(), GfxError> {
        let slot = self.enqueue_pair(mmio_base, first, second)?;
        self.wait_slot(slot)?;
        self.classify_resp(slot, "ctrl")
    }

    /// Classify a drained slot's device response. RESP_OK_NODATA → Ok; any error
    /// type is logged (the string names the exact QEMU rejection) → CommandRejected.
    fn classify_resp(&self, slot: RingSlot, label: &str) -> Result<(), GfxError> {
        let hdr = unsafe {
            core::ptr::read_volatile(self.resp_slot_va(slot) as *const protocol::VirtioGpuCtrlHdr)
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
        Err(GfxError::CommandRejected)
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn ctrl_hdr(type_: u32) -> protocol::VirtioGpuCtrlHdr {
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
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
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
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    color: RgbaColor,
) {
    let rgba = color.as_array();
    if rgba[3] == 0 {
        return;
    }
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
            if idx + 4 > fb_len {
                continue;
            }
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
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    _saturation_pct: u32,
) -> Result<(), GfxError> {
    if radius == 0 {
        return Ok(());
    }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius as usize;
    let pixels = (end_x - x) as usize;
    if pixels == 0 {
        return Ok(());
    }
    // Horizontal pass: box-blur each row in-place with a scratch buffer.
    // Allocate on stack — worst case 1280*4 = 5120 bytes for a full-width row.
    let mut scratch: [u8; 5120] = [0u8; 5120];
    let row_bytes = pixels * 4;
    if row_bytes > scratch.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for py in y..end_y {
        let row_start = (py as usize * fb_w + x as usize) * 4;
        if row_start + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(row_start), scratch.as_mut_ptr(), row_bytes);
        }
        let mut sums: [u64; 4] = [0; 4];
        let mut left: usize = 0;
        let mut right = r.min(pixels.saturating_sub(1));
        for j in left..=right {
            let bi = j * 4;
            for c in 0..4 {
                sums[c] += scratch[bi + c] as u64;
            }
        }
        for i in 0..pixels {
            let count = (right - left + 1) as u64;
            let di = row_start + i * 4;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(
                        fb.add(di + c),
                        (sums[c] / count.max(1)).min(255) as u8,
                    );
                }
            }
            if i + 1 < pixels {
                let next_left = (i + 1).saturating_sub(r);
                if next_left > left {
                    let bi = left * 4;
                    for c in 0..4 {
                        sums[c] = sums[c].saturating_sub(scratch[bi + c] as u64);
                    }
                    left = next_left;
                }
                let next_right = (i + 1 + r).min(pixels.saturating_sub(1));
                if next_right > right {
                    right = next_right;
                    let bi = right * 4;
                    for c in 0..4 {
                        sums[c] += scratch[bi + c] as u64;
                    }
                }
            }
        }
    }
    // Vertical pass
    let col_h = (end_y - y) as usize;
    let mut col_buf: [u8; 3200] = [0u8; 3200]; // 800 rows * 4 bytes
    if col_h * 4 > col_buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..col_h {
            let src = (y as usize + row_i) * fb_w + col_off;
            if src + 4 <= fb_len {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        fb.add(src),
                        col_buf.as_mut_ptr().add(row_i * 4),
                        4,
                    );
                }
            }
        }
        let mut sums: [u64; 4] = [0; 4];
        let mut top: usize = 0;
        let mut bot = r.min(col_h.saturating_sub(1));
        for j in top..=bot {
            let bi = j * 4;
            for c in 0..4 {
                sums[c] += col_buf[bi + c] as u64;
            }
        }
        for i in 0..col_h {
            let count = (bot - top + 1) as u64;
            let dst = (y as usize + i) * fb_w + col_off;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(
                        fb.add(dst + c),
                        (sums[c] / count.max(1)).min(255) as u8,
                    );
                }
            }
            if i + 1 < col_h {
                let ntop = (i + 1).saturating_sub(r);
                if ntop > top {
                    let bi = top * 4;
                    for c in 0..4 {
                        sums[c] = sums[c].saturating_sub(col_buf[bi + c] as u64);
                    }
                    top = ntop;
                }
                let nbot = (i + 1 + r).min(col_h.saturating_sub(1));
                if nbot > bot {
                    bot = nbot;
                    let bi = bot * 4;
                    for c in 0..4 {
                        sums[c] += col_buf[bi + c] as u64;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Separable gaussian blur — the virgl GPU path target.
///
/// Uses a precomputed gaussian kernel for higher-quality blur than the box-blur
/// fallback. The two-pass separable convolution (horizontal + vertical) is the
/// same algorithm a GPU compute shader would execute; this CPU implementation
/// serves as both the reference and the fallback when virgl is unavailable.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn blur_backdrop_separable_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    _saturation_pct: u32,
) -> Result<(), GfxError> {
    if radius == 0 {
        return Ok(());
    }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius as usize;
    let pixels = (end_x - x) as usize;
    let rows = (end_y - y) as usize;
    if pixels == 0 || rows == 0 {
        return Ok(());
    }

    // Precompute gaussian kernel weights for the given radius.
    // σ = radius / 2 gives a natural falloff.
    let sigma = (r as f32) / 2.0_f32.max(0.5);
    let kernel_size = r * 2 + 1;
    let kernel: [f32; 41] = {
        let mut k = [0.0_f32; 41];
        let mut sum = 0.0_f32;
        for i in 0..kernel_size.min(41) {
            let dx = (i as i32 - r as i32) as f32;
            let w = libm::expf(-dx * dx / (2.0 * sigma * sigma));
            k[i] = w;
            sum += w;
        }
        // Normalize
        if sum > 0.0 {
            for v in k.iter_mut().take(kernel_size.min(41)) {
                *v /= sum;
            }
        }
        k
    };
    let k_len = kernel_size.min(41);

    // Horizontal pass: convolve each row with the gaussian kernel.
    // Stack-allocated scratch: worst case 1280*4 = 5120 bytes.
    let row_bytes = pixels * 4;
    let mut scratch: [u8; 5120] = [0u8; 5120];
    if row_bytes > scratch.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for py in y..end_y {
        let row_start = (py as usize * fb_w + x as usize) * 4;
        if row_start + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(row_start), scratch.as_mut_ptr(), row_bytes);
        }
        for i in 0..pixels {
            let mut acc: [f32; 4] = [0.0; 4];
            for ki in 0..k_len {
                let src_i = (i as i32 + ki as i32 - r as i32).clamp(0, pixels as i32 - 1) as usize;
                let si = src_i * 4;
                let w = kernel[ki];
                for c in 0..4 {
                    acc[c] += scratch[si + c] as f32 * w;
                }
            }
            let di = row_start + i * 4;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(fb.add(di + c), libm::roundf(acc[c]).clamp(0.0, 255.0) as u8);
                }
            }
        }
    }

    // Vertical pass: convolve each column with the gaussian kernel.
    let col_bytes = rows * 4;
    let mut col_buf: [u8; 3200] = [0u8; 3200]; // 800 rows * 4 bytes
    if col_bytes > col_buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..rows {
            let src = (y as usize + row_i) * fb_w + col_off;
            if src + 4 <= fb_len {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        fb.add(src),
                        col_buf.as_mut_ptr().add(row_i * 4),
                        4,
                    );
                }
            }
        }
        for i in 0..rows {
            let mut acc: [f32; 4] = [0.0; 4];
            for ki in 0..k_len {
                let src_i = (i as i32 + ki as i32 - r as i32).clamp(0, rows as i32 - 1) as usize;
                let si = src_i * 4;
                let w = kernel[ki];
                for c in 0..4 {
                    acc[c] += col_buf[si + c] as f32 * w;
                }
            }
            let di = (y as usize + i) * fb_w + col_off;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(fb.add(di + c), libm::roundf(acc[c]).clamp(0.0, 255.0) as u8);
                }
            }
        }
    }

    Ok(())
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn blit_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), GfxError> {
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(dst_x)).min(fb_w_u.saturating_sub(src_x));
    let copy_h = h.min(fb_h.saturating_sub(dst_y)).min(fb_h.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }
    // Use stack scratch for row copy to handle overlapping regions safely.
    let row_bytes = copy_w as usize * 4;
    let mut buf: [u8; 5120] = [0u8; 5120];
    if row_bytes > buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = (sy as usize * fb_w + src_x as usize) * 4;
        let dst_off = (dy as usize * fb_w + dst_x as usize) * 4;
        if src_off + row_bytes > fb_len || dst_off + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(src_off), buf.as_mut_ptr(), row_bytes);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), fb.add(dst_off), row_bytes);
        }
    }
    Ok(())
}

/// Like `blit_vmo`, but ALPHA-BLENDS the source over the destination (instead
/// of an opaque copy). Used by the CPU glass path: composite a translucent
/// layer (e.g. the chat panel with a low-alpha background) over a blurred
/// backdrop so the blur shows through. Source rows are read into scratch first
/// so a src/dst overlap is safe.
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(clippy::too_many_arguments)]
fn blit_blend_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), GfxError> {
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(dst_x)).min(fb_w_u.saturating_sub(src_x));
    let copy_h = h.min(fb_h.saturating_sub(dst_y)).min(fb_h.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }
    let row_bytes = copy_w as usize * 4;
    let mut buf: [u8; 5120] = [0u8; 5120];
    if row_bytes > buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = (sy as usize * fb_w + src_x as usize) * 4;
        let dst_off = (dy as usize * fb_w + dst_x as usize) * 4;
        if src_off + row_bytes > fb_len || dst_off + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(src_off), buf.as_mut_ptr(), row_bytes);
        }
        for col in 0..copy_w as usize {
            let s = [buf[col * 4], buf[col * 4 + 1], buf[col * 4 + 2], buf[col * 4 + 3]];
            blend_pixel_vmo(fb, dst_off + col * 4, &s);
        }
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

/// Footprint of the procedural [`CURSOR_ARROW`] fallback (drawn when no SVG cursor
/// sprite has been uploaded yet). Matches the arrow bitmap below.
pub(crate) const CURSOR_FALLBACK_W: u32 = 12;
pub(crate) const CURSOR_FALLBACK_H: u32 = 19;

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
        b'B' => [40, 40, 40, 255],    // soft dark border
        b'W' => [255, 255, 255, 255], // white fill
        _ => [0, 0, 0, 0],            // transparent
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn blend_pixel_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    let alpha = src[3] as u32;
    if alpha == 0 {
        return;
    }
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
            core::ptr::write_volatile(
                fb.add(idx + 3),
                src[3].saturating_add((((inv * dst_alpha) * 257 + 32768) >> 16) as u8),
            );
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn corner_dist_i32(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy - r * r
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const fn align_page(value: usize) -> usize {
    (value + 4095) & !4095
}
