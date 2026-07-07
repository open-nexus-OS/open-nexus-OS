// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Display server runtime state machine for the windowd compositor:
//! retained-mode compositing, tile damage tracking, input routing, cursor management,
//! and present scheduling.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 13 unit tests (QEMU) + host smoke integration

use super::blur::checked_stride;
use super::cache::{BackdropCacheEntry, GlassLayerCache, LayerCache, PathCacheEntry};
use super::damage::cursor_damage_rect;
use super::emit_windowd_telemetry;
use super::filter::filter_layout_variant_index;
use super::primitives::draw_line_segment_row;
use super::scene::copy_scene_row;
use super::source::build_scale_lut;
use super::tile_map::TileMap;
use super::types::{
    FixedDebugLine, ProofBoxRect, ProofCard, ProofPaintPart, ProofPaintRole, RenderClip,
    SourceFrame,
};
use super::{
    BACKDROP_CACHE_ENTRIES, BACKDROP_CACHE_MAX_WIDTH, BLUR_CACHE_ROW_OFFSET,
    BUTTON_BLUR_CACHE_ABS_ROW, BUTTON_BLUR_CACHE_ABS_X, CHAT_SHADOW_ALPHA, CHAT_SHADOW_BLUR,
    CHAT_SHADOW_OFFSET_Y, COMBINED_PANEL_WIDTH, DARK_GLASS_BLUR_RADIUS,
    DARK_GLASS_SATURATION_PERCENT, DISPLAY_HEIGHT, DISPLAY_OFFSET_BYTES, DISPLAY_ROW_OFFSET,
    DISPLAY_WIDTH, GLASS_LAYER_MAX_BYTES, IPC_BATCH_LIMIT, LAYER_CACHE_MAX_BYTES,
    LAYER_CACHE_MAX_LAYER_BYTES, LIVE_FILTER_VARIANTS, PATH_CACHE_ENTRIES, PATH_CACHE_MAX_PIXELS,
    PROOF_PANEL_H, RETAINED_OFFSET_BYTES, RETAINED_ROW_OFFSET, ROUTE_NAME, ROW_WRITE_CHUNK,
    SCENE_ORIGIN_X, SCENE_ORIGIN_Y, SIDEBAR_REST_X, USE_DESKTOP_SHELL,
};
use crate::error::WindowdError;
use crate::ids::CallerCtx;
use crate::live_runtime::{
    premerge_damage_rects, select_glass_quality, DamageRect, GlassQuality, LayoutHotPathIndex,
    TargetDamage,
};
use crate::markers::*;
use crate::smoke::VisibleBootstrapMode;
use crate::systemui_shell::{DeviceProfile, SystemUiShell};
use crate::telemetry::WindowdDisplayTelemetryReport;
use alloc::vec::Vec;
use animation::{AnimProp, AnimationDriver, LayerId, ScrollConfig, ScrollMomentum, SceneUpdate};
use core::fmt::Write as _;
use input_live_protocol::{VisibleState, STATUS_MALFORMED, STATUS_OK};
use nexus_abi::{cap_clone, debug_println, debug_trace, nsec, vmo_write, Handle};
use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::{
    BackdropCache, CommandBuffer, Layer, LayerBackdrop, LayerShadow, PipelineTimer, RenderPassDesc,
    TileRect,
};
use nexus_ipc::{Client as _, KernelClient, Wait};
use nexus_layout::LayoutResult;
use nexus_layout_types::{FxPx, PathPoint};
use chat_app::ChatMessageProvider;
use nexus_virtual_list::{VirtualList, VirtualListConfig};

// Gate 2: the windowd↔gpud wire is the shared SSOT in `nexus-display-proto`;
// these local names just re-source its values (no more hand-mirroring gpud).
const GPU_ANIMATION_SUBMIT_OP: u8 = nexus_display_proto::OP_SUBMIT_ANIMATION_FRAME;
const GPU_SET_FRAMEBUFFER_VMO_OP: u8 = nexus_display_proto::OP_SET_FRAMEBUFFER_VMO;
const GPU_PRESENT_DAMAGE_OP: u8 = nexus_display_proto::OP_PRESENT_DAMAGE;
const GPU_MOVE_CURSOR_OP: u8 = nexus_display_proto::OP_MOVE_CURSOR;
const GPU_UPLOAD_CURSOR_OP: u8 = nexus_display_proto::OP_UPLOAD_CURSOR;
const GPU_SET_LAYER_SCROLL_OP: u8 = nexus_display_proto::OP_SET_LAYER_SCROLL;
/// Scroll identity of the chat body layer (ids are windowd-assigned; 0 = none).
pub(crate) const CHAT_SCROLL_ID: u32 = 1;
const GPU_UPLOAD_ICON_OP: u8 = nexus_display_proto::OP_UPLOAD_ICON;
const GPUD_STATUS_OK: u8 = nexus_display_proto::STATUS_OK;
/// Extra chat content rows rendered above/below the on-screen viewport so scroll
/// is a GPU composite offset, not a CPU re-render. Re-render only on overscan
/// exhaustion (recenter ±CHAT_OVERSCAN/2). Larger ⇒ fewer full-surface re-renders
/// during a fast flick (less VMO-write load → less heap pressure) AND more
/// rendered runway in BOTH directions (smoother up-scroll, which crosses the
/// window most). Bounded so the atlas (chat 600+this, blur 600, sidebar 800) fits
/// the 4000-row VMO atlas (grown +800 for the full-screen greeter/lock-overlay
/// band): 1600+600+800 = 3000, + greeter 800 = 3800 ≤ 4000.
const CHAT_OVERSCAN: u32 = 1000;
const GPUD_FALLBACK_SEND_SLOT: u32 = 5;
const GPUD_FALLBACK_RECV_SLOT: u32 = 6;
const FIRST_HANDOFF_DEADLINE_NS: u64 = 1_000_000_000;
use crate::systemui_shell::{CLICK_LAYER_ID, HOVER_LAYER_ID, KEYBOARD_LAYER_ID, SIDEBAR_LAYER_ID};

/// Animation layer for the topbar Apps dropdown reveal (not in the shell's
/// scene-graph layer map; handled directly in `apply_scene_updates`).
const DROPDOWN_LAYER_ID: LayerId = LayerId(70);
// Interactive geometry lives in `interaction` — the single source of truth shared
// by the live renderer and the hit-tester (hit area == rendered rect).
use crate::interaction::{
    GLASS_BUTTON_H, GLASS_BUTTON_RADIUS, GLASS_BUTTON_RIGHT, GLASS_BUTTON_TOP, GLASS_BUTTON_W,
    LUCIDE_ICON_SIZE, SIDEBAR_MARGIN_BOTTOM, SIDEBAR_MARGIN_TOP, SIDEBAR_RADIUS, SIDEBAR_WIDTH,
};
const GLASS_OVERLAY_MAX_BYTES: usize = SIDEBAR_WIDTH as usize * 4;
const ANIMATION_UPDATE_CAP: usize = 8;

// Topical submodules holding `impl DisplayServerRuntime` blocks split out of this
// file (TASK-0063 modularization). Child modules of `runtime` so they retain
// access to the struct's private fields without weakening encapsulation.
mod anim;
mod cursor;
mod gpud;
mod marker_emit;
mod framebuffer;
mod input;
mod chat_window;
mod shell;
mod search;
mod dsl_mount;
pub(crate) mod app_window;
mod settings_window;
mod present;
mod scene;
mod session;
mod greeter;
mod wm;

