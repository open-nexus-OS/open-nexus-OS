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
    GPUD_RESOURCE_ATTACH_CMD_FAIL, GPUD_RESOURCE_CREATED, GPUD_RESOURCE_CREATE_CMD_FAIL,
    GPUD_RESOURCE_VMO_CREATE_FAIL, GPUD_RESOURCE_VMO_MAP_FAIL,
};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

mod attach;
mod backing;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod bootstrap;
// The CPU executor + the software cursor's state of the 2D virtio path (the board's display
// composes nothing on the CPU — TASK-0215 5d). The sprite store is host-tested.
#[cfg(any(test, all(feature = "os-lite", target_os = "none")))]
pub(crate) mod cpu_frame;
mod cursor;
// The pointer's shape cache, shared by the CPU executor and the board's pointer layer.
#[cfg(any(test, all(feature = "os-lite", target_os = "none")))]
pub(crate) mod cursor_shapes;
// TASK-0251 P2: the board's display controller + HDMI encoder (granted instead of a GPU).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod dc;
// The board display's ring of planes and its latched flips — outside `dc` so its rules run in
// the host's tests (TASK-0342).
#[cfg(any(test, all(feature = "os-lite", target_os = "none")))]
mod flip_ring;
// What the request loop drives: the virtio GPU or the board's display controller.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod display;
mod display_mode;
mod framebuffer;
// The glass blur on virgl: the textures, the half-resolution chain, the cache's reads and writes.
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
mod glass_blur;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod lifecycle;
mod present;
mod probe_policy;
mod raster;
mod resources;
// Pure policy, host-tested; the OS probe/attach paths consume it.
#[cfg(any(test, all(feature = "os-lite", target_os = "none")))]
pub(crate) mod scanout_policy;
// RFC-0093 §5 on a display that switches what it scans (the board's controller); host-tested.
#[cfg(any(test, all(feature = "os-lite", target_os = "none")))]
pub(crate) mod splash_hold;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod transport;
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
pub(crate) mod virgl3d;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod virtio_display;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod virtqueue;

// The validation + error-mapping helpers live in `resources`; the GfxBackend
// trait impl below resolves them by bare name.
pub(crate) use resources::map_nexus_error;
use resources::{resource_byte_len, validate_rect, ResourceRecord};

// The os-lite transport + virtqueue layer (MMIO map, reg helpers, ring types +
// `CtrlQueue`) is shared by the GfxBackend command methods still in this file.
// Re-glob so those methods resolve the moved items by bare name.
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(unused_imports)]
pub(crate) use transport::*;
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(unused_imports)]
pub(crate) use virtqueue::*;
// The shared boot-splash machinery (bootstrap.rs owns it): the pulse curve —
// sampled by the 2D phase tick and the GL splash blits so the breathing stays
// continuous across the scanout switch — and the single splash-image compose
// (glow + wordmark) the 2D attach, the 2D pulse band and the GL wallpaper seed
// all render through (one asset copy, one implementation).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) use bootstrap::compose_splash_region;
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
pub(crate) use bootstrap::splash_pulse_q8;

// Bring the moved free-function clusters into the parent namespace so the impl
// blocks still in this file resolve them by bare name (zero call-site churn).
// `cursor` is ungated (its CURSOR_FALLBACK_* consts are host-available);
// `raster` is os-lite only, matching its moved symbols.
#[allow(unused_imports)]
use cursor::*;
#[cfg(all(feature = "os-lite", target_os = "none", not(feature = "virgl")))]
#[allow(unused_imports)]
use raster::*;

// External-surface re-export: `crate::backend::blend_pixel_vmo` is used by
// `cpu_vector.rs`; keep it resolving after the move into `raster`.
#[cfg(all(feature = "os-lite", target_os = "none", not(feature = "virgl")))]
pub(crate) use raster::blend_pixel_vmo;

