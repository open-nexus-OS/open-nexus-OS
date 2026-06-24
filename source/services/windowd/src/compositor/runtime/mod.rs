// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Display server runtime state machine for the windowd compositor:
//! retained-mode compositing, tile damage tracking, input routing, cursor management,
//! and present scheduling.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 13 unit tests (QEMU) + host smoke integration

use super::backdrop::{blur_backdrop_segment, saturate_bgra_segment};
use super::blur::checked_stride;
use super::cache::{
    BackdropCacheEntry, GlassLayerCache, LayerCache, PathCacheEntry, ShadowBoxCacheEntry,
};
use super::damage::cursor_damage_rect;
use super::emit_windowd_telemetry;
use super::filter::{
    build_live_proof_layouts, filter_layout_variant_index, filter_list_content_height,
    filter_list_viewport_height, refill_filtered_words,
};
use super::primitives::draw_line_segment_row;
use super::scene::copy_scene_row;
use super::sdf::{fill_sdf_rounded_rect_row, stroke_sdf_rounded_rect_row};
use super::source::build_scale_lut;
use super::tile_map::TileMap;
use super::types::{
    FixedDebugLine, ProofBoxRect, ProofCard, ProofPaintPart, ProofPaintRole, RenderClip,
    SourceFrame,
};
use super::{
    BACKDROP_CACHE_ENTRIES, BACKDROP_CACHE_MAX_WIDTH, BLUR_CACHE_ROW_OFFSET,
    BUTTON_BLUR_CACHE_ABS_ROW, BUTTON_BLUR_CACHE_ABS_X, CHAT_SHADOW_ALPHA, CHAT_SHADOW_BLUR,
    CHAT_SHADOW_OFFSET_Y, COL_SCRATCH_SIZE, COMBINED_PANEL_WIDTH, DARK_GLASS_BLUR_RADIUS,
    DARK_GLASS_SATURATION_PERCENT, DISPLAY_HEIGHT, DISPLAY_OFFSET_BYTES, DISPLAY_ROW_OFFSET,
    DISPLAY_WIDTH, GLASS_LAYER_MAX_BYTES, IPC_BATCH_LIMIT, LAYER_CACHE_MAX_BYTES,
    LAYER_CACHE_MAX_LAYER_BYTES, LIVE_FILTER_VARIANTS, PATH_CACHE_ENTRIES, PATH_CACHE_MAX_PIXELS,
    PROOF_PANEL_H, RETAINED_OFFSET_BYTES, RETAINED_ROW_OFFSET, ROUTE_NAME, ROW_WRITE_CHUNK,
    SCENE_ORIGIN_X, SCENE_ORIGIN_Y, SHADOW_BOX_CACHE_ENTRIES,
    SIDEBAR_REST_X, USE_DESKTOP_SHELL, WINDOWD_SHADOW_ARENA_SIZE,
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
use nexus_abi::{cap_clone, debug_println, nsec, vmo_write, Handle};
use nexus_effects::ShadowArena;
use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::{CommandBuffer, PipelineTimer, RenderPassDesc, TileRect};
use nexus_ipc::{Client as _, KernelClient, Wait};
use nexus_layout::LayoutResult;
use nexus_layout_types::{FxPx, PathPoint};
use chat_app::ChatMessageProvider;
use nexus_virtual_list::{VirtualList, VirtualListConfig};

const GPU_ANIMATION_SUBMIT_OP: u8 = 1;
const GPU_SET_FRAMEBUFFER_VMO_OP: u8 = 3; // mirrors gpud::OP_SET_FRAMEBUFFER_VMO
const GPU_PRESENT_DAMAGE_OP: u8 = 4; // mirrors gpud::OP_PRESENT_DAMAGE
const GPU_MOVE_CURSOR_OP: u8 = 2; // mirrors gpud::OP_MOVE_CURSOR
const GPU_UPLOAD_CURSOR_OP: u8 = 5; // mirrors gpud::OP_UPLOAD_CURSOR
const GPU_SET_CHAT_SCROLL_OP: u8 = 6; // mirrors gpud::OP_SET_CHAT_SCROLL
const GPU_UPLOAD_ICON_OP: u8 = 7; // mirrors gpud::OP_UPLOAD_ICON
const GPUD_STATUS_OK: u8 = 0;
/// Extra chat content rows rendered above/below the on-screen viewport so scroll
/// is a GPU composite offset, not a CPU re-render. Re-render only on overscan
/// exhaustion (recenter ±CHAT_OVERSCAN/2). Larger ⇒ fewer full-surface re-renders
/// during a fast flick (less VMO-write load → less heap pressure) AND more
/// rendered runway in BOTH directions (smoother up-scroll, which crosses the
/// window most). Bounded so the atlas (chat 600+this, blur 600, sidebar 800) fits
/// the 3200-row VMO atlas: 1600+600+800 = 3000 ≤ 3200.
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
mod scroll;
mod present;
mod scene;

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
    let mut frame = [0u8; 17];
    frame[0] = GPU_PRESENT_DAMAGE_OP;
    frame[1..5].copy_from_slice(&rect.x.to_le_bytes());
    frame[5..9].copy_from_slice(&rect.y.to_le_bytes());
    frame[9..13].copy_from_slice(&rect.width.to_le_bytes());
    frame[13..17].copy_from_slice(&rect.height.to_le_bytes());
    frame
}