// The split-out `impl` submodules live one module deeper than the original
// `runtime/mod.rs`, so the compositor-level siblings + consts they reference via
// `super::` are re-exported here under `runtime` to keep those paths resolving
// (TASK-0063 modularization; pure path plumbing, no behavior change).
use super::chat;
use super::desktop_layer;
use super::DISPLAY_SLOT_B_OFFSET_BYTES;

fn log_gpud_ipc_error(prefix: &str, err: nexus_ipc::IpcError) {
    let label = match err {
        nexus_ipc::IpcError::WouldBlock => "would-block",
        nexus_ipc::IpcError::Timeout => "timeout",
        nexus_ipc::IpcError::Disconnected => "disconnected",
        nexus_ipc::IpcError::NoSpace => "no-space",
        nexus_ipc::IpcError::Unsupported => "unsupported",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint) => "kernel-no-endpoint",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::QueueFull) => "kernel-queue-full",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::QueueEmpty) => "kernel-queue-empty",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied) => {
            "kernel-permission-denied"
        }
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::TimedOut) => "kernel-timeout",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSpace) => "kernel-no-space",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::Unsupported) => "kernel-unsupported",
        _ => "other",
    };
    let _ = debug_println(&alloc::format!("{prefix} {label}"));
}

/// Like [`log_gpud_ipc_error`] but for the cap-sensitive gpud sends (the VMO
/// cap-move handoff + present). On a `kernel-permission-denied` — the classic
/// "the cap at this slot lacks SEND, or the send slot points at the wrong cap" —
/// it names the gpud SEND slot and the slot contract, so a future cap regression
/// (e.g. init displacing the gpud caps off slots 5/6) is diagnosable from one
/// boot line instead of a log dig. Other errors defer to the generic logger.
fn log_gpud_cap_error(prefix: &str, err: nexus_ipc::IpcError, send_slot: u32) {
    if matches!(
        err,
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied)
    ) {
        let _ = debug_println(&alloc::format!(
            "{prefix} kernel-permission-denied (gpud send_slot={send_slot}: cap lacks SEND or slot \
             points at the wrong cap — windowd→gpud handoff contract is slots \
             {GPUD_FALLBACK_SEND_SLOT}/{GPUD_FALLBACK_RECV_SLOT}; check init cap-transfer order \
             didn't displace them)"
        ));
    } else {
        log_gpud_ipc_error(prefix, err);
    }
}

fn encode_gpud_damage_frame(rect: DamageRect) -> [u8; 17] {
    nexus_display_proto::encode_damage_frame(rect.x, rect.y, rect.width, rect.height)
}

fn encode_gpud_attach_frame(handoff_id: u32) -> [u8; 5] {
    nexus_display_proto::encode_attach_frame(handoff_id)
}

fn decode_gpud_handoff_id(reply: &[u8]) -> Option<u32> {
    nexus_display_proto::decode_handoff_id(reply)
}

#[derive(Clone, Copy)]
struct AnimatedSceneState {
    hover_opacity: f32,
    sidebar_translate_x: f32,
    sidebar_opacity: f32,
    /// Topbar Apps dropdown reveal: 0 = closed, 1 = fully open.
    apps_dropdown_progress: f32,
}

impl AnimatedSceneState {
    const fn new() -> Self {
        Self {
            hover_opacity: 0.0,
            sidebar_translate_x: 320.0,
            sidebar_opacity: 0.0,
            apps_dropdown_progress: 0.0,
        }
    }
}

// RFC-0067 P5-Final G1: `draw_animation_proof_overlay_row` (the CPU "animation
// proof overlay" — button/sidebar/lucide-icons drawn via CPU glass rows) was
// dead on BOTH backends (G0 markers `cpu-sdf-fill`/`cpu-backdrop-blur` never fired
// on virgl or the CPU-fallback boot) and had zero callers. Deleted; the GPU
// command path (and gpud's cpu_vector on mmio) renders glass. Its now-orphaned
// helpers (draw_floating_glass_rect_row, blend_span, the lucide-icon rows) are
// removed in the cascade below.

// (orphaned CPU glass-overlay helpers — draw_floating_glass_rect_row, the
// lucide-icon rows, blend_span/blend_pixel — removed with their dead caller
// `draw_animation_proof_overlay_row`; RFC-0067 P5-Final G1.)

#[derive(Default)]
struct AnimationProofState {
    runtime_marker: bool,
    timeline_marker: bool,
    implicit_marker: bool,
    batch_marker: bool,
    live_marker: bool,
    spring_marker: bool,
    v5_summary_marker: bool,
}

