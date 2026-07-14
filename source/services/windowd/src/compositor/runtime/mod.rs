// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Display server runtime state machine for the windowd compositor:
//! retained-mode compositing, tile damage tracking, input routing, cursor management,
//! and present scheduling.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 13 unit tests (QEMU) + host smoke integration

use crate::geometry::checked_stride;
use super::damage::cursor_damage_rect;
use super::emit_windowd_telemetry;
use super::filter::filter_layout_variant_index;
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
use crate::compositor::damage::{
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

// Gate 2: the windowd↔gpud wire is the shared SSOT in `nexus-display-proto`;
// these local names just re-source its values (no more hand-mirroring gpud).
const GPU_ANIMATION_SUBMIT_OP: u8 = nexus_display_proto::OP_SUBMIT_ANIMATION_FRAME;
const GPU_SET_FRAMEBUFFER_VMO_OP: u8 = nexus_display_proto::OP_SET_FRAMEBUFFER_VMO;
const GPU_PRESENT_DAMAGE_OP: u8 = nexus_display_proto::OP_PRESENT_DAMAGE;
const GPU_MOVE_CURSOR_OP: u8 = nexus_display_proto::OP_MOVE_CURSOR;
const GPU_UPLOAD_CURSOR_OP: u8 = nexus_display_proto::OP_UPLOAD_CURSOR;
const GPU_SET_LAYER_SCROLL_OP: u8 = nexus_display_proto::OP_SET_LAYER_SCROLL;
const GPU_UPLOAD_ICON_OP: u8 = nexus_display_proto::OP_UPLOAD_ICON;
const GPU_UPLOAD_CURSOR_SHAPE_OP: u8 = nexus_display_proto::OP_UPLOAD_CURSOR_SHAPE;
const GPU_SELECT_CURSOR_SHAPE_OP: u8 = nexus_display_proto::OP_SELECT_CURSOR_SHAPE;
const GPUD_STATUS_OK: u8 = nexus_display_proto::STATUS_OK;
pub(crate) const GPUD_FALLBACK_SEND_SLOT: u32 = 5;
pub(crate) const GPUD_FALLBACK_RECV_SLOT: u32 = 6;
const FIRST_HANDOFF_DEADLINE_NS: u64 = 1_000_000_000;
use crate::systemui_shell::{CLICK_LAYER_ID, HOVER_LAYER_ID, KEYBOARD_LAYER_ID, SIDEBAR_LAYER_ID};
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
mod chrome_widget;
mod transitions;
mod cursor;
mod gpud;
mod marker_emit;
mod framebuffer;
mod input;
mod input_scroll;
mod shell;
pub(crate) mod app_window;
mod app_surface;
mod desktop_surface;
mod present;
mod scene;
mod session;
mod wm;

// The split-out `impl` submodules live one module deeper than the original
// `runtime/mod.rs`, so the compositor-level siblings + consts they reference via
// `super::` are re-exported here under `runtime` to keep those paths resolving
// (TASK-0063 modularization; pure path plumbing, no behavior change).
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
}