fn encode_gpud_attach_frame(handoff_id: u32) -> [u8; 5] {
    let mut frame = [0u8; 5];
    frame[0] = GPU_SET_FRAMEBUFFER_VMO_OP;
    frame[1..5].copy_from_slice(&handoff_id.to_le_bytes());
    frame
}

fn decode_gpud_handoff_id(reply: &[u8]) -> Option<u32> {
    if reply.len() < 5 {
        return None;
    }
    Some(u32::from_le_bytes([reply[1], reply[2], reply[3], reply[4]]))
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

fn draw_animation_proof_overlay_row(
    row: &mut [u8],
    y: u32,
    mode: VisibleBootstrapMode,
    scene: AnimatedSceneState,
) {
    let button_alpha = (96.0 + 80.0 * scene.hover_opacity).clamp(0.0, 220.0) as u8;
    // SSOT: the rendered button rect is exactly the rect windowd hit-tests.
    let bh = crate::interaction::button_rect(mode.width);
    let button_rect = ProofBoxRect { x: bh.x, y: bh.y, width: bh.width, height: bh.height };
    let gt = crate::assets::GLASS_TINT;
    let ge = crate::assets::GLASS_EDGE;
    draw_floating_glass_rect_row(
        row,
        y,
        button_rect,
        GLASS_BUTTON_RADIUS,
        [gt.r, gt.g, gt.b, button_alpha],
        [ge.r, ge.g, ge.b, ge.a],
        14,
        8,
        6,
        32,
    );
    let menu_icon_x =
        button_rect.x.saturating_add((button_rect.width.saturating_sub(LUCIDE_ICON_SIZE)) / 2);
    let menu_icon_y =
        button_rect.y.saturating_add((button_rect.height.saturating_sub(LUCIDE_ICON_SIZE)) / 2);
    let menu_icon_alpha = (152.0 + 92.0 * scene.hover_opacity).clamp(120.0, 244.0) as u8;
    draw_lucide_menu_icon_row(
        row,
        y,
        menu_icon_x,
        menu_icon_y,
        LUCIDE_ICON_SIZE,
        [255, 255, 255, menu_icon_alpha],
    );

    let sidebar_alpha = (220.0 * scene.sidebar_opacity).clamp(0.0, 220.0) as u8;
    if sidebar_alpha == 0 {
        return;
    }
    // SSOT: the rendered sidebar rect is exactly the rect windowd hit-tests.
    let sh = crate::interaction::sidebar_rect(mode, scene.sidebar_translate_x);
    let sidebar_rect = ProofBoxRect { x: sh.x, y: sh.y, width: sh.width, height: sh.height };
    let gt = crate::assets::GLASS_TINT;
    let ge = crate::assets::GLASS_EDGE;
    draw_floating_glass_rect_row(
        row,
        y,
        sidebar_rect,
        SIDEBAR_RADIUS,
        [gt.r, gt.g, gt.b, sidebar_alpha],
        [ge.r, ge.g, ge.b, ge.a],
        20,
        10,
        8,
        34,
    );

    // SSOT: the rendered close icon rect is exactly the close hit target.
    let close = crate::interaction::sidebar_close_icon_rect(
        mode,
        crate::interaction::HitRect {
            x: sidebar_rect.x,
            y: sidebar_rect.y,
            width: sidebar_rect.width,
            height: sidebar_rect.height,
        },
    );
    draw_lucide_x_icon_row(
        row,
        y,
        close.x,
        close.y,
        LUCIDE_ICON_SIZE,
        [255, 255, 255, sidebar_alpha.saturating_add(40)],
    );
}

fn draw_floating_glass_rect_row(
    row: &mut [u8],
    y: u32,
    rect: ProofBoxRect,
    radius: u32,
    tint: [u8; 4],
    border: [u8; 4],
    blur_radius: u32,
    shadow_dx: u32,
    shadow_dy: u32,
    shadow_alpha: u8,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let rect_end_y = rect.y.saturating_add(rect.height);
    if y >= rect.y.saturating_add(shadow_dy) && y < rect_end_y.saturating_add(shadow_dy) {
        blend_span(row, rect.x.saturating_add(shadow_dx), rect.width, [0, 0, 0, shadow_alpha]);
    }
    if y < rect.y || y >= rect_end_y {
        return;
    }
    // Phase A1: CPU blur removed — GPU BlurBackdrop handles glass blur.
    // Per-frame CommandBuffer contains BlurBackdrop for the combined panel region.
    // sdf fill and stroke below provide the glass container coloring.
    let _ = fill_sdf_rounded_rect_row(y, row, rect, radius, tint);
    let _ = stroke_sdf_rounded_rect_row(y, row, rect, radius, 1, border);
}

fn draw_lucide_menu_icon_row(row: &mut [u8], y: u32, x: u32, top: u32, size: u32, color: [u8; 4]) {
    let _ = draw_line_segment_row(
        y,
        row,
        x,
        top,
        size,
        size,
        PathPoint::new(160, 220),
        PathPoint::new(840, 220),
        color,
    );
    let _ = draw_line_segment_row(
        y,
        row,
        x,
        top,
        size,
        size,
        PathPoint::new(160, 500),
        PathPoint::new(840, 500),
        color,
    );
    let _ = draw_line_segment_row(
        y,
        row,
        x,
        top,
        size,
        size,
        PathPoint::new(160, 780),
        PathPoint::new(840, 780),
        color,
    );
}

fn draw_lucide_x_icon_row(row: &mut [u8], y: u32, x: u32, top: u32, size: u32, color: [u8; 4]) {
    let _ = draw_line_segment_row(
        y,
        row,
        x,
        top,
        size,
        size,
        PathPoint::new(220, 220),
        PathPoint::new(780, 780),
        color,
    );
    let _ = draw_line_segment_row(
        y,
        row,
        x,
        top,
        size,
        size,
        PathPoint::new(780, 220),
        PathPoint::new(220, 780),
        color,
    );
}

fn blend_span(row: &mut [u8], x: u32, width: u32, color: [u8; 4]) {
    let max_px = (row.len() / 4) as u32;
    let end = x.saturating_add(width).min(max_px);
    let mut px = x.min(max_px);
    while px < end {
        blend_pixel(row, px, color);
        px += 1;
    }
}

fn blend_pixel(row: &mut [u8], x: u32, color: [u8; 4]) {
    let idx = x as usize * 4;
    if idx + 3 >= row.len() {
        return;
    }
    let alpha = u16::from(color[3]);
    let inv = 255u16.saturating_sub(alpha);
    row[idx] = ((u16::from(color[0]) * alpha + u16::from(row[idx]) * inv) / 255) as u8;
    row[idx + 1] = ((u16::from(color[1]) * alpha + u16::from(row[idx + 1]) * inv) / 255) as u8;
    row[idx + 2] = ((u16::from(color[2]) * alpha + u16::from(row[idx + 2]) * inv) / 255) as u8;
    row[idx + 3] = 0xff;
}

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
    /// Shadow compositing row buffer (zero-copy — allocated once at startup).
    shadow_scratch: Vec<u8>,
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
    /// Fixed storage for per-box shadow rendering. `ShadowArena` borrows this
    /// slice per flush and never owns or grows a Vec internally.
    shadow_arena_buf: Vec<u8>,
    /// Persisted bump offset so cached shadow slices survive partial flushes.
    shadow_arena_used: usize,
    /// Pre-allocated column buffer for 2D blur vertical pass.
    col_scratch: Vec<u8>,
    /// Per-box shadow cache (fixed-size, zero heap alloc).
    shadow_box_cache: [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES],
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
    proof_layouts: Option<Vec<LayoutResult>>,
    proof_layout_index: Option<LayoutHotPathIndex>,
    filtered_words: Vec<&'static str>,
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
    /// Topbar "Apps" dropdown: open state, hovered row, atlas + render dirty.
    apps_dropdown_open: bool,
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
    /// source of truth for scroll *physics* (Apple-style eased momentum via
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
        let mut filtered_words = Vec::with_capacity(crate::proof_panel_spec::FILTER_WORDS.len());
        refill_filtered_words(&mut filtered_words, initial_state.text_input());
        // Shell-P2b: source the composited scene from the desktop shell when
        // enabled, else the baked proof+filter panel. The desktop scene is laid
        // out to fit between the scene insets on both sides of the display.
        let proof_layouts = if USE_DESKTOP_SHELL {
            let content_width = mode.width.saturating_sub(2 * SCENE_ORIGIN_X).max(1);
            crate::desktop_scene::build_live_desktop_layouts(content_width)
        } else {
            build_live_proof_layouts(initial_state)
        };
        let proof_layout_index =
            proof_layouts.as_ref().and_then(|layouts| layouts.first()).map(|layout| {
                LayoutHotPathIndex::build(
                    layout,
                    SCENE_ORIGIN_X,
                    SCENE_ORIGIN_Y,
                    mode.width,
                    mode.height,
                )
            });
        let _ = debug_println(RUNTIME_INIT_OK);
        let _ = debug_println("dbg: windowd init self-build start");
        let band_scratch = alloc::vec![0u8; mode.stride as usize * ROW_WRITE_CHUNK];
        let _ = debug_println("dbg: windowd init band-scratch ok");
        let shadow_scratch = alloc::vec![0u8; mode.stride as usize];
        let _ = debug_println("dbg: windowd init shadow-scratch ok");
        let blur_row_buf = alloc::vec![0u8; mode.stride as usize];
        let _ = debug_println("dbg: windowd init blur-row ok");
        let layer_cache = LayerCache::default();
        let _ = debug_println("dbg: windowd init layer-cache ok");
        let shadow_arena_buf = alloc::vec![0u8; WINDOWD_SHADOW_ARENA_SIZE];
        let _ = debug_println("dbg: windowd init shadow-arena ok");
        let col_scratch = alloc::vec![0u8; COL_SCRATCH_SIZE];
        let _ = debug_println("dbg: windowd init col-scratch ok");
        let backdrop_cache = core::array::from_fn(|_| BackdropCacheEntry::new());
        let _ = debug_println("dbg: windowd init backdrop-cache ok");
        let glass_layer = GlassLayerCache::new();
        let _ = debug_println("dbg: windowd init glass-layer ok");
        let glass_scratch = alloc::vec![0u8; GLASS_LAYER_MAX_BYTES];
        let _ = debug_println("dbg: windowd init glass-scratch ok");
        let path_cache = core::array::from_fn(|_| PathCacheEntry::new());
        let _ = debug_println("dbg: windowd init path-cache ok");
        let animation_driver = AnimationDriver::new();
        let _ = debug_println("dbg: windowd init animation-driver ok");
        let pipeline_timer = PipelineTimer::new();
        let _ = debug_println("dbg: windowd init pipeline-timer ok");
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
        // Glass side panel surface — narrow, tall. Capped so a contiguous tail is
        // left for the on-demand window pool (content + blur cache); without this
        // reserve the panel's "take the rest" would starve a later search show.
        const WINDOW_POOL_ROWS: u32 = 2 * super::desktop_layer::search_full_h() + 16;
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
        chat.mount(chat_atlas, chat_blur_cache);
        // RFC-0065: the desktop starts clean — chat is NOT auto-shown. It opens on
        // demand (chat button / "Chat" in the Apps dropdown via `toggle_chat`),
        // the first visible step away from a baked-open window toward a launched app.
        chat.visible = false;
        let _ = debug_println("dbg: windowd init chat hidden ok");
        Ok(Self {
            mode,
            source_frame,
            source_x_lut,
            source_y_lut,
            cursor_width,
            cursor_height,
            framebuffer: None,
            band_scratch,
            shadow_scratch,
            blur_row_buf,
            state: initial_state,
            observer_state: initial_state,
            markers_emitted: false,
            input_markers_emitted: InputMarkerState::default(),
            input_state_debug_emitted: false,
            pending_damage_rects: Vec::new(),
            tile_map: TileMap::new(),
            layer_cache,
            shadow_arena_buf,
            shadow_arena_used: 0,
            col_scratch,
            shadow_box_cache: [ShadowBoxCacheEntry::empty(); SHADOW_BOX_CACHE_ENTRIES],
            pending_damage_rect: None,
            pending_cursor_rect: None,
            pending_gpu_blit_rect: None,
            sidebar_blur_cache_valid: false,
            button_blur_cache_valid: false,
            paint_only_damage: false,
            proof_layouts,
            proof_layout_index,
            filtered_words,
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
            apps_dropdown_open: false,
            dropdown_hover: None,
            dropdown_atlas,
            dropdown_h,
            dropdown_surface_dirty: true,
            app_menu,
            app_menu_fetched: false,
            search,
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
            pending_chat_wheel: 0,
            pending_input: None,
            chat_content_h,
            chat_visible,
            chat_render_base: 0,
            chat_drag_marker_emitted: false,
            chat_button_marker_emitted: false,
            shell_config,
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
        self.shadow_arena_used = 0;
        for entry in &mut self.shadow_box_cache {
            entry.valid = false;
        }
        for entry in &mut self.backdrop_cache {
            entry.valid = false;
        }
        self.glass_layer.valid = false;
    }

}