pub(crate) struct DisplayServerRuntime {
    mode: VisibleBootstrapMode,
    source_frame: SourceFrame,
    source_x_lut: Vec<u32>,
    source_y_lut: Vec<u32>,
    cursor_width: u32,
    cursor_height: u32,
    framebuffer: Option<Handle>,
    band_scratch: Vec<u8>,
    /// Temporary row buffer for horizontal blur (zero-copy — allocated once).
    blur_row_buf: Vec<u8>,
    state: VisibleState,
    observer_state: VisibleState,
    markers_emitted: bool,
    input_markers_emitted: InputMarkerState,
    input_state_debug_emitted: bool,
    pending_damage_rects: Vec<DamageRect>,
    tile_map: TileMap,
    layer_cache: LayerCache,
    /// True when pending damage only affects paint (no layout/shadow change needed).
    paint_only_damage: bool,
    pending_damage_rect: Option<DamageRect>,
    /// Cursor damage (old ∪ new pointer rect) for the next frame. Tracked
    /// separately from content damage: cursor rects only need a retained→display
    /// blit + cursor overlay (no CPU recomposite — Plane 1 is already cursor-free).
    pending_cursor_rect: Option<DamageRect>,
    /// Animation-driven frame: only GPU CB params changed (translate_x, opacity).
    /// Plane 1 is already current — no CPU recomposite needed. Merged rect passed
    /// to the GPU CB blit list so the display plane is refreshed from Plane 1.
    pending_gpu_blit_rect: Option<DamageRect>,
    /// True when the sidebar blur has been computed and cached in Plane 3 (Slot B).
    /// Invalidated after the close animation fully completes (opacity < 0.01).
    sidebar_blur_cache_valid: bool,
    /// True when the glass button blur is cached in Plane 3 at BUTTON_BLUR_CACHE_ABS_X/ROW.
    /// Never invalidated: button occupies y=24..80, above the proof panel at y=440.
    button_blur_cache_valid: bool,
    telemetry: crate::telemetry::WindowdDisplayTelemetry,
    backdrop_cache: [BackdropCacheEntry; BACKDROP_CACHE_ENTRIES],
    glass_layer: GlassLayerCache,
    glass_scratch: Vec<u8>,
    path_cache: [PathCacheEntry; PATH_CACHE_ENTRIES],
    /// Index into `LIVE_FILTER_VARIANTS` for the active filter text/layout.
    active_filter_idx: usize,
    /// Filter cycle counter for automated proof (advances on each keyboard event).
    filter_cycle: u8,
    /// Whether clipping marker was emitted.
    clipping_marker_emitted: bool,
    /// Whether scroll marker was emitted.
    scroll_marker_emitted: bool,
    /// Whether live scroll marker was emitted.
    live_scroll_marker_emitted: bool,
    /// Whether v3b selftest summary markers were emitted.
    selftest_v3b_emitted: bool,
    /// Whether flush_pending_damage has verified v3b composition (P1 fix: no longer fake).
    v3b_composition_verified: bool,
    /// Whether v3b markers were already emitted.
    v3b_markers_emitted: bool,
    /// Animation driver: spring physics, keyframes, reduced motion (RFC-0059).
    animation_driver: AnimationDriver,
    animated_scene: AnimatedSceneState,
    animation_proof: AnimationProofState,
    gpud_client: Option<KernelClient>,
    /// Pipeline performance timer with Soll-gate validation.
    pipeline_timer: PipelineTimer,
    /// Persistent per-frame command buffer. Reused (cleared, not dropped) every
    /// frame so the GPU-first present path records its ~15 commands without any
    /// heap allocation. windowd runs on a non-freeing bump allocator, so a fresh
    /// `CommandBuffer::new()` per frame would leak its `Vec<Command>` regrowth
    /// (~1.4KB/frame) and exhaust the 1MB heap after ~700 animation
    /// frames — the cause of the `alloc-fail svc=windowd` crash mid-animation.
    scene_cb: CommandBuffer,
    /// SystemUI shell with retained scene graph — the single rendering authority
    /// for all UI surfaces (Phase 0: GPU pipeline hardening).
    shell: SystemUiShell,
    /// Set when register_framebuffer_vmo creates the framebuffer VMO but
    /// after sending the response.
    framebuffer_pending_first_write: bool,
    /// Phase 6d: monotonic present sequence number for completion correlation.
    present_seq: u32,
    /// Phase 6d: count of frames submitted to gpud but not yet acknowledged.
    frames_in_flight: u32,
    /// Phase 6d: last present sequence number acknowledged by gpud.
    last_completed_seq: u32,
    /// Stall watchdog (Android-ANR / Linux hung-task style): timestamp of the last
    /// observed present *progress*, the seq it was at, and whether a stall was
    /// already reported for the current episode. If damage stays pending and the
    /// completed seq doesn't advance for `STALL_THRESHOLD_NS`, we log one
    /// diagnostic line to the UART (→ `build/logs/*/uart.log`) so a "scrolled and
    /// it stopped responding" freeze is self-reported with its state.
    stall_last_progress_ns: u64,
    stall_last_seq: u32,
    stall_reported: bool,
    /// Latch so a backpressured present logs its failure ONCE per episode instead
    /// of every retry (which would flood the UART at ~120 Hz during the very stall
    /// we want to read). Cleared on the next successful send.
    present_fail_reported: bool,
    /// Phase 4: active frame ring slot (0 = Plane 2 / slot A, 1 = Plane 3 / slot B).
    /// Toggled after each successful present. gpud scanout follows on swap.
    current_display_slot: u8,
    /// handoff ID for the initial framebuffer VMO transfer to gpud.
    first_handoff_id: u32,
    first_handoff_deadline_ns: u64,
    first_handoff_frame_written: bool,
    first_handoff_bootstrap_markers_emitted: bool,
    first_handoff_attach_acked: bool,
    first_handoff_present_sent: bool,
    /// True when gpud armed the virtio-gpu hardware cursor overlay. Pointer
    /// moves then ship as a 9-byte OP_MOVE_CURSOR (host repositions the
    /// overlay) — zero composite, zero blits, zero presents per move.
    hw_cursor_active: bool,
    /// True when gpud draws a procedural cursor on the virgl GL scanout (the
    /// build-up present owns the scanout, so the software BlendCursor in the VMO
    /// is ignored). Pointer moves ship OP_MOVE_CURSOR (updates gpud's pointer
    /// pos) AND damage the cursor rect so a present re-renders the procedural
    /// arrow at the new position.
    gl_cursor_active: bool,
    /// One-shot: pre-blur the sidebar backdrop into the Plane 3 cache during
    /// the first handoff present, so the first open never pays for a blur.
    precache_blur_pending: bool,
    /// Cursor over the top-right glass button (highlight only — windowd-internal,
    /// independent of the proof-panel hover test card in `state.hover_visible`).
    button_hover: bool,
    /// Chat window — the SECOND instance of the reusable `ShellWindow` glass
    /// frame (the first is `search`). Owns the chat window's geometry, drag/close
    /// state, visibility, cached blurred backdrop (`blur_valid`) and the
    /// off-screen atlas content surface (`atlas`) — rendered once on change and
    /// composited at the window's current position, so moving it is just a
    /// different blit destination (no content re-render). The scroll *physics*
    /// live in `chat_list` (the message provider + momentum) below; this frame
    /// supplies geometry + chrome + glass. E1 retired the old single-window
    /// `wm::WindowManager`: both windows are now `ShellWindow` instances whose
    /// pure geometry lives in the host-tested `window_frame::Frame`.
    chat: super::shell_window::ShellWindow,
    /// Cached fully-composited sidebar (valid only while it's settled/static).
    sidebar_composite_cache: crate::atlas::AtlasSurface,
    sidebar_composite_cache_valid: bool,
    /// Shell-P2b: the desktop shell scene as ONE opaque retained-surface LAYER
    /// (atlas surface). Rendered once into `shell_atlas` and composited onto the
    /// scanout via the GPU layer path every present (the path that reaches the
    /// virgl scanout; Plane 1 does not). `shell_w`/`shell_h` are the root rect.
    shell_atlas: crate::atlas::AtlasSurface,
    shell_w: u32,
    shell_h: u32,
    shell_surface_dirty: bool,
    /// Index of the topbar item currently under the cursor (drives the hover
    /// highlight; a change re-renders the topbar atlas).
    topbar_hover: Option<usize>,
    /// Whether the cursor is over the topbar menu (hamburger) icon.
    topbar_menu_hover: bool,
    /// Glass side panel layer (slides in from the right on the menu toggle).
    sidepanel_atlas: crate::atlas::AtlasSurface,
    sidepanel_h: u32,
    sidepanel_surface_dirty: bool,
    /// Which topbar item's dropdown menu is open (the item index: 0 = Apps,
    /// 2 = Edit), or `None` when closed. One dropdown surface serves whichever
    /// menu is active (`active_menu`) — the menu bar is one component, not a
    /// per-item parallel structure.
    open_topbar_menu: Option<usize>,
    /// The static "Edit" menu (one entry: Settings). Reuses the same `AppMenu`
    /// row model as the dynamic Apps menu so the dropdown renders it identically.
    edit_menu: crate::app_menu::AppMenu,
    /// Active light/dark theme (TASK-0072 Phase 9). Colors come from the matching
    /// baked snapshot (`theme()`); a switch is a const swap + full redraw. Boot
    /// default = Dark until settingsd's `ui.theme.mode` is applied (Phase 10).
    theme_mode: crate::theme::ThemeMode,
    dropdown_hover: Option<usize>,
    dropdown_atlas: crate::atlas::AtlasSurface,
    /// Open (animated) height of the dropdown = `app_menu.dropdown_full_h()`. The
    /// reserved atlas band (`dropdown_atlas`) is sized for the max list; this is
    /// the height of the *current* registry-sourced menu.
    dropdown_h: u32,
    dropdown_surface_dirty: bool,
    /// Dynamic Apps menu (RFC-0065): built from the `bundlemgrd` registry
    /// (`OP_LIST_APPS`), seeded until the lazy fetch on first open succeeds.
    app_menu: crate::app_menu::AppMenu,
    /// Whether the live registry fetch has been attempted (one-shot, lazy).
    app_menu_fetched: bool,
    /// The Search window — the first instance of the reusable `ShellWindow`
    /// glass-frame component (drag/close/scroll/cached-blur live in the component;
    /// the filtered word list below is the body content the runtime supplies).
    search: super::shell_window::ShellWindow,
    /// The Settings window (TASK-0072): a third `ShellWindow` instance, opened
    /// from the topbar Edit → Settings menu. Static body (no scroll) — its atlas
    /// surface is acquired on show and released on hide, like Search.
    settings_win: super::shell_window::ShellWindow,
    /// The DSL demo window frame (TASK-0076B).
    dsl_win: super::shell_window::ShellWindow,
    /// The cross-process app-client window (ADR-0042 R1): body pixels come
    /// from the app process's surface VMO via the damage-blit.
    app_win: super::shell_window::ShellWindow,
    /// ADR-0042 surface table + flow control (host-tested bookkeeping).
    client_surfaces: crate::client_surface::ClientSurfaces,
    /// The app's DEDICATED event channel (SEND cap slot, execd-attached via
    /// `OP_SURFACE_EVENTS`): input events + surface acks go out here — the
    /// shared response endpoint raced with inputd's ack drain (ADR-0042).
    #[cfg(nexus_env = "os")]
    app_event_channel: Option<u32>,
    /// Cached lifecycle-broker route (resolved lazily with retries — a
    /// single `new_for` attempt is one 100ms routing window and fails
    /// under load; the inputd windowd-route lesson).
    #[cfg(nexus_env = "os")]
    abilitymgr_client: Option<nexus_ipc::KernelClient>,
    /// Mounted DSL interpreter state (view + layout + markers).
    dsl_mount: dsl_mount::DslMount,
    /// Prefix-filtered words shown in the Search window body.
    search_filtered: Vec<&'static str>,
    /// Search window scroll engine — the SAME shared `animation::ScrollMomentum`
    /// that backs the chat window's `VirtualList` (E2: one Android-style eased
    /// momentum engine, not two implementations). Pixel offset; the whole filtered
    /// list is rendered once into a tall surface and this offset scrolls it via a
    /// GPU source-row offset (E3), so a flick coasts smoothly with zero re-render.
    search_scroll: ScrollMomentum,
    /// `nsec()` of the last search-momentum tick (frame-rate-independent integrate).
    search_scroll_last_ns: u64,
    /// Atlas allocator, kept live so windows can acquire surfaces on show and
    /// release them on hide (the on-demand surface pool — a closed window costs
    /// zero atlas rows). The boot layers reserved their bands from it in `new`.
    atlas_alloc: crate::atlas::AtlasAllocator,
    /// Chat scroll engine: the `nexus-virtual-list` component is the single
    /// source of truth for scroll *physics* (eased momentum via
    /// `fling`/`tick`) and owns the message provider. windowd remains the height
    /// authority (its bitmap hard-wrap measures the real surface) and feeds that
    /// in via `set_content_height`; the component clamps + eases. `chat_scroll_y`
    /// below is a u32 mirror of `chat_list.scroll_offset()` consumed by the
    /// overscan render + GPU source-row-offset composite (unchanged render path).
    /// `chat_visible` is rebuilt only on re-render (not per row).
    chat_list: VirtualList<ChatMessageProvider>,
    chat_scroll_y: u32,
    /// `nsec()` of the last momentum tick — lets `tick_chat_scroll` integrate the
    /// glide over real elapsed time (frame-rate independent). 0 = no glide active.
    chat_scroll_last_ns: u64,
    /// `nsec()` of the last emitted scroll-diagnostic line (rate-limited ~200ms).
    chat_scroll_diag_ns: u64,
    /// `nsec()` of the last emitted wheel-miss line (rate-limited ~500ms): a
    /// wheel over no window is an honest diag marker, never a silent no-op.
    wheel_miss_diag_ns: u64,
    /// Coalesced wheel delta: every queued `OP_UPDATE_VISIBLE_STATE` adds its
    /// `wheel_delta_y` here instead of scrolling immediately. The whole frame's
    /// worth is applied ONCE per present-loop iteration (`commit_scroll_input`).
    /// Without this, a flood of input events (hidrawd ~800/s during a drag) made
    /// windowd replay each queued wheel one-by-one → the scroll lagged further and
    /// further behind ("old commands still being processed the more I scroll").
    pending_chat_wheel: i32,
    /// Frame-aligned input sample (Android `Choreographer`/`InputConsumer` model):
    /// every queued `OP_UPDATE_VISIBLE_STATE` is STAGED here (latest cursor/buttons
    /// win, wheel deltas sum) and the full state is applied ONCE per present-loop
    /// iteration — not `apply_input_state`'d per raw event. Decouples per-frame
    /// work from input rate, so a flood (hidrawd ~800/s) can't back up the cursor
    /// command stream + hit-testing ("mouse vanished then everything caught up").
    pending_input: Option<VisibleState>,
    chat_content_h: u32,
    chat_visible: Vec<super::chat::ChatVisibleMsg>,
    /// Wrapped line count per message, precomputed ONCE (fixed wrap width) so a
    /// scroll re-window costs O(1) per message instead of re-measuring 5000
    /// texts per recenter.
    chat_msg_lines: Vec<u32>,
    /// Per-visible-message wrapped-line char ranges (shared, REUSED buffer —
    /// cleared per rebuild, never reallocated in steady state). The renderer
    /// indexes lines here instead of re-walking the wrap per pixel row.
    chat_line_ranges: Vec<(u32, u32)>,
    /// Scroll position the chat **overscan surface** is currently rendered at.
    /// The surface holds the content window `[base .. base + viewport + overscan]`;
    /// scrolling within that window is a GPU composite source-row offset
    /// (`chat_scroll_y - chat_render_base`), NOT a CPU re-render. We only re-render
    /// (recenter the base) when the scroll leaves the overscan window.
    chat_render_base: u32,
    /// One-shot `chat window drag ok` marker latch.
    chat_drag_marker_emitted: bool,
    /// One-shot `chat button click ok` marker latch (first real chat-button click).
    chat_button_marker_emitted: bool,
    /// Active shell configuration resolved from SystemUI's declarative manifest
    /// registry (`systemui::shell_config_default()` — the boot default product).
    /// Replaces the old hardcoded shell-chrome compile-time constants: the
    /// compositor chrome is now config-driven, so a later runtime shell switch
    /// (tablet/kiosk) just swaps this. Desktop default ⇒ chrome on.
    shell_config: systemui::ShellConfig,
    /// Session-authority probe (TASK-0065B): after the handoff, ask sessiond
    /// whether a session is active (apply its shell product) or the greeter
    /// owns the display. Bounded; unreachable = auto shell, never a brick.
    session_probe: session::SessionProbe,
    /// Persisted-theme probe (TASK-0072 Phase 10): after the handoff, GET
    /// `ui.theme.mode` from settingsd and apply it, so a saved light/dark
    /// choice is restored across reboots. Bounded, one-shot-until-success;
    /// settingsd unreachable/slow = the build-time default (Dark), never a brick.
    theme_probe: shell::ThemeProbe,
    /// The login greeter, while it owns the display (TASK-0065B): blurred
    /// wallpaper + avatar card baked into Plane 1; all shell affordances
    /// suppressed until sessiond accepts a login.
    greeter: Option<greeter::GreeterState>,
    /// The z/focus stack (host-tested SSOT in `window_scene`): the ONE ordering
    /// authority for shell windows. Scene emission composites in `order()` and
    /// input hit-tests in `hit_order()` (its exact reverse), replacing the old
    /// hardcoded emit/press sequence that pinned chat above search forever.
    /// Visibility here MIRRORS each `ShellWindow.visible` — kept in sync by the
    /// `show_window`/`hide_window` helpers, never written directly.
    windows: crate::window_scene::WindowStack,
    /// Dock surface (TASK-0070 Phase 2): the bottom-center bar of MINIMIZED
    /// windows. Allocated on the first minimize (sized for `MAX_WINDOWS`
    /// icons), freed when the last window restores — no permanent taskbar.
    dock_surface: Option<crate::atlas::AtlasSurface>,
    /// Icon count the dock surface currently renders (0 = never rendered).
    dock_rendered_n: usize,
    /// The dock surface needs re-rendering (membership changed).
    dock_dirty: bool,
    /// Active pointer shape (TASK-0070 Phase 3: resize edges swap the sprite).
    cursor_shape: cursor::CursorShape,
    /// Hotspot of the ACTIVE shape (SW/GL draw offset; gpud gets it per upload).
    cursor_hot: (i32, i32),
    /// Active edge-resize drag: (window, edge, drag-START frame, grab point).
    /// Resize math is deterministic in the start frame (`Frame::resized`).
    resize_drag: Option<(
        crate::window_scene::WindowId,
        crate::compositor::shell_window::ResizeEdge,
        crate::compositor::shell_window::Frame,
        (i32, i32),
    )>,
}

