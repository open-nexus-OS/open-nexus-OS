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

#[cfg(all(feature = "os-lite", target_os = "none"))]
mod bootstrap;
mod cursor;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod lifecycle;
mod present;
mod raster;
mod resources;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod transport;
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
mod virgl3d;
#[cfg(all(feature = "os-lite", target_os = "none"))]
mod virtqueue;

// The validation + error-mapping helpers live in `resources`; the GfxBackend
// trait impl below resolves them by bare name.
use resources::{map_nexus_error, resource_byte_len, validate_rect};

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
pub(crate) use bootstrap::{compose_splash_region, splash_pulse_q8};

// Bring the moved free-function clusters into the parent namespace so the impl
// blocks still in this file resolve them by bare name (zero call-site churn).
// `cursor` is ungated (its CURSOR_FALLBACK_* consts are host-available);
// `raster` is os-lite only, matching its moved symbols.
#[allow(unused_imports)]
use cursor::*;
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[allow(unused_imports)]
use raster::*;

// External-surface re-export: `crate::backend::blend_pixel_vmo` is used by
// `cpu_vector.rs`; keep it resolving after the move into `raster`.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) use raster::blend_pixel_vmo;

/// One cached cursor shape: `(premultiplied BGRA, w, h, hot_x, hot_y)`.
pub(crate) type CursorShapeEntry = (alloc::vec::Vec<u8>, u32, u32, u32, u32);