impl AnimatedSceneState {
    const fn new() -> Self {
        Self {
            hover_opacity: 0.0,
            sidebar_translate_x: 320.0,
            sidebar_opacity: 0.0,
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

/// Hover routing targets (see `DisplayServerRuntime::hover_route`).
pub(crate) const HOVER_ROUTE_NONE: u8 = 0;
pub(crate) const HOVER_ROUTE_DESKTOP: u8 = 1;
pub(crate) const HOVER_ROUTE_APP: u8 = 2;

/// One concurrently open floating app-client window (RFC-0065 multi-window):
/// `WindowId::App(i)` indexes the runtime's `apps[i]`. Bundles the per-window
/// state that used to live as `app_win`/`app_*` singletons — the frame object
/// (bands, drag, hover), the bound surface, the live-resize overlay, the glass
/// layer set, the window intent and the app's dedicated event channel.
pub(crate) struct AppWindowSlot {
    /// Frame + atlas bands + drag/hover state (the reusable glass window).
    pub(crate) win: super::shell_window::ShellWindow,
    /// The bound client surface id (`None` = slot free).
    pub(crate) surface_id: Option<u32>,
    /// Live-resize title-bar overlay (TASK #23).
    pub(crate) title_overlay: Option<crate::atlas::AtlasSurface>,
    /// Frame width the overlay was last rendered at (0 = never).
    pub(crate) title_overlay_w: u32,
    /// R1 layer seam: the app's material-tagged glass regions.
    pub(crate) layers: [nexus_display_proto::client_surface::LayerDesc;
        nexus_display_proto::client_surface::MAX_SURFACE_LAYERS],
    pub(crate) layer_count: usize,
    /// Window intent (`OP_SURFACE_INTENT` tags, sent before create).
    pub(crate) intent_style: u8,
    pub(crate) intent_level: u8,
    pub(crate) intent_mode: u8,
    pub(crate) intent_resizable: bool,
    /// The app's DEDICATED event channel (SEND cap slot, `OP_SURFACE_EVENTS`).
    #[cfg(nexus_env = "os")]
    pub(crate) event_channel: Option<u32>,
    /// One-shot frame-pulse request (`OP_SURFACE_FRAME_REQ`, the Choreographer
    /// contract): answered with ONE `OP_SURFACE_FRAME` after the next
    /// composited frame, then cleared — the client re-requests while animating.
    pub(crate) frame_pulse_pending: bool,
    /// WebRender compositor-scroll id (0 = non-scrollable, unchanged behavior).
    /// Assigned `slot_index + 1` at create when the surface declared a scroll
    /// band (`content_h > 0`); ≤ `MAX_SCROLL_IDS`. windowd is the SINGLE writer
    /// of the scroll position: a wheel notch shifts the gpud layer `src_row`
    /// (`OP_SET_LAYER_SCROLL`) instead of the app re-rendering per frame.
    pub(crate) scroll_id: u32,
    /// Scroll band geometry from `SURFACE_CREATE` (surface rows): the tall
    /// resident-content height, and the fixed top/bottom chrome heights the app
    /// packed into the band (chat: Toolbar / composer). `content_h == 0` ⇒ not
    /// scrollable.
    pub(crate) content_h: u32,
    pub(crate) header_h: u32,
    pub(crate) footer_h: u32,
    /// Current absolute scroll offset (rows), windowd-owned. Clamped to
    /// `content_h - visible_body_h`; drives the `src_row` override.
    pub(crate) scroll_rows: u32,
    /// Per-slot scroll physics (reused `animation::ScrollMomentum`): a wheel
    /// notch extends a target the offset eases toward; the pacer ticks it while
    /// animating and re-emits `OP_SET_LAYER_SCROLL` each tick (flings without
    /// the app in the per-frame loop).
    pub(crate) scroll_momentum: ScrollMomentum,
    /// Last scroll-physics tick (ns) for dt integration.
    pub(crate) scroll_last_ns: u64,
    /// Damage-bounded band blit (ADR-0042, the 120Hz damage contract): union
    /// of the client's presented damage rows (surface/body coords, end
    /// exclusive) still to blit. `None` = FULL body re-blit (a present with no
    /// rects, resize, scrollable band). `render_app_surface` copies only these
    /// rows out of the client VMO — a 16-row animation present costs 16
    /// row-copies, not the whole window body + title chrome re-raster.
    pub(crate) surface_dirty_rows: Option<(u32, u32)>,
    /// Track C3: the window's CURRENT animated whole-layer transform (window
    /// transitions — open/close/minimize). Identity at rest; windowd's own
    /// `AnimationDriver` interpolates it and each tick emits ONE
    /// `OP_SET_LAYER_TRANSFORM` (gpud records + re-composites, no re-render).
    /// Full presents BAKE translate+opacity into the encoded layer (gpud
    /// clears its override table then — the scroll snap-back contract).
    pub(crate) transform: WinTransform,
    /// Deferred WM action executed when the transition converges (close
    /// after fade-out, minimize after fly-to-dock).
    pub(crate) pending_wm: Option<PendingWm>,
}

/// A window's animated whole-layer transform (identity = no visible change).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct WinTransform {
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    /// 0.0..=1.0 (1.0 = opaque).
    pub(crate) opacity: f32,
    /// 1.0 = identity (uniform, about the window center).
    pub(crate) scale: f32,
    /// True while a transition drives this window (emit overrides + bake).
    pub(crate) active: bool,
}

impl WinTransform {
    pub(crate) const IDENTITY: WinTransform =
        WinTransform { dx: 0.0, dy: 0.0, opacity: 1.0, scale: 1.0, active: false };
}

/// The WM action a finished transition performs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingWm {
    Close,
    Minimize,
}

impl AppWindowSlot {
    /// A fresh (unbound) slot. Windows CASCADE by slot index so several apps
    /// opened in sequence never stack pixel-exactly on top of each other.
    fn new(index: usize) -> Self {
        let win = super::shell_window::ShellWindow::new(
            "App",
            300 + (index as i32) * 64,
            140 + (index as i32) * 56,
            app_window::APP_WIN_MAX_W,
            app_window::APP_WIN_MAX_H,
            app_window::APP_TITLE_H,
            app_window::APP_CLOSE_W,
            app_window::APP_WIN_RADIUS,
            18,
            5,
            90,
        );
        Self {
            win,
            surface_id: None,
            title_overlay: None,
            title_overlay_w: 0,
            layers: [nexus_display_proto::client_surface::LayerDesc::default();
                nexus_display_proto::client_surface::MAX_SURFACE_LAYERS],
            layer_count: 0,
            intent_style: nexus_display_proto::client_surface::WIN_STYLE_TITLEBAR,
            intent_level: nexus_display_proto::client_surface::WIN_LEVEL_NORMAL,
            intent_mode: nexus_display_proto::client_surface::WIN_MODE_AUTO,
            intent_resizable: true,
            #[cfg(nexus_env = "os")]
            event_channel: None,
            frame_pulse_pending: false,
            scroll_id: 0,
            content_h: 0,
            header_h: 0,
            footer_h: 0,
            scroll_rows: 0,
            scroll_momentum: ScrollMomentum::new(ScrollConfig::default()),
            scroll_last_ns: 0,
            surface_dirty_rows: None,
            transform: WinTransform::IDENTITY,
            pending_wm: None,
        }
    }
}

/// Frame pacing interval (120 Hz). SSOT for both the compositor loop's pacer
/// timer and the paced damage flush (`flush_pending_damage_paced`).
pub(crate) const PACER_INTERVAL_NS: u64 = 8_333_333;

/// WebRender compositor-scroll id-table capacity — MUST match gpud's
/// `backend::MAX_SCROLL_IDS` (ids `1..=MAX_SCROLL_IDS`; 0 = not scrollable).
/// `MAX_APP_WINDOWS` (4) is ≤ this, so a `slot_index + 1` id always fits.
pub(crate) const MAX_SCROLL_IDS: usize = 8;

/// Per-notch scroll step (surface rows) — the fixed, predictable amount a wheel
/// notch adds to the scroll target (mirrors the app-host `STEP_PX`).
pub(crate) const SCROLL_STEP_PX: i32 = 72;

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
    /// True when pending damage only affects paint (no layout/shadow change needed).
    paint_only_damage: bool,
    pending_damage_rect: Option<DamageRect>,
    /// Cursor damage (old ∪ new pointer rect) for the next frame. Tracked
    /// separately from content damage: cursor rects only need a retained→display
    /// blit + cursor overlay (no CPU recomposite — Plane 1 is already cursor-free).
    pending_cursor_rect: Option<DamageRect>,
    /// Monotonic stamp of the last paced damage flush — vsync alignment for
    /// sustained input bursts (see `flush_pending_damage_paced`).
    last_paced_flush_ns: u64,
    /// Animation-driven frame: only GPU CB params changed (translate_x, opacity).
    /// Plane 1 is already current — no CPU recomposite needed. Merged rect passed
    /// to the GPU CB blit list so the display plane is refreshed from Plane 1.
    pending_gpu_blit_rect: Option<DamageRect>,
    telemetry: crate::telemetry::WindowdDisplayTelemetry,
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
    /// P0.3 self-heal: consecutive present NACKs from gpud (deadline-missed /
    /// lost-command frames). Each NACK requeues full-frame damage; the budget
    /// bounds the retries so a permanently failing device degrades loudly
    /// (FAIL marker) instead of re-presenting forever. Reset on a clean ack.
    present_retry_count: u32,
    /// One-shot latch for the retries-exhausted FAIL marker (per episode).
    present_retry_exhausted: bool,
    /// Bounded counter for the app-surface present-rejection diagnostic
    /// (P0.2 tap repro): a rejected client present is otherwise silent.
    app_present_reject_markers: u32,
    /// Bounded `surface presented` proof markers (first few presents only —
    /// a per-present formatted marker at hover/animation rates floods the
    /// UART and leaks on the non-freeing bump heap).
    app_present_markers: u32,
    /// Damage-limited desktop blit: the union row span (start, end exclusive)
    /// of the client damage rects since the last `render_desktop_surface`.
    /// `(u32::MAX, 0)` = empty. A present with no rects = full span.
    desktop_dirty_rows: (u32, u32),
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
    /// True once every pointer shape has been pushed into gpud's shape cache
    /// (OP_UPLOAD_CURSOR_SHAPE). Shape changes are then a 2-byte fire-and-forget
    /// OP_SELECT_CURSOR_SHAPE instead of a blocking 4KB re-upload per
    /// window-edge crossing. False = self-healing fallback to the upload path.
    shape_cache_pushed: bool,
    /// Active light/dark theme (TASK-0072 Phase 9). Colors come from the matching
    /// baked snapshot (`theme()`); a switch is a const swap + full redraw. Boot
    /// default = Dark until settingsd's `ui.theme.mode` is applied (Phase 10).
    theme_mode: crate::theme::ThemeMode,
    /// The cross-process app-client windows (ADR-0042 R1 → RFC-0065
    /// multi-window): one SLOT per concurrently open floating app window
    /// (`WindowId::App(i)` indexes this array). Each slot bundles what used to
    /// be the `app_win` singleton — frame + bands, resize overlay, glass
    /// layers, intent, surface binding and event channel.
    apps: [AppWindowSlot; crate::window_scene::MAX_APP_WINDOWS],
    /// ADR-0042 surface table + flow control (host-tested bookkeeping).
    client_surfaces: crate::client_surface::ClientSurfaces,
    /// Nonce → event-channel map (bounded, LRU-replace): each app-host attaches
    /// its OWN channel tagged with a self-minted nonce and repeats the nonce on
    /// SURFACE_CREATE — the bind is deterministic under N concurrent connects
    /// (the pending-slot era crossed channels between greeter/shell/counter).
    /// Non-consuming: a resize RE-create binds the same nonce again.
    #[cfg(nexus_env = "os")]
    event_channels: [(u64, u32); 8],
    #[cfg(nexus_env = "os")]
    event_channels_len: usize,
    /// The DESKTOP surface (RFC-0065 Umbau #17): the shell/greeter app-host that
    /// declared `level: desktop`. Own slot — id, event channel, full-screen
    /// atlas band, dirty flag — fully separate from the floating `app_win`
    /// (counter), so both coexist. Composited as the base layer (bottom z-band).
    desktop_surface_id: Option<u32>,
    #[cfg(nexus_env = "os")]
    desktop_channel: Option<u32>,
    desktop_band: Option<crate::atlas::AtlasSurface>,
    desktop_dirty: bool,
    /// One-shot frame pulse armed by the desktop surface (shell scroll).
    desktop_frame_pulse: bool,