#[derive(Default)]
struct InputMarkerState {
    scheduler: bool,
    v2_present: bool,
    input: bool,
    full_window: bool,
    focus_route: bool,
    launcher_click_route: bool,
    v2_input: bool,
    cursor: bool,
    hover: bool,
    focus: bool,
    launcher_click: bool,
    keyboard: bool,
    wheel: bool,
    visible_input_summary: bool,
    visible_wheel_summary: bool,
    v2b_assets_summary: bool,
}

/// Reserve a full-width atlas band, emitting a precise OOM marker (which surface,
/// rows needed, rows remaining) on failure. Without this, a starved atlas makes
/// `new()` return a generic error → `windowd: init fail display-server` with no
/// hint which surface overflowed → the bootsplash stays and the cause is a hunt.
/// This turns it into one actionable log line (RFC-0066 "actionable errors").
fn alloc_band_or_log(
    atlas: &mut crate::atlas::AtlasAllocator,
    rows: u32,
    label: &str,
) -> Result<crate::atlas::AtlasSurface, WindowdError> {
    let remaining = atlas.rows_remaining();
    match atlas.alloc_band(rows) {
        Some(s) => Ok(s),
        None => {
            let _ = debug_println(&alloc::format!(
                "windowd: atlas OOM surface={label} need={rows} rem={remaining}"
            ));
            Err(WindowdError::BufferLengthMismatch)
        }
    }
}