/// Wraps a virtio-gpu MMIO device and implements GfxBackend.
/// On real hardware, this would be replaced by a different GfxBackend impl
/// (e.g., MaliGpuBackend, ImaginationGpuBackend) — same trait, different hardware.
pub struct VirtioGpuBackend {
    mmio_base: usize,
    _mmio_len: usize,
    next_resource_id: u32,
    probed: bool,
    /// The scanout's VISIBLE mode, resolved once at probe from the device's
    /// `GET_DISPLAY_INFO` (QEMU: `xres=`/`yres=`), falling back to 1280×800.
    /// The RESOURCE layout (plane rows, strides, atlas budget) stays at the
    /// fixed maximum — this is the visible sub-rect the compositor targets.
    /// (Read by the os-lite service/present paths and the virgl scanout; the
    /// host slice only constructs the backend, hence the scoped allow.)
    #[cfg_attr(not(all(feature = "os-lite", target_os = "none")), allow(dead_code))]
    pub(crate) display_w: u32,
    #[cfg_attr(not(all(feature = "os-lite", target_os = "none")), allow(dead_code))]
    pub(crate) display_h: u32,
    /// True when virgl GPU acceleration is detected at probe time.
    /// Requires `virgl` feature + QEMU `-device virtio-gpu-pci,virgl=on`.
    #[allow(dead_code)]
    pub(crate) virgl_capable: bool,
    /// Virgl rendering context ID (0 = not created).
    #[allow(dead_code)]
    pub(crate) virgl_ctx_id: u32,
    resources: alloc::vec::Vec<ResourceRecord>,
    /// Monotonic GPU-resource VA-window slot (task #124): VA slots are never
    /// reused — a released resource's pages stay mapped (no unmap primitive),
    /// so its window must not be handed to a new resource.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    next_resource_va_index: usize,
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
    /// Cursor shape cache (OP_UPLOAD_CURSOR_SHAPE): pre-uploaded sprites so a
    /// pointer shape change is a 2-byte OP_SELECT_CURSOR_SHAPE instead of a
    /// blocking 4KB re-upload per window-edge crossing. Slot = shape id;
    /// entry = (premultiplied BGRA, w, h, hot_x, hot_y).
    pub(crate) cursor_shape_cache:
        [Option<CursorShapeEntry>; nexus_display_proto::CURSOR_SHAPE_SLOTS],
    /// Real icon sprite (premultiplied BGRA), rendered by windowd from an SVG via
    /// the nexus-svg HiDPI pipeline and uploaded once. Composited as a GPU sprite
    /// layer at (`icon_dst_x`,`icon_dst_y`) in the virgl buildup — the production
    /// "real icon on the GPU compositor" path, reusing the cursor's layer plumbing.
    /// Empty until uploaded.
    pub(crate) icon_sprite: alloc::vec::Vec<u8>,
    pub(crate) icon_sprite_w: u32,
    pub(crate) icon_sprite_h: u32,
    pub(crate) icon_dst_x: u32,
    pub(crate) icon_dst_y: u32,
    /// On-screen size (logical px) the icon is composited at. May be smaller than
    /// the sprite (rendered at 2× → supersampled, GPU-downscaled when composited).
    pub(crate) icon_dst_w: u32,
    pub(crate) icon_dst_h: u32,
    /// Hardware cursor resource (64×64, cursor queue). `None` until a
    /// successful `upload_cursor` arms the overlay. Unused on display backends
    /// where the overlay is not composited into the captured/shown scanout —
    /// there the save-under software cursor below is the live path.
    cursor_resource_id: Option<ResourceId>,
    pub(crate) cursor_hot: (u32, u32),
    /// Save-under software cursor (composited into the scanout, so it is visible
    /// on every display backend). `cursor_ox/oy` are the screen-space top-left of
    /// the drawn sprite; `cursor_saveunder` holds the scene pixels it covers.
    cursor_owned: bool,
    // The paint/unpaint/suspend cursor paths live in os-lite-only methods
    // (backend/cursor.rs); host builds only initialize these fields.
    #[cfg_attr(not(all(feature = "os-lite", target_os = "none")), allow(dead_code))]
    cursor_drawn: bool,
    #[cfg_attr(not(all(feature = "os-lite", target_os = "none")), allow(dead_code))]
    cursor_suspended: bool,
    pub(crate) cursor_ox: i32,
    pub(crate) cursor_oy: i32,
    #[cfg_attr(not(all(feature = "os-lite", target_os = "none")), allow(dead_code))]
    cursor_dw: u32,
    #[cfg_attr(not(all(feature = "os-lite", target_os = "none")), allow(dead_code))]
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
    /// Icon sprite uploaded into its own GL sampler texture (same scheme as the
    /// cursor). Backing VA + dims latched at the first `icon_tex_init`.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) icon_tex_va: usize,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) icon_tex_w: u32,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) icon_tex_h: u32,
    /// Backdrop scratch texture (destination-so-far glass blur) created — set by
    /// `backdrop_tex_init`, read by `blur_rt_backdrop` to pick its blur source.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) backdrop_tex_ready: bool,
    /// One-shot marker: first destination-so-far backdrop snapshot+blur submitted.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) rt_backdrop_marker_done: bool,
    /// One-shot marker: first `OP_SET_LAYER_SCROLL` applied (scroll fast path live).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) layer_scroll_marker_done: bool,
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) layer_transform_marker_done: bool,
    /// First GPU layer composited (marker bookkeeping).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    virgl_layer_marker_done: bool,
    /// One-shot latch: a scroll band was clamped to the GL atlas texture height
    /// (logged once so a mis-sized band is diagnosable without flooding the UART).
    /// Gated to match the `virgl_composite` module (`feature = "virgl"`) that
    /// reads it in `composite_layer_rt` (a top-level module — needs `pub(crate)`).
    #[cfg(feature = "virgl")]
    pub(crate) scroll_band_clamp_logged: bool,
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
    /// `OP_WALLPAPER_DIRTY`: windowd rewrote Plane 0 (theme-matched wallpaper
    /// swap) — the next buildup present re-uploads the wallpaper texture from
    /// the VMO instead of keeping the one-shot reveal upload forever.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) wallpaper_reupload_pending: bool,
    /// Atomic boot reveal: guest-time (ns) of the first buildup present — the origin for
    /// the reveal fallback timers. The logo splash is held until the desktop is composable
    /// (wallpaper + cursor), but a hard cap from this origin guarantees it is NEVER held
    /// forever if a signal is slow/absent. 0 = not yet latched.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) reveal_content_since_ns: u64,
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
    /// Scroll fast path: per-`scroll_id` source-row overrides. A retained layer
    /// with a non-zero `scroll_id` is re-sampled at its override row (when set)
    /// instead of its stored `src_row_abs`. Set by `OP_SET_LAYER_SCROLL` (a 54µs
    /// GPU re-composite, no CPU re-render), cleared when a full present brings a
    /// fresh authoritative layer set. Index = `scroll_id - 1`.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) scroll_src_rows: [Option<u32>; MAX_SCROLL_IDS],
    /// Transform fast path (Track C2, the scroll generalization): per-layer-id
    /// translate/opacity/scale overrides. A retained layer whose id has an
    /// override composites transformed — no re-render, no re-upload. Set by
    /// `OP_SET_LAYER_TRANSFORM` (record + coalesced flush, the scroll
    /// contract), cleared when a full present brings a fresh authoritative
    /// layer set (windowd bakes the transform into the encoded layer).
    /// Index = `layer_id - 1` (same id domain as `scroll_id`: slot + 1).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) layer_transforms: [Option<LayerTransform>; MAX_SCROLL_IDS],
    /// Build-up only: the retained layer set's atlas content changed, so the next
    /// composite must re-upload it to the GL texture (`virgl_transfer_to_host`).
    /// Cleared after upload — cursor-move presents then re-composite from the
    /// already-uploaded texture WITHOUT the per-frame transfer (the slow path).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    rt_layers_dirty: bool,
}