    /// Once-guard for the transitional shell-as-app-host launch (TASK-0080C
    /// #17): fired on the FIRST session activation (STATE_ACTIVE — after
    /// sessiond authorized), not at boot (pre-login launches are denied by
    /// abilitymgr's session gate). Re-activations (unlock) must not relaunch.
    shell_app_launched: bool,
    /// The environment's windowing POLICY (`intent ⟂ policy`, RFC-0065): the
    /// shell profile the product selects. Desktop honours app intent; Kiosk
    /// forces chromeless/non-resizable (single-app OS). Consumed ONLY through
    /// `surface_presentation::WindowPresentation::resolve` — never re-derived.
    windowing_policy: crate::surface_presentation::WindowingPolicy,

    /// Cached lifecycle-broker route (resolved lazily with retries — a
    /// single `new_for` attempt is one 100ms routing window and fails
    /// under load; the inputd windowd-route lesson).
    #[cfg(nexus_env = "os")]
    abilitymgr_client: Option<nexus_ipc::KernelClient>,
    /// Atlas allocator, kept live so windows can acquire surfaces on show and
    /// release them on hide (the on-demand surface pool — a closed window costs
    /// zero atlas rows). The boot layers reserved their bands from it in `new`.
    atlas_alloc: crate::atlas::AtlasAllocator,
    /// Frame-aligned input sample (Android `Choreographer`/`InputConsumer` model):
    /// every queued `OP_UPDATE_VISIBLE_STATE` is STAGED here (latest cursor/buttons
    /// win, wheel deltas sum) and the full state is applied ONCE per present-loop
    /// iteration — not `apply_input_state`'d per raw event. Decouples per-frame
    /// work from input rate, so a flood (hidrawd ~800/s) can't back up the cursor
    /// command stream + hit-testing ("mouse vanished then everything caught up").
    pending_input: Option<VisibleState>,
    /// The DESKTOP surface's material-tagged glass regions (R1 seam): each
    /// composites as a frosted layer over the wallpaper in the Desktop arm.
    desktop_layers: [nexus_display_proto::client_surface::LayerDesc;
        nexus_display_proto::client_surface::MAX_SURFACE_LAYERS],
    desktop_layer_count: usize,
    /// Hover routing state (RFC-0067 R2): which surface currently receives
    /// frame-aligned pointer MOVEs (`HOVER_ROUTE_*`). On a target change the
    /// previous surface gets an `INPUT_KIND_LEAVE` so its hover wash clears.
    hover_route: u8,
    /// Last hover position forwarded (display space) — carried on LEAVE.
    hover_last: (i32, i32),
    /// Which app-window slot the hover route targets (valid when
    /// `hover_route == HOVER_ROUTE_APP`; a window-to-window crossing is a
    /// route change so the old window gets its LEAVE).
    hover_app_idx: usize,
    /// Bounded S1 rate diagnostics: wheel deltas that reached the router.
    wheel_route_count: u32,
    /// One-time proof marker: the first drag move composited as a pure GPU
    /// transform (Track C1 — no CPU Plane-1 recomposite, no band re-blit).
    drag_transform_marker: bool,
    /// Wait-cursor state (animated loading ring): pending launches that have
    /// not surfaced yet + the failsafe deadline + the ring's current frame.
    cursor_wait_n: u32,
    cursor_wait_deadline_ns: u64,
    cursor_ring_frame: u8,
    cursor_ring_active: bool,
    /// P3.2 windows-as-widgets: the ONE shared widget-rendered title-bar
    /// raster (see `chrome_widget.rs`).
    chrome_cache: chrome_widget::ChromeCache,
    /// Bounded S1 rate diagnostics: wheel deltas staged from inputd pushes.
    wheel_stage_count: u32,
    /// One-time proof marker latch for the hover chain.
    hover_marker_emitted: bool,
    /// Last hover-MOVE forward (ns) — ~33Hz throttle.
    hover_last_move_ns: u64,
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
    /// DSL-greeter login watch (Umbau #17): armed when sessiond reports
    /// STATE_GREETER (the DSL greeter app-host owns the display; the built-in
    /// avatar greeter is DELETED). The login happens OUT of process (greeter
    /// app-host → sessiond), so windowd polls sessiond on a slow cadence until
    /// the session activates, then applies the session shell. Disarmed on
    /// activation.
    greeter_login_watch: bool,
    /// Monotonic deadline before the next login-watch poll.
    greeter_watch_next_ns: u64,
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
    /// The fixed-maximum 1280×800 mode (tests + host harness).
    pub(crate) fn new() -> Result<Self, WindowdError> {
        Self::new_with_mode(crate::compositor::DISPLAY_WIDTH, crate::compositor::DISPLAY_HEIGHT)
    }