impl DisplayServerRuntime {
    pub(crate) fn new() -> Result<Self, WindowdError> {
        let _ = debug_println(RUNTIME_INIT_START);
        // Runtime text (TASK-0070 Phase 6): dynamic text renders from the baked
        // glyph atlases of the manifest-default face (`ui.font.family` key shape).
        let _ = debug_println(&alloc::format!(
            "windowd: font family={} sizes=13,16",
            crate::assets::FONT_FAMILY
        ));
        let mode = VisibleBootstrapMode::fixed()?.validate()?;

        // Resolve the active shell from SystemUI's declarative manifest registry
        // (the boot default product). This drives the compositor chrome instead of
        // the old hardcoded shell-chrome constants — the first step of "the shell
        // is set in SystemUI". Infallible (desktop fallback).
        let shell_config = systemui::shell_config_default();
        let _ = debug_println(&alloc::format!(
            "windowd: shell config product={} profile={} shell={} kind={} chrome={} locked={}",
            shell_config.product_id,
            shell_config.profile_id,
            shell_config.shell_id,
            shell_config.shell_kind,
            shell_config.desktop_chrome,
            shell_config.locked,
        ));

        // Wallpaper: prefer JPEG, fall back to solid dark color on failure.
        // Production-grade: the compositor must start even without wallpaper assets.
        let (source_width, source_height, source_pixels) = if systemui::wallpaper_source_is_jpeg() {
            let _ = debug_println(WALLPAPER_LOADED);
            let (w, h) = systemui::wallpaper_decoded_size();
            (w, h, systemui::wallpaper_bgra())
        } else {
            let _ = debug_println(WALLPAPER_FALLBACK);
            // 160×100 solid dark-blue fallback — scaled to fill the display.
            const FALLBACK_W: u32 = 160;
            const FALLBACK_H: u32 = 100;
            static FALLBACK_BGRA: [u8; (FALLBACK_W * FALLBACK_H * 4) as usize] = {
                let mut buf = [0u8; (FALLBACK_W * FALLBACK_H * 4) as usize];
                let mut i = 0;
                while i < buf.len() {
                    buf[i] = 10; // B
                    buf[i + 1] = 22; // G
                    buf[i + 2] = 40; // R
                    buf[i + 3] = 255; // A
                    i += 4;
                }
                buf
            };
            (FALLBACK_W, FALLBACK_H, &FALLBACK_BGRA[..])
        };
        let source_frame = SourceFrame {
            width: source_width,
            height: source_height,
            stride: checked_stride(source_width)?,
            pixels: source_pixels,
        };
        let source_x_lut = build_scale_lut(mode.width, source_width)?;
        let source_y_lut = build_scale_lut(mode.height, source_height)?;
        let cursor = crate::render_assets::render_cursor_surface(CallerCtx::system());
        let (cursor_width, cursor_height) = match cursor {
            Some(cursor) => (cursor.width, cursor.height),
            None => (0, 0),
        };
        let initial_state = VisibleState {
            backend_visible: true,
            systemui_first_frame_visible: false,
            scene_ready: true,
            full_window_visible: true,
            click_target_visible: true,
            keyboard_target_visible: true,
            cursor_svg_visible: cursor_width != 0 && cursor_height != 0,
            text_target_visible: true,
            icon_target_visible: true,
            wallpaper_visible: systemui::wallpaper_source_is_jpeg(),
            cursor_x: 100,
            cursor_y: 100,
            ..VisibleState::default()
        };
        let _ = debug_println(RUNTIME_INIT_OK);
        let _ = debug_trace("dbg: windowd init self-build start");
        let band_scratch = alloc::vec![0u8; mode.stride as usize * ROW_WRITE_CHUNK];
        let _ = debug_trace("dbg: windowd init band-scratch ok");
        let blur_row_buf = alloc::vec![0u8; mode.stride as usize];
        let _ = debug_trace("dbg: windowd init blur-row ok");
        let layer_cache = LayerCache::default();
        let _ = debug_trace("dbg: windowd init layer-cache ok");
        let backdrop_cache = core::array::from_fn(|_| BackdropCacheEntry::new());
        let _ = debug_trace("dbg: windowd init backdrop-cache ok");
        let glass_layer = GlassLayerCache::new();
        let _ = debug_trace("dbg: windowd init glass-layer ok");
        let glass_scratch = alloc::vec![0u8; GLASS_LAYER_MAX_BYTES];
        let _ = debug_trace("dbg: windowd init glass-scratch ok");
        let path_cache = core::array::from_fn(|_| PathCacheEntry::new());
        let _ = debug_trace("dbg: windowd init path-cache ok");
        let animation_driver = AnimationDriver::new();
        let _ = debug_trace("dbg: windowd init animation-driver ok");
        let pipeline_timer = PipelineTimer::new();
        let _ = debug_trace("dbg: windowd init pipeline-timer ok");
        // Chat panel: 5000 deterministic mixed-length messages — a STRESS TEST of
        // the virtual list. Only the visible window is ever rendered, so scroll
        // must stay smooth regardless of collection size. The provider is the data
        // source; chat layout heights come from `interaction` (hard-wrap) so the
        // precomputed window matches the renderer exactly.
        let chat_provider = ChatMessageProvider::synthetic(
            5000,
            crate::interaction::chat_chars_per_line(),
            crate::interaction::CHAT_LINE_H,
        );
        let mut chat_visible = Vec::new();
        // Precompute every message's wrapped line count ONCE (fixed wrap width):
        // scroll re-windows then cost O(1) per message instead of re-measuring
        // 5000 texts per recenter (that full-collection walk was a visible hitch
        // at every overscan exhaustion).
        let mut chat_msg_lines = Vec::new();
        super::chat::build_lines_cache(&chat_provider, &mut chat_msg_lines);
        let mut chat_line_ranges = Vec::new();
        // Window the OVERSCAN content surface (viewport + overscan) at base 0.
        // compute_visible_window returns the TOTAL content height (for max-scroll)
        // and fills the window for the given render base.
        let chat_overscan_content_h = (crate::interaction::CHAT_PANEL_H + CHAT_OVERSCAN)
            .saturating_sub(crate::interaction::CHAT_TITLE_BAR_H + crate::interaction::CHAT_PAD);
        let chat_content_h = super::chat::compute_visible_window(
            &chat_provider,
            0,
            &mut chat_visible,
            chat_overscan_content_h,
            &chat_msg_lines,
            &mut chat_line_ranges,
        );
        // Hand the provider to the virtual-list component (scroll-physics SSOT).
        // windowd stays the height authority: the chat viewport is the panel body
        // (minus the title bar + pad), and the real content height comes from the
        // bitmap hard-wrap measure above — so the component's `max_scroll`/momentum
        // clamp to the true bottom while it owns the eased motion.
        let chat_viewport_h = crate::interaction::CHAT_PANEL_H
            .saturating_sub(crate::interaction::CHAT_PAD.saturating_mul(2));
        let mut chat_list = VirtualList::new(
            chat_provider,
            FxPx::new(chat_viewport_h as i32),
            VirtualListConfig::default(),
        );
        chat_list.set_content_height(FxPx::new(chat_content_h as i32));
        // Reserve the chat layer's surface in the VMO atlas (off-screen).
        let mut atlas = crate::atlas::AtlasAllocator::new();
        // Overscan-tall surface: viewport + CHAT_OVERSCAN extra content rows, so
        // scroll within the window is a composite source-row offset, not a re-render.
        let chat_atlas =
            alloc_band_or_log(&mut atlas, crate::interaction::CHAT_PANEL_H + CHAT_OVERSCAN, "chat")?;
        // Cache of the BLURRED backdrop behind the chat window. The backdrop is
        // the static base, so we blur it once per window move and reuse it every
        // present (Task #17 pattern) — zero per-frame blur for glass, the key to
        // running several glass layers at 120Hz.
        let chat_blur_cache =
            alloc_band_or_log(&mut atlas, crate::interaction::CHAT_PANEL_H, "chat_blur")?;
        // Cache of the FULLY COMPOSITED sidebar (blurred backdrop + glass tint +
        // border + icons). When the sidebar is settled (fully open, static), it's
        // composited once and then blitted each present — so a cursor move over
        // the sidebar costs one blit, not 4 full-height SDF fills.
        // When the new glass side panel is on, the old proof-sidebar (and its
        // full-height composite cache) is suppressed — reclaim those ~800 atlas
        // rows for the shell windows (topbar/panel/dropdown/search) instead of a
        // dead allocation. A 1-row dummy keeps the field valid.
        let sidebar_composite_cache = alloc_band_or_log(
            &mut atlas,
            if shell_config.desktop_chrome { 1 } else { mode.height },
            "sidebar_cache",
        )?;
        // Shell-P2b: reserve the glass topbar layer surface — full-width, one
        // bar tall. Composited at (TOPBAR_MARGIN_X, TOPBAR_TOP) with blur +
        // rounded corners + shadow each present.
        let shell_w = mode.width.saturating_sub(2 * super::desktop_layer::TOPBAR_MARGIN_X).max(1);
        let shell_h = super::desktop_layer::TOPBAR_H;
        let shell_atlas = alloc_band_or_log(&mut atlas, shell_h, "topbar")?;
        // Topbar Apps dropdown surface — small, fixed. Reserved BEFORE the side
        // panel so the side panel's "take the rest" clamp can't starve it (that
        // exhausted the atlas → new() Err → no handoff after the bootsplash).
        // The band is sized for the bounded MAX displayed list (`MAX_MENU_APPS`,
        // small) so a later registry fetch fits without re-reserving — and kept
        // small enough that the side-panel/search-pool tail survives (the atlas is
        // tight; the dropdown grew from 2 rows to this, the side panel absorbs it).
        let dropdown_band_h = super::desktop_layer::dropdown_band_h();
        let app_menu = crate::app_menu::AppMenu::seed();
        let dropdown_h = app_menu.dropdown_full_h();
        let dropdown_atlas = alloc_band_or_log(&mut atlas, dropdown_band_h, "dropdown")?;
        // The Search window as a ShellWindow instance (the reusable glass frame).
        // Its atlas surfaces are NOT reserved at boot — they are acquired from the
        // allocator on show and released on hide (on-demand pool). The boot path
        // instead reserves a contiguous tail (`WINDOW_POOL_ROWS`) for them below.
        let search_h = super::desktop_layer::search_full_h();
        let search = super::shell_window::ShellWindow::new(
            "Search",
            120,
            110,
            super::desktop_layer::SEARCH_W,
            search_h,
            super::desktop_layer::SEARCH_TITLE_H,
            super::desktop_layer::SEARCH_CLOSE_W,
            super::desktop_layer::SEARCH_RADIUS,
            18,
            5,
            90,
        );
        // The Settings window — same reusable glass frame, static body.
        let settings_win = super::shell_window::ShellWindow::new(
            "Settings",
            200,
            140,
            super::desktop_layer::SETTINGS_W,
            super::desktop_layer::settings_full_h(),
            super::desktop_layer::SETTINGS_TITLE_H,
            super::desktop_layer::SETTINGS_CLOSE_W,
            super::desktop_layer::SETTINGS_RADIUS,
            18,
            5,
            90,
        );
        // The DSL demo window (TASK-0076B) — same reusable glass frame; the
        // body is rendered from the DSL interpreter's retained scene.
        let dsl_win = super::shell_window::ShellWindow::new(
            "DSL Demo",
            420,
            160,
            dsl_mount::DSL_WIN_W,
            dsl_mount::DSL_WIN_H,
            dsl_mount::DSL_TITLE_H,
            dsl_mount::DSL_CLOSE_W,
            dsl_mount::DSL_RADIUS,
            18,
            5,
            90,
        );
        // The app-client window (ADR-0042 R1) — same reusable glass frame; the
        // body is blitted from the app's own surface VMO on present.
        let app_win = super::shell_window::ShellWindow::new(
            "App",
            460,
            420,
            app_window::APP_WIN_MAX_W,
            app_window::APP_WIN_MAX_H,
            app_window::APP_TITLE_H,
            app_window::APP_CLOSE_W,
            dsl_mount::DSL_RADIUS,
            18,
            5,
            90,
        );
        // Glass side panel surface — narrow, tall. Capped so a contiguous tail is
        // left for the on-demand window pool (content + blur cache); without this
        // reserve the panel's "take the rest" would starve a later search show.
        const WINDOW_POOL_ROWS: u32 = 2 * super::desktop_layer::search_full_h()
            + 2 * dsl_mount::DSL_WIN_H // DSL demo window: content + blur bands
            + 2 * app_window::APP_WIN_MAX_H // app-client window (ADR-0042 R1)
            + 16;
        let sidepanel_h = mode
            .height
            .saturating_sub(super::desktop_layer::SIDEPANEL_TOP + super::desktop_layer::SIDEPANEL_MARGIN)
            .max(1)
            .min(atlas.rows_remaining().saturating_sub(WINDOW_POOL_ROWS).max(1));
        let sidepanel_atlas = alloc_band_or_log(&mut atlas, sidepanel_h, "sidepanel")?;
        // The Chat window as a ShellWindow instance (the SAME reusable glass frame
        // as Search). It mounts the overscan content band + blur cache reserved
        // above and starts open at the panel's default position. E1 retired the
        // single-window `wm::WindowManager`; geometry now lives in `window_frame`.
        let mut chat = super::shell_window::ShellWindow::new(
            "Chat",
            crate::interaction::CHAT_PANEL_X as i32,
            crate::interaction::CHAT_PANEL_Y as i32,
            crate::interaction::CHAT_PANEL_W,
            crate::interaction::CHAT_PANEL_H,
            crate::interaction::CHAT_TITLE_BAR_H,
            crate::interaction::CHAT_CLOSE_ZONE_W,
            super::desktop_layer::SEARCH_RADIUS,
            CHAT_SHADOW_BLUR,
            CHAT_SHADOW_OFFSET_Y,
            CHAT_SHADOW_ALPHA as u32,
        );
        chat.mount(chat_atlas, Some(chat_blur_cache));
        // RFC-0065: the desktop starts clean — chat is NOT auto-shown. It opens on
        // demand (chat button / "Chat" in the Apps dropdown via `toggle_chat`),
        // the first visible step away from a baked-open window toward a launched app.
        chat.visible = false;
        let _ = debug_trace("dbg: windowd init chat hidden ok");
        Ok(Self {
            mode,
            source_frame,
            source_x_lut,
            source_y_lut,
            cursor_width,
            cursor_height,
            framebuffer: None,
            band_scratch,
            blur_row_buf,
            state: initial_state,
            observer_state: initial_state,
            markers_emitted: false,
            input_markers_emitted: InputMarkerState::default(),
            input_state_debug_emitted: false,
            pending_damage_rects: Vec::new(),
            tile_map: TileMap::new(),
            layer_cache,
            pending_damage_rect: None,
            pending_cursor_rect: None,
            pending_gpu_blit_rect: None,
            sidebar_blur_cache_valid: false,
            button_blur_cache_valid: false,
            paint_only_damage: false,
            telemetry: crate::telemetry::WindowdDisplayTelemetry::default(),
            backdrop_cache,
            glass_layer,
            glass_scratch,
            path_cache,
            active_filter_idx: 0,
            filter_cycle: 0,
            clipping_marker_emitted: false,
            scroll_marker_emitted: false,
            live_scroll_marker_emitted: false,
            selftest_v3b_emitted: false,
            v3b_composition_verified: false,
            v3b_markers_emitted: false,
            animation_driver,
            animated_scene: AnimatedSceneState::new(),
            animation_proof: AnimationProofState::default(),
            gpud_client: None,
            pipeline_timer,
            scene_cb: CommandBuffer::new(),
            shell: SystemUiShell::new(DeviceProfile::qemu_default()),
            framebuffer_pending_first_write: false,
            present_seq: 0,
            stall_last_progress_ns: 0,
            stall_last_seq: 0,
            stall_reported: false,
            present_fail_reported: false,
            frames_in_flight: 0,
            last_completed_seq: 0,
            current_display_slot: 0,
            first_handoff_id: 0,
            first_handoff_deadline_ns: 0,
            first_handoff_frame_written: false,
            first_handoff_bootstrap_markers_emitted: false,
            first_handoff_attach_acked: false,
            first_handoff_present_sent: false,
            hw_cursor_active: false,
            gl_cursor_active: false,
            precache_blur_pending: true,
            button_hover: false,
            chat,
            sidebar_composite_cache,
            sidebar_composite_cache_valid: false,
            shell_atlas,
            shell_w,
            shell_h,
            shell_surface_dirty: true,
            topbar_hover: None,
            topbar_menu_hover: false,
            sidepanel_atlas,
            sidepanel_h,
            sidepanel_surface_dirty: true,
            open_topbar_menu: None,
            edit_menu: crate::app_menu::AppMenu::single("settings", "Settings"),
            theme_mode: crate::theme::ThemeMode::Dark,
            dropdown_hover: None,
            dropdown_atlas,
            dropdown_h,
            dropdown_surface_dirty: true,
            app_menu,
            app_menu_fetched: false,
            session_probe: session::SessionProbe::default(),
            theme_probe: shell::ThemeProbe::default(),
            greeter: None,
            search,
            settings_win,
            dsl_win,
            app_win,
            client_surfaces: crate::client_surface::ClientSurfaces::new(),
            #[cfg(nexus_env = "os")]
            app_event_channel: None,
            #[cfg(nexus_env = "os")]
            abilitymgr_client: None,
            dsl_mount: dsl_mount::DslMount::new(),
            search_filtered: {
                let mut v = Vec::new();
                super::desktop_layer::search_filter("", &mut v);
                v
            },
            search_scroll: ScrollMomentum::new(ScrollConfig::default()),
            search_scroll_last_ns: 0,
            atlas_alloc: atlas,
            chat_list,
            chat_scroll_y: 0,
            chat_scroll_last_ns: 0,
            chat_scroll_diag_ns: 0,
            wheel_miss_diag_ns: 0,
            pending_chat_wheel: 0,
            pending_input: None,
            chat_content_h,
            chat_visible,
            chat_msg_lines,
            chat_line_ranges,
            chat_render_base: 0,
            chat_drag_marker_emitted: false,
            chat_button_marker_emitted: false,
            shell_config,
            // Registration order = initial stacking (later on top once shown);
            // both start hidden, mirroring the ShellWindow `visible` flags above.
            windows: crate::window_scene::WindowStack::new(&[
                crate::window_scene::WindowId::Search,
                crate::window_scene::WindowId::Chat,
                crate::window_scene::WindowId::Settings,
                crate::window_scene::WindowId::DslDemo,
                crate::window_scene::WindowId::AppClient,
            ]),
            dock_surface: None,
            dock_rendered_n: 0,
            dock_dirty: false,
            cursor_shape: cursor::CursorShape::Default,
            cursor_hot: (crate::assets::CURSOR_HOTSPOT_X, crate::assets::CURSOR_HOTSPOT_Y),
            resize_drag: None,
        })
    }