/// One recorded layer-transform override (Track C2): identity = absent.
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
#[derive(Clone, Copy)]
pub(crate) struct LayerTransform {
    pub(crate) dx: i16,
    pub(crate) dy: i16,
    /// Multiplies the layer's encoded opacity (255 = unchanged).
    pub(crate) opacity: u8,
    /// Uniform scale about the layer center, percent (100 = unchanged).
    pub(crate) scale_pct: u16,
}

/// A CompositeLayer op deferred for RT-direct compositing after the base upload.
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
#[derive(Clone, Copy, Default)]
struct PendingRtLayer {
    src_row_abs: u32,
    src_x: u32,
    width: u32,
    height: u32,
    /// Content sub-size drawn at the layer's top-left (`0` = same as
    /// `width`/`height`). Backdrop blur covers `width`×`height` (the frame); the
    /// content texture is drawn at `content_w`×`content_h` (the band) — the
    /// "glass frame grows, content 1:1" resize path.
    content_w: u32,
    content_h: u32,
    dst_x: u32,
    dst_y: u32,
    opacity: u32,
    corner_radius: u32,
    shadow_blur: u32,
    shadow_offset_y: i32,
    shadow_alpha: u32,
    /// Frosted-glass backdrop blur radius (0 = opaque/no glass). When > 0 the
    /// build-up blurs the wallpaper behind this layer's rect before compositing.
    backdrop_blur: u32,
    /// Scroll identity (0 = not scrollable). Non-zero: gpud re-samples the layer
    /// at the id's `scroll_src_rows` override on the lightweight scroll fast path.
    scroll_id: u32,
    /// WebRender scroll band (0 = not scrollable): the FULL resident-content band
    /// `[scroll_band_top_abs, +scroll_band_h)` uploaded to the GL atlas texture
    /// ONCE (on a full present) so the `src_row` override can shift within it. The
    /// SAMPLE still uses `src_row_abs` + `height` (the overridden scroll row).
    scroll_band_top_abs: u32,
    scroll_band_h: u32,
    /// Transform identity (`0` = none) - keys `layer_transforms` (Track C2).
    /// DISTINCT from `scroll_id`: all slices of a window share one transform
    /// id, only the scrolling body carries the scroll id.
    layer_id: u32,
}