/// Wraps a virtio-gpu MMIO device and implements GfxBackend.
/// On real hardware, this would be replaced by a different GfxBackend impl
/// (e.g., MaliGpuBackend, ImaginationGpuBackend) — same trait, different hardware.
pub struct VirtioGpuBackend {
    /// The GPU's device capability slot: what its queues and backings are made
    /// for and whose bus addresses they are programmed with (RFC-0098 C4,
    /// TASK-0246 P1).
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    device: u32,
    mmio_base: usize,
    _mmio_len: usize,
    next_resource_id: u32,
    probed: bool,
    /// The scanout's VISIBLE mode, decided once at probe (`display_mode`, RFC-0098 C7) and
    /// granted to windowd with the framebuffer; the RESOURCE layout stays at the maximum.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) display_w: u32,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) display_h: u32,
    /// The shared framebuffer resource: gpud's own, made for its device at the first grant
    /// (`crate::framebuffer_grant`), kept for gpud's lifetime.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) framebuffer_vmo: Option<u32>,
    /// True when virgl GPU acceleration is detected at probe time.
    /// Requires `virgl` feature + QEMU `-device virtio-gpu-pci,virgl=on`.
    #[allow(dead_code)]
    pub(crate) virgl_capable: bool,
    /// The device offered VIRTIO_GPU_F_VIRGL (it is a `virtio-gpu-gl`): read
    /// in BOTH builds at probe, independent of what the driver acked. Feeds
    /// `scanout_policy` — a GL device never gets the 2D plane-row scanout.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) gl_device: bool,
    /// Virgl rendering context ID (0 = not created).
    #[allow(dead_code)]
    pub(crate) virgl_ctx_id: u32,
    resources: alloc::vec::Vec<ResourceRecord>,
    pub(crate) scanout_resource: Option<ResourceId>,
    /// What the CPU executor reads besides the framebuffer: windowd's fragment uniforms and the
    /// software cursor (sprite, hotspot, shape cache) — `cpu_frame`.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) cpu: cpu_frame::CpuFrame,
    /// Real icon sprite (premultiplied BGRA), windowd-rendered from SVG and
    /// uploaded once; composited as a GPU sprite layer at
    /// (`icon_dst_x`,`icon_dst_y`) in the virgl buildup (the cursor's layer
    /// plumbing reused). Empty until uploaded.
    pub(crate) icon_sprite: alloc::vec::Vec<u8>,
    pub(crate) icon_sprite_w: u32,
    pub(crate) icon_sprite_h: u32,
    pub(crate) icon_dst_x: u32,
    pub(crate) icon_dst_y: u32,
    /// On-screen size (logical px) the icon is composited at. May be smaller than
    /// the sprite (rendered at 2× → supersampled, GPU-downscaled when composited).
    pub(crate) icon_dst_w: u32,
    pub(crate) icon_dst_h: u32,
    /// Hardware cursor resource (64×64, cursor queue). `None` until
    /// `upload_cursor` arms the overlay. Unused on backends whose overlay is
    /// not in the captured scanout — there the software cursor is live.
    cursor_resource_id: Option<ResourceId>,
    /// The pointer position windowd last sent (`OP_MOVE_CURSOR`): where the GL scanout's build-up
    /// draws the cursor sprite each present.
    pub(crate) cursor_ox: i32,
    pub(crate) cursor_oy: i32,
    /// When set, the control-queue submit helpers ENQUEUE (no per-command wait)
    /// instead of submit-and-drain. A present sets it, enqueues all its SUBMIT_3D
    /// draws + the flush, then drains once — so a textured draw whose completion
    /// QEMU defers no longer blocks the next command. Inert (false) on every other
    /// path, so mmio/init keep the exact synchronous behaviour.
    #[allow(dead_code)]
    ctrl_batch: bool,
    /// True while the early 2D bootstrap text scanout is what's on screen — the
    /// window where the boot-splash pulse breathes the title line. Set by
    /// `attach_bootstrap_text_scanout`, cleared when windowd's framebuffer
    /// handoff replaces the bootstrap scanout.
    #[allow(dead_code)]
    bootstrap_splash_live: bool,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    ctrlq: Option<CtrlQueue>,
    /// virtio-gpu cursor virtqueue (index 1) — carries UPDATE_CURSOR / MOVE_CURSOR
    /// so the host composites the pointer as a hardware overlay (the GPU hot path).
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    cursorq: Option<CtrlQueue>,
    /// True after the boot draw self-test verified a full GPU draw by readback
    /// (or the first scanout proved it, TASK-0326). Gates every RT draw.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_draw_ok: bool,
    /// The pipeline library's pipelines in the virgl context (`virgl_pipelines`).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) pipelines: crate::virgl_pipelines::Pipelines,
    /// Atlas texture (rows 3200..6399) aliased as a GPU sampler view for layer content.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) virgl_atlas_ready: bool,
    /// Cursor sprite uploaded into its own GL sampler texture so the cursor can be
    /// composited as a proper layer (`layer_pass`) instead of a procedural
    /// rect. Backing VA + dimensions latched at the first upload.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) cursor_tex_va: usize,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) cursor_tex_w: u32,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) cursor_tex_h: u32,
    /// Icon sprite uploaded into its own GL sampler texture (same scheme as the
    /// cursor). Backing VA + dims latched at the first `icon_tex_init`.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) icon_tex_va: usize,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) icon_tex_w: u32,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) icon_tex_h: u32,
    /// The frame planner's state (TASK-0215 5c-3 P4): the retained layer set the GL scanout
    /// composes every frame, the windows' transforms, the glass cache's keys.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) frame: nexus_gfx::frame::Frame,
    /// One-shot marker: first destination-so-far backdrop snapshot+blur submitted.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) rt_backdrop_marker_done: bool,
    /// First GPU layer composited (marker bookkeeping).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_layer_marker_done: bool,
    /// One-shot latch: a scroll band was clamped to the GL atlas texture height
    /// (logged once so a mis-sized band is diagnosable without flooding the UART).
    /// Gated to match the `virgl_composite` module (`feature = "virgl"`) that
    /// reads it in `composite_layer_rt` (a top-level module — needs `pub(crate)`).
    #[cfg(feature = "virgl")]
    pub(crate) scroll_band_clamp_logged: bool,
    /// True once the GL scanout RT owns the display (gl_scanout module, G0).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_scanout_active: bool,
    /// One-shot GL present parity readback done (gl_scanout module, G1).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_present_parity_done: bool,
    /// Double-buffered scanout swapchain state (see `gl_scanout::GlSwapState`).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) gl_swap: crate::gl_scanout::GlSwapState,
    /// SUBMIT_3D coalescing buffer (batch mode): consecutive 3D command
    /// streams append here and go out as ONE ring entry (present-cost fix —
    /// each entry costs a QEMU main-loop round trip). Flushed before any
    /// non-3D command and at `ctrl_batch_end`. Reused, no per-frame alloc.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) submit3d_pend: alloc::vec::Vec<u8>,
    /// virgl ctx of the pending coalesced stream (streams from different
    /// contexts never merge; in practice gpud uses one ctx).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) submit3d_pend_ctx: u32,
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
    /// `OP_WALLPAPER_DIRTY`: windowd rewrote Plane 0 (theme-matched wallpaper
    /// swap) — the next buildup present re-uploads the wallpaper texture from
    /// the VMO instead of keeping the one-shot reveal upload forever.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) wallpaper_reupload_pending: bool,
    /// RFC-0093 §5: windowd sent `OP_REVEAL` — wallpaper written, cursor uploaded, first
    /// frame presented. The splash is held until this is set; the first present after it
    /// uploads the wallpaper from Plane 0 and is acked `STATUS_REVEALED`. A handshake, not a
    /// probe: no pixel sampling, no fallback timer, no hard cap. Read by the request loop
    /// and the GL scanout — both OS-only — so it carries their gate.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) reveal_requested: bool,
    /// The ring's deadline-expiry count when the current present began (`Display::present_lost`).
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) deadline_expiries_at_present: u32,
    /// One-shot: the frame clock's hold tick was logged (`gpud: hold tick alive`).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) hold_tick_logged: bool,
    /// The last 2D bootstrap-splash pulse (~30 Hz redraw of the title band).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) last_splash_pulse_ns: u64,
}