    pub(crate) const fn visible_state(&self) -> VisibleState {
        self.observer_state
    }

    fn refresh_observer_state(&mut self) {
        self.observer_state.backend_visible |= self.state.backend_visible;
        self.observer_state.display_scanout_ready |= self.state.display_scanout_ready;
        self.observer_state.systemui_first_frame_visible |= self.state.systemui_first_frame_visible;
        self.observer_state.virtio_raw_seen |= self.state.virtio_raw_seen;
        self.observer_state.hid_normalized_seen |= self.state.hid_normalized_seen;
        self.observer_state.scene_ready |= self.state.scene_ready;
        self.observer_state.full_window_visible |= self.state.full_window_visible;
        self.observer_state.click_target_visible |= self.state.click_target_visible;
        self.observer_state.keyboard_target_visible |= self.state.keyboard_target_visible;
        self.observer_state.input_visible_on |= self.state.input_visible_on;
        self.observer_state.cursor_move_visible |= self.state.cursor_move_visible;
        // Hover proof: either hover source (test card or glass button) counts —
        // the automated injection drives the pointer onto the button.
        self.observer_state.hover_visible |= self.state.hover_visible || self.button_hover;
        self.observer_state.sidebar_open_visible |= self.state.sidebar_open_visible;
        self.observer_state.focus_visible |= self.state.focus_visible;
        self.observer_state.launcher_click_visible |= self.state.launcher_click_visible;
        self.observer_state.keyboard_visible |= self.state.keyboard_visible;
        self.observer_state.wheel_up_visible |= self.state.wheel_up_visible;
        self.observer_state.wheel_down_visible |= self.state.wheel_down_visible;
        self.observer_state.pointer_route_live |= self.state.pointer_route_live;
        self.observer_state.keyboard_route_live |= self.state.keyboard_route_live;
        self.observer_state.cursor_svg_visible |= self.state.cursor_svg_visible;
        self.observer_state.text_target_visible |= self.state.text_target_visible;
        self.observer_state.icon_target_visible |= self.state.icon_target_visible;
        self.observer_state.wallpaper_visible |= self.state.wallpaper_visible;
        self.observer_state.cursor_overlay_visible |= self.state.cursor_overlay_visible;
        self.observer_state.cursor_x = self.state.cursor_x;
        self.observer_state.cursor_y = self.state.cursor_y;
        self.observer_state.text_input_len = self.state.text_input_len;
        self.observer_state.text_input_bytes = self.state.text_input_bytes;
    }