#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
// Sized for a REAL desktop: 4 app windows already need 16+ composites (a
// scrollable window is 3 band slices + a title overlay; plus per-app glass
// regions, dock, top bar). The old cap of 8 SILENTLY dropped the overflow —
// windows/chrome vanished with 4 apps open. ~68 B/entry, static array.
const MAX_PENDING_RT_LAYERS: usize = 32;
/// Scroll-id table capacity (ids 1..=MAX_SCROLL_IDS; 0 = not scrollable).
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
pub(crate) const MAX_SCROLL_IDS: usize = 8;

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

    /// Create a new backend. Does NOT probe — call probe() separately.
    pub fn new(mmio_base: usize, mmio_len: usize) -> Self {
        Self {
            mmio_base,
            _mmio_len: mmio_len,
            next_resource_id: 1,
            probed: false,
            display_w: 1280,
            display_h: 800,
            virgl_capable: false,
            virgl_ctx_id: 0,
            resources: alloc::vec::Vec::new(),
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            next_resource_va_index: 0,
            scanout_resource: None,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            fragment_data: [0u8; 64],
            cursor_sprite: alloc::vec::Vec::new(),
            cursor_sprite_w: 0,
            cursor_sprite_h: 0,
            cursor_shape_cache: [const { None }; nexus_display_proto::CURSOR_SHAPE_SLOTS],
            icon_sprite: alloc::vec::Vec::new(),
            icon_sprite_w: 0,
            icon_sprite_h: 0,
            icon_dst_x: 0,
            icon_dst_y: 0,
            icon_dst_w: 0,
            icon_dst_h: 0,
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
            bootstrap_splash_live: false,
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
            icon_tex_va: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            icon_tex_w: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            icon_tex_h: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_atlas_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            backdrop_tex_ready: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            rt_backdrop_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            layer_scroll_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            layer_transform_marker_done: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            virgl_layer_marker_done: false,
            #[cfg(feature = "virgl")]
            scroll_band_clamp_logged: false,
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
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            wallpaper_reupload_pending: false,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            reveal_content_since_ns: 0,
            // RT-direct layer compositing on by default for the virgl path; the
            // field is the kill-switch if a regression shows up in the thumbnail.
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            rt_direct_layers: true,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            pending_rt_layers: [PendingRtLayer::default(); MAX_PENDING_RT_LAYERS],
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            layer_transforms: [None; MAX_SCROLL_IDS],
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            pending_rt_count: 0,
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            scroll_src_rows: [None; MAX_SCROLL_IDS],
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            rt_layers_dirty: true,
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
                            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_NOOP);
                        }
                        Ok(_) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_MISMATCH);
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
                            let _ = nexus_abi::debug_println("gpud: virgl gradient submit fail");
                        }
                    }
                    // G2: GPU layer compositor primitive proof (textured layer +
                    // rounded mask + opacity composited into an RT, readback).
                    match self.virgl_composite_selftest() {
                        Ok(true) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_OK);
                        }
                        Ok(false) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_OFF);
                        }
                        Err(_) => {
                            let _ = nexus_abi::debug_println("gpud: virgl composite submit fail");
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

        // Resolve the scanout's VISIBLE mode from the device
        // (GET_DISPLAY_INFO → pmodes[0], QEMU's `xres=`/`yres=`). The
        // RESOURCE layout stays at the fixed maximum budget; consumers size
        // the visible sub-rect from this. Fallback = the 1280×800 defaults.
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        if self.ctrlq.is_some() {
            if let Some((w, h)) = self.ctrl_query_display_info() {
                // Clamp to the fixed resource budget (planes are laid out
                // for at most 1280×800 — larger modes need the atlas/VMO
                // budget work first).
                self.display_w = w.min(1280);
                self.display_h = h.min(800);
                emit_display_info_marker(self.display_w, self.display_h);
            } else {
                let _ = nexus_abi::trace_line("gpud: display info query fail (1280x800)");
            }
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

/// `gpud: display info WxH` — the resolved visible mode (alloc-free: gpud's
/// stack-buffer marker pattern, the heap never sees boot markers).
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn emit_display_info_marker(w: u32, h: u32) {
    fn put(buf: &mut [u8; 40], p: &mut usize, s: &[u8]) {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    }
    fn put_dec(buf: &mut [u8; 40], p: &mut usize, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut n = 0;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            put(buf, p, &tmp[n..=n]);
        }
    }
    let mut buf = [0u8; 40];
    let mut p = 0usize;
    put(&mut buf, &mut p, b"gpud: display info ");
    put_dec(&mut buf, &mut p, w);
    put(&mut buf, &mut p, b"x");
    put_dec(&mut buf, &mut p, h);
    let _ = nexus_abi::trace_line(core::str::from_utf8(&buf[..p]).unwrap_or("gpud: display info"));
}