    /// Boot entry: the VISIBLE mode gpud resolved from the device (≤ the
    /// fixed shared-VMO layout; `mode.stride` stays the fixed layout pitch).
    pub(crate) fn new_with_mode(visible_w: u32, visible_h: u32) -> Result<Self, WindowdError> {
        let _ = debug_println(RUNTIME_INIT_START);
        // Runtime text (TASK-0070 Phase 6): dynamic text renders from the baked
        // glyph atlases of the manifest-default face (`ui.font.family` key shape).
        let _ = debug_println(&alloc::format!(
            "windowd: font family={} sizes=13,16",
            crate::assets::FONT_FAMILY
        ));
        let mode = VisibleBootstrapMode::for_visible(visible_w, visible_h)?;

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
        let (source_width, source_height, source_pixels, source_rows) =
            if systemui::wallpaper_source_is_jpeg() {
            let _ = debug_println(WALLPAPER_LOADED);
            let (w, h) = systemui::wallpaper_decoded_size();
            // Boot theme default is DARK — start on the theme-matched
            // wallpaper; `set_theme_mode` swaps the source live. ROW-RLE:
            // both variants full-res inside the image budget.
            let (data, rows) = systemui::wallpaper_rle_for(true);
            (w, h, data, Some(rows))
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
            (FALLBACK_W, FALLBACK_H, &FALLBACK_BGRA[..], None)
        };
        let source_frame = SourceFrame {
            width: source_width,
            height: source_height,
            stride: checked_stride(source_width)?,
            pixels: source_pixels,
            rows: source_rows,
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
        let animation_driver = AnimationDriver::new();
        let _ = debug_trace("dbg: windowd init animation-driver ok");
        let pipeline_timer = PipelineTimer::new();
        let _ = debug_trace("dbg: windowd init pipeline-timer ok");
        // The on-demand atlas pool: NO legacy shell-chrome bands (topbar/
        // sidepanel/dropdown/chat/search — DELETED, the DSL shell app-host owns
        // that UI). Bands are acquired on demand: the DESKTOP surface + the
        // floating app window allocate from the free pool; a closed window
        // costs zero rows.
        let mut atlas = crate::atlas::AtlasAllocator::new();

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
            pending_damage_rect: None,
            pending_cursor_rect: None,
            last_paced_flush_ns: 0,
            pending_gpu_blit_rect: None,
            paint_only_damage: false,
            telemetry: crate::telemetry::WindowdDisplayTelemetry::default(),
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
            present_retry_count: 0,
            present_retry_exhausted: false,
            app_present_reject_markers: 0,
            app_present_markers: 0,
            desktop_dirty_rows: (u32::MAX, 0),
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
            shape_cache_pushed: false,
            theme_mode: crate::theme::ThemeMode::Dark,
            session_probe: session::SessionProbe::default(),
            theme_probe: shell::ThemeProbe::default(),
            greeter_login_watch: false,
            greeter_watch_next_ns: 0,
            apps: core::array::from_fn(AppWindowSlot::new),
            client_surfaces: crate::client_surface::ClientSurfaces::new(),
            #[cfg(nexus_env = "os")]
            event_channels: [(0, 0); 8],
            #[cfg(nexus_env = "os")]
            event_channels_len: 0,
            desktop_surface_id: None,
            #[cfg(nexus_env = "os")]
            desktop_channel: None,
            desktop_band: None,
            desktop_dirty: false,
            desktop_frame_pulse: false,
            shell_app_launched: false,
            windowing_policy: crate::surface_presentation::WindowingPolicy::Desktop,
            #[cfg(nexus_env = "os")]
            abilitymgr_client: None,
            atlas_alloc: atlas,
            pending_input: None,
            desktop_layers: [nexus_display_proto::client_surface::LayerDesc::default();
                nexus_display_proto::client_surface::MAX_SURFACE_LAYERS],
            desktop_layer_count: 0,
            hover_route: HOVER_ROUTE_NONE,
            hover_last: (0, 0),
            hover_app_idx: 0,
            wheel_route_count: 0,
            drag_transform_marker: false,
            cursor_wait_n: 0,
            cursor_wait_deadline_ns: 0,
            cursor_ring_frame: 0,
            cursor_ring_active: false,
            chrome_cache: chrome_widget::ChromeCache::new(),
            wheel_stage_count: 0,
            hover_marker_emitted: false,
            hover_last_move_ns: 0,
            shell_config,
            // Registration order = initial stacking (later on top once shown);
            // both start hidden, mirroring the ShellWindow `visible` flags above.
            windows: crate::window_scene::WindowStack::new(&[
                crate::window_scene::WindowId::App(0),
                crate::window_scene::WindowId::App(1),
                crate::window_scene::WindowId::App(2),
                crate::window_scene::WindowId::App(3),
                // The desktop base (shell/greeter app-host). Registered hidden;
                // shown + composited once a desktop-level client surface connects
                // (2b-render / 2c session-gate). DESKTOP z-band → always bottom.
                crate::window_scene::WindowId::Desktop,
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

    /// The app-window slot behind a floating window id (`None` for the
    /// desktop / non-app ids). The single id→slot authority — every
    /// interaction/scene/WM path resolves through here.
    pub(crate) fn app_slot(&self, id: crate::window_scene::WindowId) -> Option<&AppWindowSlot> {
        match id {
            crate::window_scene::WindowId::App(i) => self.apps.get(i as usize),
            crate::window_scene::WindowId::Desktop => None,
        }
    }

    /// Mutable [`Self::app_slot`].
    pub(crate) fn app_slot_mut(
        &mut self,
        id: crate::window_scene::WindowId,
    ) -> Option<&mut AppWindowSlot> {
        match id {
            crate::window_scene::WindowId::App(i) => self.apps.get_mut(i as usize),
            crate::window_scene::WindowId::Desktop => None,
        }
    }

    /// Slot index currently bound to `surface_id` (present/input routing).
    pub(crate) fn app_index_by_surface(&self, surface_id: u32) -> Option<usize> {
        self.apps.iter().position(|a| a.surface_id == Some(surface_id))
    }

    /// A free slot for a NEW app window (no bound surface).
    pub(crate) fn free_app_index(&self) -> Option<usize> {
        self.apps.iter().position(|a| a.surface_id.is_none())
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
        self.observer_state.hover_visible |= self.state.hover_visible;
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
        // The CPU glass caches are DELETED (GPU path composites live); the
        // seam stays for the mode-switch call site until the Plane-1 CPU
        // path retires with the evidence-contract move.
    }

    // ── Window stack sync (TASK-0070 Phase 1) ──
    // The `windows` stack mirrors each ShellWindow's `visible` flag and owns
    // z/focus. Every visibility change goes through these helpers so the two
    // can never drift; raises emit honest markers + damage the affected rects.

    /// Stable short name for markers/diagnostics.
    pub(super) fn window_name(id: crate::window_scene::WindowId) -> &'static str {
        match id {
            crate::window_scene::WindowId::App(0) => "app0",
            crate::window_scene::WindowId::App(1) => "app1",
            crate::window_scene::WindowId::App(2) => "app2",
            crate::window_scene::WindowId::App(_) => "app3",
            crate::window_scene::WindowId::Desktop => "desktop",
        }
    }

    /// The on-screen damage rect (incl. shadow halo) of a stack window.
    pub(super) fn window_damage_rect(&self, id: crate::window_scene::WindowId) -> DamageRect {
        // The desktop surface is the full-screen base layer — its damage is the
        // whole display (it is not a chrome `ShellWindow`).
        match self.app_slot(id) {
            Some(slot) => slot.win.damage_rect(self.mode.width, self.mode.height),
            // The desktop surface is the full-screen base layer.
            None => DamageRect { x: 0, y: 0, width: self.mode.width, height: self.mode.height },
        }
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
        if let Some(slot) = self.app_slot_mut(id) {
            slot.win.leave_fullscreen();
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