    fn reset_effect_caches(&mut self) {
        for entry in &mut self.backdrop_cache {
            entry.valid = false;
        }
        self.glass_layer.valid = false;
    }

    // ── Window stack sync (TASK-0070 Phase 1) ──
    // The `windows` stack mirrors each ShellWindow's `visible` flag and owns
    // z/focus. Every visibility change goes through these helpers so the two
    // can never drift; raises emit honest markers + damage the affected rects.

    /// Stable short name for markers/diagnostics.
    pub(super) fn window_name(id: crate::window_scene::WindowId) -> &'static str {
        match id {
            crate::window_scene::WindowId::Chat => "chat",
            crate::window_scene::WindowId::Search => "search",
            crate::window_scene::WindowId::Settings => "settings",
            crate::window_scene::WindowId::DslDemo => "dsl",
            crate::window_scene::WindowId::AppClient => "app",
        }
    }

    /// The on-screen damage rect (incl. shadow halo) of a stack window.
    pub(super) fn window_damage_rect(&self, id: crate::window_scene::WindowId) -> DamageRect {
        let win = match id {
            crate::window_scene::WindowId::Chat => &self.chat,
            crate::window_scene::WindowId::Search => &self.search,
            crate::window_scene::WindowId::Settings => &self.settings_win,
            crate::window_scene::WindowId::DslDemo => &self.dsl_win,
            crate::window_scene::WindowId::AppClient => &self.app_win,
        };
        win.damage_rect(self.mode.width, self.mode.height)
    }

    /// Mirror a window becoming visible into the stack: it is raised to the top
    /// and focused (opening is user intent). Callers still run their own
    /// mount/damage logic — this only owns ordering + focus + markers.
    pub(super) fn show_window(&mut self, id: crate::window_scene::WindowId) {
        self.windows.show(id);
        let _ = debug_println(&alloc::format!(
            "windowd: focus id={} z={}",
            Self::window_name(id),
            self.windows.z_of(id),
        ));
    }

    /// Mirror a window hiding into the stack; focus falls to the topmost
    /// remaining visible window (marker only when focus actually moved). A
    /// close also leaves the dock and forgets fullscreen (fresh open =
    /// floating at the remembered origin), so the dock is reconciled here.
    pub(super) fn hide_window(&mut self, id: crate::window_scene::WindowId) {
        let before = self.windows.focused();
        self.windows.hide(id);
        match id {
            crate::window_scene::WindowId::Chat => self.chat.leave_fullscreen(),
            crate::window_scene::WindowId::Search => self.search.leave_fullscreen(),
            crate::window_scene::WindowId::Settings => self.settings_win.leave_fullscreen(),
            crate::window_scene::WindowId::DslDemo => self.dsl_win.leave_fullscreen(),
            crate::window_scene::WindowId::AppClient => self.app_win.leave_fullscreen(),
        }
        self.update_dock();
        let after = self.windows.focused();
        if after != before {
            if let Some(next) = after {
                let _ = debug_println(&alloc::format!(
                    "windowd: focus id={} z={}",
                    Self::window_name(next),
                    self.windows.z_of(next),
                ));
            }
        }
    }

    /// Click-to-raise: bring `id` to the top + focus it. When the stack order
    /// actually changed, damage every visible window rect (the overlap regions
    /// swap occlusion) so the next present recomposites the new order.
    pub(super) fn raise_window(&mut self, id: crate::window_scene::WindowId) {
        let focus_changed = self.windows.focused() != Some(id);
        if self.windows.raise(id) {
            let _ = debug_println(&alloc::format!(
                "windowd: raise id={} z={}",
                Self::window_name(id),
                self.windows.z_of(id),
            ));
            let (order, n) = self.windows.order(USE_DESKTOP_SHELL);
            for &wid in &order[..n] {
                let rect = self.window_damage_rect(wid);
                self.queue_gpu_blit_rect(rect);
            }
        } else if focus_changed {
            let _ = debug_println(&alloc::format!(
                "windowd: focus id={} z={}",
                Self::window_name(id),
                self.windows.z_of(id),
            ));
        }
    }
}