impl VirtioGpuBackend {
    /// True while the boot splash is still held: the GL scanout owns the display but the
    /// desktop has NOT been revealed yet (wallpaper + cursor not both ready). gpud
    /// self-ticks presents in this window so the atomic reveal fires the instant the
    /// desktop is ready, instead of blocking on windowd — whose present loop stalls after
    /// its first frame. Always `false` outside the virgl GL-scanout path (only *called*
    /// on the virgl path, hence unused elsewhere).
    #[allow(dead_code)]
    pub(crate) fn is_holding_boot_splash(&self) -> bool {
        #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
        {
            self.gl_scanout_active && !self.wallpaper_from_vmo_uploaded
        }
        #[cfg(not(all(feature = "virgl", feature = "os-lite", target_os = "none")))]
        {
            false
        }
    }

    /// True while the early 2D bootstrap text scanout is on screen — the phase
    /// the boot-splash pulse animates before the GL scanout takes over.
    #[allow(dead_code)]
    pub(crate) fn bootstrap_splash_active(&self) -> bool {
        self.bootstrap_splash_live
    }

    /// Create a new backend for the device behind the capability in `device`,
    /// its registers mapped at `mmio_base`. Does NOT probe — call probe() separately.
    pub fn new(device: u32, mmio_base: usize, mmio_len: usize) -> Self {
        // Off the OS a backend makes no DMA memory: nothing keeps the device.
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let _ = device;
        Self {
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            device,
            mmio_base,
            _mmio_len: mmio_len,
            next_resource_id: 1,
            probed: false,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            display_w: nexus_display_proto::LAYOUT_MAX.0,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            display_h: nexus_display_proto::LAYOUT_MAX.1,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            framebuffer_vmo: None,
            virgl_capable: false,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            gl_device: false,
            virgl_ctx_id: 0,
            resources: alloc::vec::Vec::new(),
            scanout_resource: None,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            cpu: cpu_frame::CpuFrame::new(),
            icon_sprite: alloc::vec::Vec::new(),
            icon_sprite_w: 0,
            icon_sprite_h: 0,
            icon_dst_x: 0,
            icon_dst_y: 0,
            icon_dst_w: 0,
            icon_dst_h: 0,
            cursor_resource_id: None,
            cursor_ox: 0,
            cursor_oy: 0,
            ctrl_batch: false,
            bootstrap_splash_live: false,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            ctrlq: None,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            cursorq: None,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_draw_ok: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            pipelines: Default::default(),
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            cursor_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            cursor_tex_w: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            cursor_tex_h: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            icon_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            icon_tex_w: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            icon_tex_h: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_atlas_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            frame: nexus_gfx::frame::Frame::new(glass_blur::BLUR_CACHE_TEX_H),
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            rt_backdrop_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_layer_marker_done: false,
            #[cfg(feature = "virgl")]
            scroll_band_clamp_logged: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_scanout_active: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_present_parity_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_swap: Default::default(),
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            submit3d_pend: alloc::vec::Vec::new(),
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            submit3d_pend_ctx: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            gl_wallpaper_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            wallpaper_from_vmo_uploaded: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            wallpaper_reupload_pending: false,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            reveal_requested: false,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            deadline_expiries_at_present: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            hold_tick_logged: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            last_splash_pulse_ns: 0,
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

        // Capability proof (virgl self-test cascade or CPU fallback), then the
        // scanout policy: a GL device gets the GL render target or nothing.
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        {
            self.probe_gpu_capabilities();
            self.enforce_scanout_policy()?;
        }

        // Decide the VISIBLE mode — gpud is the authority (RFC-0098 C7): the lane's request
        // from the tree wins (race-free); GET_DISPLAY_INFO is validated CAPABILITY (QEMU's GTK
        // backend transiently reports the un-realized window default → never the source alone).
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        {
            let device = if self.ctrlq.is_some() { self.ctrl_query_display_info() } else { None };
            if let Some((w, h)) = device {
                display_mode::emit_display_info_marker(w, h); // diagnostic: advertised capability
            }
            let (rw, rh) = display_mode::resolve(display_mode::request_from_tree(), device);
            self.display_w = rw;
            self.display_h = rh;
        }

        Ok(())
    }

    pub fn is_probed(&self) -> bool {
        self.probed
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
        let (backing_va, backing_len, backing_vmo) =
            self.create_resource_os(id, w, h, fmt, _byte_len)?;
        self.resources.push(ResourceRecord {
            id,
            width: w,
            height: h,
            format: fmt,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_va,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            dma_vmo: backing_vmo,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_len,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_vmo,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_map_len: crate::backend::transport::align_page(_byte_len),
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
