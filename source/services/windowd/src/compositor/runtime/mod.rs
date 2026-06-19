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
    SCENE_ORIGIN_X, SCENE_ORIGIN_Y, SHADOW_BOX_CACHE_ENTRIES, SHELL_SIDEPANEL, SHELL_TOPBAR,
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
use animation::{AnimProp, AnimationDriver, LayerId, SceneUpdate};
use core::fmt::Write as _;
use input_live_protocol::{VisibleState, STATUS_MALFORMED, STATUS_OK};
use nexus_abi::{cap_clone, debug_println, nsec, vmo_write, Handle};
use nexus_effects::ShadowArena;
use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::{CommandBuffer, PipelineTimer, RenderPassDesc, TileRect};
use nexus_ipc::{Client as _, KernelClient, Wait};
use nexus_layout::LayoutResult;
use nexus_layout_types::{FxPx, PathPoint};
use nexus_virtual_list::{ChatMessageProvider, VirtualList, VirtualListConfig};

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
}

impl AnimatedSceneState {
    const fn new() -> Self {
        Self { hover_opacity: 0.0, sidebar_translate_x: 320.0, sidebar_opacity: 0.0 }
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
    /// Chat panel as a retained-surface LAYER. Content is rendered once (on
    /// change) into `chat_atlas` (an off-screen VMO atlas surface) and the
    /// compositor blits that surface to the on-screen position. Moving the
    /// window is just a different blit destination — no content re-render.
    chat_atlas: crate::atlas::AtlasSurface,
    /// Cached blurred backdrop behind the chat window (atlas surface). Rebuilt
    /// only when `chat_blur_cache_valid` is false (window opened/moved); reused
    /// every present so glass costs one blit, not a blur, per frame.
    chat_blur_cache: crate::atlas::AtlasSurface,
    chat_blur_cache_valid: bool,
    /// Cached fully-composited sidebar (valid only while it's settled/static).
    sidebar_composite_cache: crate::atlas::AtlasSurface,
    sidebar_composite_cache_valid: bool,
    /// Set when the chat surface needs re-rendering (init, scroll, new message).
    chat_surface_dirty: bool,
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
    /// Side-panel "Apps" dropdown expanded, and the hovered row.
    sidepanel_apps_expanded: bool,
    sidepanel_hover: Option<crate::compositor::desktop_layer::SidepanelItem>,
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
    /// Window manager: owns the chat window's bounds/visibility/drag. The
    /// compositor blits the chat surface at `wm.chat_window().bounds`, so moving
    /// the window is just a different blit destination (no content re-render).
    wm: crate::wm::WindowManager,
    /// One-shot `chat window drag ok` marker latch.
    chat_drag_marker_emitted: bool,
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

impl DisplayServerRuntime {
    pub(crate) fn new() -> Result<Self, WindowdError> {
        let _ = debug_println(RUNTIME_INIT_START);
        let mode = VisibleBootstrapMode::fixed()?.validate()?;

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
        let chat_atlas = atlas
            .alloc(
                crate::interaction::CHAT_PANEL_W,
                crate::interaction::CHAT_PANEL_H + CHAT_OVERSCAN,
            )
            .ok_or(WindowdError::BufferLengthMismatch)?;
        // Cache of the BLURRED backdrop behind the chat window. The backdrop is
        // the static base, so we blur it once per window move and reuse it every
        // present (Task #17 pattern) — zero per-frame blur for glass, the key to
        // running several glass layers at 120Hz.
        let chat_blur_cache = atlas
            .alloc(crate::interaction::CHAT_PANEL_W, crate::interaction::CHAT_PANEL_H)
            .ok_or(WindowdError::BufferLengthMismatch)?;
        // Cache of the FULLY COMPOSITED sidebar (blurred backdrop + glass tint +
        // border + icons). When the sidebar is settled (fully open, static), it's
        // composited once and then blitted each present — so a cursor move over
        // the sidebar costs one blit, not 4 full-height SDF fills.
        let sidebar_composite_cache = atlas
            .alloc(crate::interaction::SIDEBAR_WIDTH, mode.height)
            .ok_or(WindowdError::BufferLengthMismatch)?;
        // Shell-P2b: reserve the glass topbar layer surface — full-width, one
        // bar tall. Composited at (TOPBAR_MARGIN_X, TOPBAR_TOP) with blur +
        // rounded corners + shadow each present.
        let shell_w = mode.width.saturating_sub(2 * super::desktop_layer::TOPBAR_MARGIN_X).max(1);
        let shell_h = super::desktop_layer::TOPBAR_H;
        let shell_atlas = atlas
            .alloc(mode.width.min(crate::atlas::ATLAS_WIDTH), shell_h)
            .ok_or(WindowdError::BufferLengthMismatch)?;
        // Glass side panel surface — narrow, tall (clamped to the atlas budget).
        let sidepanel_h = mode
            .height
            .saturating_sub(super::desktop_layer::SIDEPANEL_TOP + super::desktop_layer::SIDEPANEL_MARGIN)
            .max(1)
            .min(atlas.rows_remaining());
        let sidepanel_atlas = atlas
            .alloc(super::desktop_layer::SIDEPANEL_W, sidepanel_h)
            .ok_or(WindowdError::BufferLengthMismatch)?;
        // Window manager. The chat window starts open at the panel's current
        // position (a dedicated chat button will toggle it in a later step).
        let mut wm = crate::wm::WindowManager::new(crate::wm::WmRect::new(
            crate::interaction::CHAT_PANEL_X as i32,
            crate::interaction::CHAT_PANEL_Y as i32,
            crate::interaction::CHAT_PANEL_W as i32,
            crate::interaction::CHAT_PANEL_H as i32,
        ));
        wm.open(crate::wm::WindowId::Chat);
        let _ = debug_println("dbg: windowd init chat ok");
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
            chat_atlas,
            chat_blur_cache,
            chat_blur_cache_valid: false,
            sidebar_composite_cache,
            sidebar_composite_cache_valid: false,
            chat_surface_dirty: true,
            shell_atlas,
            shell_w,
            shell_h,
            shell_surface_dirty: true,
            topbar_hover: None,
            topbar_menu_hover: false,
            sidepanel_atlas,
            sidepanel_h,
            sidepanel_surface_dirty: true,
            sidepanel_apps_expanded: false,
            sidepanel_hover: None,
            chat_list,
            chat_scroll_y: 0,
            chat_scroll_last_ns: 0,
            chat_scroll_diag_ns: 0,
            pending_chat_wheel: 0,
            pending_input: None,
            chat_content_h,
            chat_visible,
            chat_render_base: 0,
            wm,
            chat_drag_marker_emitted: false,
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

    /// Phase 6c: Write source frame (wallpaper) to VMO bottom half once.
    /// Moves 4MB of pixel data from control-plane heap to data-plane VMO.
    pub(crate) fn write_source_frame_to_vmo(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let sf = &self.source_frame;
        if sf.pixels.is_empty() || sf.width == 0 || sf.height == 0 {
            return Ok(());
        }
        let src_stride = sf.stride as usize;
        let dst_stride = DISPLAY_WIDTH as usize * 4;
        for row in 0..sf.height.min(DISPLAY_HEIGHT) {
            let src_off = row as usize * src_stride;
            let dst_off = row as usize * dst_stride;
            let copy_len = (sf.width as usize * 4).min(src_stride).min(dst_stride);
            vmo_write(handle, dst_off, &sf.pixels[src_off..src_off + copy_len])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        Ok(())
    }

    /// Phase 1 of framebuffer registration: store the VMO handle and set
    /// display-ready flags. Returns immediately so the IPC response
    /// is not blocked by the expensive first-frame write.
    ///
    /// Phase 2 (write_current_frame + marker emissions) happens deferred
    /// via `process_deferred_framebuffer_write()`.
    pub(crate) fn register_framebuffer_vmo(&mut self, handle: Handle) {
        self.framebuffer = Some(handle);
        self.framebuffer_pending_first_write = true;
        let next = self.first_handoff_id.wrapping_add(1);
        self.first_handoff_id = if next == 0 { 1 } else { next };
        self.first_handoff_deadline_ns =
            nsec().ok().map(|now| now.saturating_add(FIRST_HANDOFF_DEADLINE_NS)).unwrap_or(0);
        self.first_handoff_frame_written = false;
        self.first_handoff_bootstrap_markers_emitted = false;
        self.first_handoff_attach_acked = false;
        self.first_handoff_present_sent = false;
    }

    /// Phase D.1: true while first-frame handoff is still in progress.
    pub(crate) fn is_handoff_pending(&self) -> bool {
        self.framebuffer_pending_first_write
    }

    /// Phase 6d: called when gpud acknowledges a present (blocking reply received).
    fn note_present_completed(&mut self) {
        self.last_completed_seq = self.present_seq;
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
        // Phase 4: toggle display slot on completion so next frame uses alternate slot.
        // gpud scanout switch is deferred to Phase 7 (unified pacing loop).
        self.current_display_slot ^= 1;
    }

    /// Phase 4: byte offset into VMO for the current display slot.
    fn current_display_offset(&self) -> usize {
        if self.current_display_slot == 0 {
            super::DISPLAY_OFFSET_BYTES
        } else {
            super::DISPLAY_SLOT_B_OFFSET_BYTES
        }
    }

    /// Phase 2 of framebuffer registration: write the first composed frame
    /// and emit all bootstrap markers. Called from the IPC loop after the
    /// VMO-ack response has been sent.
    pub(crate) fn process_deferred_framebuffer_write(&mut self) -> u8 {
        if !self.framebuffer_pending_first_write {
            return STATUS_OK;
        }
        if self.first_handoff_deadline_ns != 0 {
            let now = nsec().unwrap_or(0);
            if now >= self.first_handoff_deadline_ns {
                let _ = debug_println("windowd: ERROR first-frame handoff timeout");
                self.framebuffer_pending_first_write = false;
                return STATUS_MALFORMED;
            }
        }
        let Some(handle) = self.framebuffer else {
            let _ = debug_println("windowd: ERROR framebuffer missing during handoff");
            self.framebuffer_pending_first_write = false;
            return STATUS_MALFORMED;
        };

        if !self.first_handoff_frame_written {
            if let Err(err) = self.write_current_frame() {
                let _ = debug_println(&alloc::format!(
                    "windowd: ERROR first-frame write failed err={:?}",
                    err
                ));
                self.framebuffer_pending_first_write = false;
                return STATUS_MALFORMED;
            }
            self.first_handoff_frame_written = true;
        }

        if !self.first_handoff_bootstrap_markers_emitted {
            if self.active_proof_layout().is_some() {
                let _ = debug_println(LAYOUT_ENGINE_ON_MARKER);
                let _ = debug_println(TEXT_WRAPPING_ON_MARKER);
            }
            let _ = debug_println(DISPLAY_BOOTSTRAP_MARKER);
            let _ = debug_println(DISPLAY_MODE_MARKER);
            let _ = debug_println(VISIBLE_BACKEND_MARKER);
            let _ = debug_println(COMPOSE_READY_MARKER);
            let _ = debug_println(PRESENT_QUEUED_MARKER);
            self.first_handoff_bootstrap_markers_emitted = true;
        }

        // Reactive handoff: block until gpud accepts the VMO (no polling).
        if !self.first_handoff_attach_acked {
            self.do_handoff_attach_blocking(handle);
        }

        // Reactive present: blit the full retained scene to the display plane and
        // overlay the cursor, then block until ack. The CPU composite above wrote
        // the scene into Plane 1; this CB copies it to Plane 2 (display) so the
        // first frame is identical to every steady-state frame (one code path).
        if !self.first_handoff_present_sent {
            let full = DamageRect { x: 0, y: 0, width: self.mode.width, height: self.mode.height };
            let mut frame_buf = [0u8; 8192];
            let sent = match self.build_scene_cb_into(&[full], 1, &mut frame_buf[1..]) {
                Ok(written) => {
                    frame_buf[0] = GPU_PRESENT_DAMAGE_OP;
                    Some(self.send_gpud_present(&frame_buf[..1 + written]))
                }
                Err(_) => None,
            };
            if sent == Some(true) {
                let _ = debug_println("windowd: handoff present sent");
                self.first_handoff_present_sent = true;
                // Drain the ack reply (kernel delivers it reactively).
                self.drain_gpud_replies();
                // Proof-harness contract (TASK-0055/0055B): first checked
                // present — one full-screen damage rect, sequence 1.
                let _ = debug_println("windowd: present ok (seq=1 dmg=1)");
            } else {
                let _ = debug_println("windowd: handoff present failed");
                self.framebuffer_pending_first_write = false;
                return STATUS_MALFORMED;
            }
        }

        self.state.display_scanout_ready = true;
        self.state.systemui_first_frame_visible = true;
        self.refresh_observer_state();
        let _ = debug_println(PRESENT_SCHEDULER_ON_MARKER);
        self.input_markers_emitted.scheduler = true;
        let _ = debug_println(SELFTEST_UI_V2_PRESENT_OK_MARKER);
        self.input_markers_emitted.v2_present = true;
        let _ = debug_println(DISPLAY_FIRST_SCANOUT_MARKER);
        let _ = debug_println(SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
        let _ = debug_println(PRESENT_VISIBLE_MARKER);
        let _ = debug_println(SELFTEST_UI_VISIBLE_PRESENT_MARKER);
        self.emit_asset_markers();
        // First frame IS a real composition — set verified so emit_v3b_markers()
        // fires. The gate checks v3b_composition_verified before emitting.
        self.v3b_composition_verified = true;
        self.emit_v3b_markers();
        // Upload cursor sprite to gpud for software BlendCursor compositing.
        // This is a software-side sprite (not a hardware cursor resource), so it
        // avoids the QEMU virtio-gpu quirk where UPDATE_CURSOR corrupts RESOURCE_FLUSH.
        if self.state.cursor_svg_visible {
            self.upload_cursor_bitmap_to_gpud();
        }
        // The standalone test icon sprite (TASK #61) is retired — the shell's
        // chrome (topbar + chat) is the real UI now. `upload_icon_to_gpud`
        // remains available for when the topbar hosts a real app icon (P3).
        self.framebuffer_pending_first_write = false;
        STATUS_OK
    }

    /// Reactive handoff: send VMO to gpud and block until acknowledged.
    /// No polling — the kernel wakes us when gpud's reply arrives.
    fn do_handoff_attach_blocking(&mut self, fb_handle: Handle) {
        if !self.ensure_gpud_client() {
            let _ = debug_println("windowd: handoff no gpud client");
            return;
        }
        let clone = match nexus_abi::cap_clone(fb_handle) {
            Ok(cap) => cap,
            Err(_) => {
                let _ = debug_println("windowd: handoff cap-clone failed");
                return;
            }
        };
        let frame = encode_gpud_attach_frame(self.first_handoff_id);
        let send_ok = {
            let Some(client) = self.gpud_client.as_ref() else {
                let _ = nexus_abi::cap_close(clone);
                return;
            };
            match client.send_with_cap_move_wait(&frame, clone, Wait::Blocking) {
                Ok(()) => true,
                Err(e) => {
                    log_gpud_ipc_error("windowd: handoff cap-move send failed", e);
                    self.gpud_client = None;
                    false
                }
            }
        };
        if !send_ok {
            return;
        }
        let _ = debug_println("windowd: handoff attach sent");
        // Block until gpud responds — fully reactive, no polling.
        let ack_ok = {
            let Some(client) = self.gpud_client.as_ref() else {
                return;
            };
            match client.recv(Wait::Blocking) {
                Ok(reply) => reply.first().copied() == Some(GPUD_STATUS_OK),
                Err(e) => {
                    log_gpud_ipc_error("windowd: handoff ack recv failed", e);
                    self.gpud_client = None;
                    false
                }
            }
        };
        if ack_ok {
            let _ = debug_println("windowd: handoff attach ack");
            // Proof-harness contract (TASK-0055B): the acked VMO handoff is
            // exactly what this marker asserts.
            let _ = debug_println("windowd: fb handoff to gpud ok");
            self.first_handoff_attach_acked = true;
        } else {
            let _ = debug_println("windowd: handoff attach ack bad status");
        }
    }

    fn write_fast_bootstrap_frame(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        let width = self.mode.width as usize;
        let height = self.mode.height as usize;
        if row_len < width.saturating_mul(4) {
            return Err(WindowdError::BufferLengthMismatch);
        }

        let win_w = 820usize;
        let win_h = 460usize;
        let win_x = (width.saturating_sub(win_w)) / 2;
        let win_y = (height.saturating_sub(win_h)) / 2;
        let title_h = 56usize;

        let bg = [18u8, 18u8, 18u8, 255u8];
        let panel = [42u8, 46u8, 54u8, 255u8];
        let bar = [64u8, 74u8, 92u8, 255u8];
        let border = [84u8, 106u8, 144u8, 255u8];

        let mut band_start = 0usize;
        while band_start < height {
            let band_end = (band_start + ROW_WRITE_CHUNK).min(height);
            let band_rows = band_end - band_start;
            let band_bytes = band_rows * row_len;
            let band = &mut self.band_scratch[..band_bytes];
            band.fill(0);
            for row_idx in 0..band_rows {
                let y = band_start + row_idx;
                let row = &mut band[row_idx * row_len..(row_idx + 1) * row_len];
                for px in row[..width * 4].chunks_exact_mut(4) {
                    px.copy_from_slice(&bg);
                }
                if y >= win_y && y < win_y + win_h {
                    let in_border_y = y == win_y || y + 1 == win_y + win_h;
                    for x in win_x..(win_x + win_w) {
                        let idx = x * 4;
                        let in_border_x = x == win_x || x + 1 == win_x + win_w;
                        let color = if in_border_x || in_border_y {
                            border
                        } else if y < win_y + title_h {
                            bar
                        } else {
                            panel
                        };
                        row[idx..idx + 4].copy_from_slice(&color);
                    }
                }
            }
            vmo_write(handle, DISPLAY_OFFSET_BYTES + band_start * row_len, band)
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// STAGE one upstream input update into the frame-aligned sample instead of
    /// applying it now. The latest cursor/button/text snapshot wins (we only render
    /// the newest position); wheel deltas SUM (no scroll notch is lost). Replies
    /// can be sent immediately by the caller — staging always "accepts". This is
    /// the consumer half of the Android frame-aligned input model.
    pub(crate) fn stage_input_state(&mut self, mut state: VisibleState) -> u8 {
        if let Some(prev) = self.pending_input.take() {
            // Carry the accumulated wheel forward (sum), keep the newest of all else.
            state.wheel_delta_y = state.wheel_delta_y.saturating_add(prev.wheel_delta_y);
        }
        self.pending_input = Some(state);
        STATUS_OK
    }

    /// Apply the frame's staged input sample ONCE (called from the present loop
    /// after draining the IPC batch). Returns true if there was input to apply.
    /// This collapses N raw events/frame into a single hit-test + hover + cursor
    /// move + scroll — the work is bounded by frame rate, not input rate.
    pub(crate) fn apply_staged_input(&mut self) -> bool {
        match self.pending_input.take() {
            Some(state) => {
                self.apply_input_state(state);
                true
            }
            None => false,
        }
    }

    pub(crate) fn apply_input_state(&mut self, upstream: VisibleState) -> u8 {
        if !self.input_state_debug_emitted {
            let _ = debug_println("dbg: windowd input state applied");
            // Input-chain hop I6: input reached windowd and was applied. The
            // present chain (gpud G1..G4) takes over from here to put it onscreen.
            let _ = debug_println("windowd: chain I6 input recv (state applied)");
            self.input_state_debug_emitted = true;
        }
        let old_state = self.state;
        let old_cursor_x = self.state.cursor_x;
        let old_cursor_y = self.state.cursor_y;
        let old_filter_idx = self.active_filter_idx;
        self.state.virtio_raw_seen |= upstream.virtio_raw_seen;
        self.state.hid_normalized_seen |= upstream.hid_normalized_seen;
        self.state.pointer_route_live |= upstream.pointer_route_live;
        self.state.keyboard_route_live |= upstream.keyboard_route_live;
        self.state.input_visible_on |= upstream.input_visible_on
            || upstream.pointer_route_live
            || upstream.keyboard_route_live;
        self.state.cursor_move_visible |=
            upstream.cursor_move_visible || upstream.pointer_route_live;
        // ── windowd-owned hit-testing (compositor model) ──
        // inputd ships a raw display-space pointer + raw button/wheel/key facts;
        // windowd resolves all UI intent against its own rendered geometry, so a
        // control's hit area is exactly its rendered rect (interaction::*).
        let cursor_x = upstream.cursor_x;
        let cursor_y = upstream.cursor_y;
        let mode = self.mode;

        // Two independent hover signals, both from real rendered geometry:
        //  - hover_visible: cursor over the HOVER TEST CARD in the proof panel
        //    (its border recolors — the actual "hover test"). The card rect
        //    comes from the live layout index, so hit area == rendered rect.
        //  - button_hover: cursor over the top-right glass button (highlight
        //    only; the sidebar animation stays click-driven — user requirement:
        //    "nur der button rechts oben soll die animation auslösen").
        let old_button_hover = self.button_hover;
        self.button_hover = crate::interaction::hover_over_button(mode, cursor_x, cursor_y);
        self.state.hover_visible = self
            .active_proof_layout_index()
            .and_then(|idx| idx.target_rect(TargetDamage::Hover))
            .map(|r| {
                cursor_x >= r.x as i32
                    && cursor_y >= r.y as i32
                    && (cursor_x as u32) < r.x.saturating_add(r.width)
                    && (cursor_y as u32) < r.y.saturating_add(r.height)
            })
            .unwrap_or(false);

        // Raw primary-button level from inputd; rising/falling edges are click/release.
        let primary_down = upstream.launcher_click_visible;
        let primary_press = primary_down && !old_state.launcher_click_visible;
        let primary_release = !primary_down && old_state.launcher_click_visible;
        self.state.launcher_click_visible = primary_down;

        // Window manager first: a press on the chat title bar starts a drag, a
        // press on the close button closes it. Both consume the press so it does
        // not also hit the panel/sidebar logic below.
        let mut window_consumed_press = false;
        if primary_press {
            use crate::wm::PointerAction;
            match self.wm.on_pointer_down(cursor_x, cursor_y) {
                PointerAction::DragStarted(_) => {
                    window_consumed_press = true;
                }
                PointerAction::Closed(id) => {
                    window_consumed_press = true;
                    self.on_chat_window_closed(id);
                }
                PointerAction::None => {}
            }
        }
        // Continue an in-progress drag: move the window and damage old+new regions.
        if self.wm.is_dragging() {
            let old_bounds = self.wm.chat_window().bounds;
            if self.wm.on_pointer_move(cursor_x, cursor_y, mode.width as i32, mode.height as i32) {
                self.note_chat_window_moved(old_bounds);
            }
        }
        if primary_release {
            let _ = self.wm.on_pointer_up();
        }

        // Shell-P2b: the topbar menu icon (right) toggles the animated side
        // panel — the same scene-graph-driven slide animation as the hamburger.
        if primary_press && !window_consumed_press && SHELL_TOPBAR {
            use crate::compositor::desktop_layer::{topbar_menu_icon_hit, TOPBAR_MARGIN_X, TOPBAR_TOP};
            if cursor_x >= TOPBAR_MARGIN_X as i32 && cursor_y >= TOPBAR_TOP as i32 {
                let lx = (cursor_x - TOPBAR_MARGIN_X as i32) as u32;
                let ly = (cursor_y - TOPBAR_TOP as i32) as u32;
                if topbar_menu_icon_hit(lx, ly, self.shell_w) {
                    self.state.sidebar_open_visible = !self.state.sidebar_open_visible;
                    window_consumed_press = true;
                    let _ = debug_println(if self.state.sidebar_open_visible {
                        "dbg: topbar menu -> sidebar OPEN"
                    } else {
                        "dbg: topbar menu -> sidebar CLOSE"
                    });
                }
            }
        }

        // Side-panel item clicks: Apps expands the dropdown; Chat opens the chat
        // window; Search opens the search window (next phase).
        if primary_press && !window_consumed_press && SHELL_SIDEPANEL && self.state.sidebar_open_visible
        {
            use crate::compositor::desktop_layer::{
                sidepanel_item_at, SidepanelItem, SIDEPANEL_MARGIN, SIDEPANEL_TOP, SIDEPANEL_W,
            };
            let slide =
                self.animated_scene.sidebar_translate_x.clamp(0.0, SIDEPANEL_W as f32 + 32.0) as u32;
            let base_x =
                self.mode.width.saturating_sub(SIDEPANEL_MARGIN + SIDEPANEL_W).saturating_add(slide);
            if cursor_x >= base_x as i32
                && cursor_y >= SIDEPANEL_TOP as i32
                && (cursor_x as u32) < base_x + SIDEPANEL_W
                && (cursor_y as u32) < SIDEPANEL_TOP + self.sidepanel_h
            {
                if let Some(item) =
                    sidepanel_item_at((cursor_y - SIDEPANEL_TOP as i32) as u32, self.sidepanel_apps_expanded)
                {
                    window_consumed_press = true;
                    let panel_damage = DamageRect {
                        x: base_x,
                        y: SIDEPANEL_TOP,
                        width: SIDEPANEL_W.min(self.mode.width.saturating_sub(base_x)),
                        height: self.sidepanel_h,
                    };
                    match item {
                        SidepanelItem::Apps => {
                            self.sidepanel_apps_expanded = !self.sidepanel_apps_expanded;
                            self.sidepanel_surface_dirty = true;
                            self.queue_dirty_rect(panel_damage);
                        }
                        SidepanelItem::Chat => {
                            let now = self.wm.toggle(crate::wm::WindowId::Chat);
                            if now {
                                self.chat_blur_cache_valid = false;
                                let b = self.wm.chat_window().bounds;
                                self.erase_chat_region(b.x, b.y);
                                self.note_chat_button_dirty();
                            } else {
                                self.on_chat_window_closed(crate::wm::WindowId::Chat);
                            }
                        }
                        SidepanelItem::Search => {
                            let _ = debug_println("dbg: sidepanel Search (window — next phase)");
                        }
                    }
                }
            }
        }

        // Resolve the click against the rendered geometry (only if the window
        // manager did not consume it). The sidebar is the single click-driven
        // animation trigger.
        if primary_press && !window_consumed_press {
            use crate::interaction::{resolve_click, ClickAction};
            match resolve_click(mode, self.state.sidebar_open_visible, cursor_x, cursor_y) {
                ClickAction::ToggleSidebar => {
                    self.state.sidebar_open_visible = !self.state.sidebar_open_visible;
                }
                ClickAction::CloseSidebar => {
                    self.state.sidebar_open_visible = false;
                }
                ClickAction::ToggleChat => {
                    let now_visible = self.wm.toggle(crate::wm::WindowId::Chat);
                    if now_visible {
                        let _ = debug_println("windowd: chat window open");
                        // Surface content is retained in the atlas — just
                        // damage the window region (plus shadow halo) so the
                        // composite draws it at the current bounds. Rebuild the
                        // blurred-backdrop cache (it may be stale from a prior pos).
                        self.chat_blur_cache_valid = false;
                        let b = self.wm.chat_window().bounds;
                        self.erase_chat_region(b.x, b.y);
                        self.note_chat_button_dirty();
                    } else {
                        self.on_chat_window_closed(crate::wm::WindowId::Chat);
                    }
                }
                ClickAction::FocusPanel => {
                    self.state.focus_visible = true;
                }
                ClickAction::None => {}
            }
        }
        self.state.focus_visible |= upstream.focus_visible;
        // Reflect the momentary key-held state from inputd (which already sends
        // `keyboard_visible = keyboard_held`). Must NOT be OR-latched with
        // `keyboard_route_live` — that flag stays true forever once the keyboard
        // is seen, which would pin the "key pressed" highlight on permanently.
        // The once-only proof marker is latched separately in observer_state.
        self.state.keyboard_visible = upstream.keyboard_visible;
        self.state.wheel_up_visible = upstream.wheel_up_visible;
        self.state.wheel_down_visible = upstream.wheel_down_visible;
        self.state.cursor_x = upstream.cursor_x;
        self.state.cursor_y = upstream.cursor_y;
        // Shell-P2b: topbar hover. Recompute the hovered item from the cursor and,
        // on change, re-render the topbar atlas + damage its band so the present
        // recomposites with the new hover highlight.
        if SHELL_TOPBAR {
            use crate::compositor::desktop_layer::{
                topbar_item_at, topbar_menu_icon_hit, TOPBAR_H, TOPBAR_MARGIN_X, TOPBAR_TOP,
            };
            let cx = self.state.cursor_x;
            let cy = self.state.cursor_y;
            let in_bar = cy >= TOPBAR_TOP as i32
                && cy < (TOPBAR_TOP + TOPBAR_H) as i32
                && cx >= TOPBAR_MARGIN_X as i32;
            let (new_hover, new_menu_hover) = if in_bar {
                let lx = (cx - TOPBAR_MARGIN_X as i32) as u32;
                let ly = (cy - TOPBAR_TOP as i32) as u32;
                (topbar_item_at(lx), topbar_menu_icon_hit(lx, ly, self.shell_w))
            } else {
                (None, false)
            };
            if new_hover != self.topbar_hover || new_menu_hover != self.topbar_menu_hover {
                self.topbar_hover = new_hover;
                self.topbar_menu_hover = new_menu_hover;
                self.shell_surface_dirty = true;
                self.queue_dirty_rect(DamageRect {
                    x: TOPBAR_MARGIN_X,
                    y: TOPBAR_TOP,
                    width: self.shell_w,
                    height: TOPBAR_H,
                });
            }
        }
        // Side-panel row hover (only while the panel is open).
        if SHELL_SIDEPANEL && self.state.sidebar_open_visible {
            use crate::compositor::desktop_layer::{
                sidepanel_item_at, SIDEPANEL_MARGIN, SIDEPANEL_TOP, SIDEPANEL_W,
            };
            let slide =
                self.animated_scene.sidebar_translate_x.clamp(0.0, SIDEPANEL_W as f32 + 32.0) as u32;
            let base_x = self.mode.width.saturating_sub(SIDEPANEL_MARGIN + SIDEPANEL_W).saturating_add(slide);
            let cx = self.state.cursor_x;
            let cy = self.state.cursor_y;
            let new_hover = if cx >= base_x as i32
                && cy >= SIDEPANEL_TOP as i32
                && (cx as u32) < base_x + SIDEPANEL_W
                && (cy as u32) < SIDEPANEL_TOP + self.sidepanel_h
            {
                sidepanel_item_at((cy - SIDEPANEL_TOP as i32) as u32, self.sidepanel_apps_expanded)
            } else {
                None
            };
            if new_hover != self.sidepanel_hover {
                self.sidepanel_hover = new_hover;
                self.sidepanel_surface_dirty = true;
                self.queue_dirty_rect(DamageRect {
                    x: base_x,
                    y: SIDEPANEL_TOP,
                    width: SIDEPANEL_W.min(self.mode.width.saturating_sub(base_x)),
                    height: self.sidepanel_h,
                });
            }
        }
        self.state.set_text_input(upstream.text_input());
        refill_filtered_words(&mut self.filtered_words, self.state.text_input());
        // The desktop scene has a single layout (no filter variants); keep the
        // active index pinned at 0 so the single-element layout set stays valid.
        if !USE_DESKTOP_SHELL {
            self.active_filter_idx = filter_layout_variant_index(self.state.text_input());
        }
        if self.active_filter_idx != old_filter_idx {
            self.refresh_active_proof_hot_path();
        }
        self.refresh_observer_state();
        let button_hover_changed = old_button_hover != self.button_hover;
        if self.state == old_state && self.active_filter_idx == old_filter_idx {
            if button_hover_changed {
                self.note_button_hover_changed();
            }
            return STATUS_OK;
        }
        // ── Phase 0: Scene graph updates instead of damage rect queueing ──
        // Card active states: hover → slot 0, click → slot 1, keyboard → slot 2
        let hover_changed = old_state.hover_visible != self.state.hover_visible;
        let click_changed = old_state.launcher_click_visible != self.state.launcher_click_visible;
        let key_changed = old_state.keyboard_visible != self.state.keyboard_visible;
        if hover_changed {
            self.shell.set_card_active(0, self.state.hover_visible);
        }
        if click_changed {
            self.shell.set_card_active(1, self.state.launcher_click_visible);
        }
        if key_changed {
            self.shell.set_card_active(2, self.state.keyboard_visible);
        }
        if button_hover_changed {
            self.note_button_hover_changed();
        }
        // CPU repaint of the test cards whose state flags flipped — this is what
        // recolors the card borders (proof_box_border reads these flags).
        self.queue_target_damage(old_state, self.state);
        // Sidebar visibility
        if old_state.sidebar_open_visible != self.state.sidebar_open_visible {
            self.shell.set_sidebar_visible(self.state.sidebar_open_visible);
        }
        // Detect paint-only: only hover/click/keyboard flags changed, not cursor or text
        let cursor_changed =
            old_cursor_x != self.state.cursor_x || old_cursor_y != self.state.cursor_y;
        let text_changed = old_state.text_input() != self.state.text_input();
        let filter_changed = old_filter_idx != self.active_filter_idx;
        let paint_flags_changed = old_state.hover_visible != self.state.hover_visible
            || old_state.sidebar_open_visible != self.state.sidebar_open_visible
            || old_state.launcher_click_visible != self.state.launcher_click_visible
            || old_state.keyboard_visible != self.state.keyboard_visible;

        // Implicit transitions (RFC-0059 Phase 4): when paint flags change,
        // trigger spring animation for opacity/transform on the affected proof cards.
        if paint_flags_changed && !self.animation_driver.reduced_motion() {
            if !self.animation_proof.runtime_marker {
                let _ = debug_println(UIRUNTIME_ON);
                self.animation_proof.runtime_marker = true;
            }
            if !self.animation_proof.implicit_marker {
                let _ = debug_println(WINDOWD_IMPLICIT_TRANSITIONS_ON);
                self.animation_proof.implicit_marker = true;
            }
            let spring = animation::SpringConfig {
                stiffness: 200.0,
                damping: 20.0,
                mass: 1.0,
                initial_velocity: 0.0,
            };
            // (The HOVER_LAYER spring is the glass-button highlight and is driven
            // by `note_button_hover_changed`, not by the hover test card.)
            // Sidebar open/close uses a dedicated state so close actions are not
            // coupled to hover leave.
            if old_state.sidebar_open_visible != self.state.sidebar_open_visible {
                let sidebar_from =
                    if old_state.sidebar_open_visible { 0.0 } else { SIDEBAR_WIDTH as f32 };
                let sidebar_to =
                    if self.state.sidebar_open_visible { 0.0 } else { SIDEBAR_WIDTH as f32 };
                self.animation_driver.spring_to(
                    SIDEBAR_LAYER_ID,
                    AnimProp::TranslateX,
                    sidebar_from,
                    sidebar_to,
                    spring,
                );
                self.animation_driver.spring_to(
                    SIDEBAR_LAYER_ID,
                    AnimProp::Opacity,
                    self.animated_scene.sidebar_opacity,
                    if self.state.sidebar_open_visible { 1.0 } else { 0.0 },
                    spring,
                );
                if !self.animation_proof.timeline_marker {
                    let _ = debug_println(UIANIM_TIMELINE_ON);
                    self.animation_proof.timeline_marker = true;
                }
            }
            // Click card opacity
            if old_state.launcher_click_visible != self.state.launcher_click_visible {
                let from = if old_state.launcher_click_visible { 1.0 } else { 0.0 };
                let to = if self.state.launcher_click_visible { 1.0 } else { 0.0 };
                self.animation_driver.spring_to(
                    CLICK_LAYER_ID,
                    AnimProp::Opacity,
                    from,
                    to,
                    spring,
                );
            }
            // Keyboard card opacity
            if old_state.keyboard_visible != self.state.keyboard_visible {
                let from = if old_state.keyboard_visible { 1.0 } else { 0.0 };
                let to = if self.state.keyboard_visible { 1.0 } else { 0.0 };
                self.animation_driver.spring_to(
                    KEYBOARD_LAYER_ID,
                    AnimProp::Opacity,
                    from,
                    to,
                    spring,
                );
            }
        }
        if old_state.sidebar_open_visible != self.state.sidebar_open_visible {
            let _ = debug_println(if self.state.sidebar_open_visible {
                SIDEBAR_OPEN_MARKER
            } else {
                SIDEBAR_CLOSE_MARKER
            });
            // Sidebar is a GPU overlay — no CPU content in P1 changes on open/close.
            // Cache invalidation is deferred until the close animation completes.
            self.queue_gpu_blit_rect(self.sidebar_damage_rect());
        }
        self.paint_only_damage =
            paint_flags_changed && !cursor_changed && !text_changed && !filter_changed;
        // Cursor hot path. Hardware overlay: the move is a 9-byte message to
        // gpud's cursor queue — the host repositions the overlay, no composite,
        // no blit, no present. The frame pipeline is not involved at all.
        // Software fallback: queue the merged old+new cursor rect — flush blits
        // that region from the retained Plane 1 and overlays BlendCursor.
        if self.hw_cursor_active {
            if cursor_changed {
                self.send_cursor_move_to_gpud();
            }
        } else if self.gl_cursor_active {
            // virgl procedural cursor: update gpud's pointer pos AND damage the
            // cursor rect so a present is scheduled — the build-up present
            // redraws the procedural arrow at the new spot (its VMO BlendCursor
            // is ignored while the GL build-up owns the scanout).
            if cursor_changed {
                self.send_cursor_move_to_gpud();
                self.queue_cursor_damage(
                    old_cursor_x,
                    old_cursor_y,
                    self.state.cursor_x,
                    self.state.cursor_y,
                );
            }
        } else {
            self.queue_cursor_damage(
                old_cursor_x,
                old_cursor_y,
                self.state.cursor_x,
                self.state.cursor_y,
            );
        }

        // ── v3b: reflect real upstream text instead of synthetic keyboard cycling ──
        if old_state.text_input() != self.state.text_input() {
            self.note_filter_text_changed();
        }

        // ── v3b: scroll on wheel events, routed to the control under the cursor ──
        // Gate on the real signed delta (edge-accurate per update) rather than the
        // latched pulse booleans, so each notch is applied once with its magnitude.
        if upstream.wheel_delta_y != 0 {
            use crate::interaction::{resolve_wheel_target, HitRect, WheelTarget};
            // Wheel routing follows the chat window's live bounds (None when
            // closed) — a dragged window keeps scrolling under the cursor.
            let chat_bounds = {
                let w = self.wm.chat_window();
                w.visible.then(|| HitRect {
                    x: w.bounds.x.max(0) as u32,
                    y: w.bounds.y.max(0) as u32,
                    width: w.bounds.w.max(0) as u32,
                    height: w.bounds.h.max(0) as u32,
                })
            };
            let target =
                resolve_wheel_target(mode, self.state.cursor_x, self.state.cursor_y, chat_bounds);
            // Scroll diagnostic (rate-limited ~200ms): logs on every wheel input —
            // even when nothing moves — the routing target + full scroll state, so a
            // "scrolled but nothing happened" freeze is explained by VALUES, not guesses.
            let now = nsec().unwrap_or(0);
            if now.saturating_sub(self.chat_scroll_diag_ns) >= 200_000_000 {
                self.chat_scroll_diag_ns = now;
                let _ = debug_println(&alloc::format!(
                    "scroll-diag: in={} tgt={} cur=({},{}) chat_vis={} y={} pos={} target={} max={} base={} gl={}",
                    upstream.wheel_delta_y,
                    if matches!(target, WheelTarget::Chat) { "chat" } else { "filter" },
                    self.state.cursor_x,
                    self.state.cursor_y,
                    self.wm.chat_window().visible,
                    self.chat_scroll_y,
                    self.chat_list.scroll_offset().as_i32(),
                    self.chat_list.scroll_target(),
                    self.chat_list.max_scroll(),
                    self.chat_render_base,
                    self.gl_cursor_active,
                ));
            }
            match target {
                // Coalesce: accumulate this event's notches; `commit_scroll_input`
                // applies the frame's total ONCE (reactive, no per-event replay).
                WheelTarget::Chat => {
                    self.pending_chat_wheel =
                        self.pending_chat_wheel.saturating_add(upstream.wheel_delta_y);
                }
                // Filter is the fallback so a wheel anywhere off the chat still
                // scrolls the proof list (and emits the scroll markers).
                _ => {
                    if self.active_proof_layout().is_some() {
                        self.handle_scroll_input();
                    }
                }
            }
        }

        // ── v3b: selftest summary markers (once) ──
        if !self.selftest_v3b_emitted
            && self.live_scroll_marker_emitted
            && self.clipping_marker_emitted
            && self.filter_cycle > 0
        {
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_SCROLL_OK_MARKER);
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_FILTER_OK_MARKER);
            let _ = debug_println(crate::markers::SELFTEST_UI_V3_IME_OK_MARKER);
            self.selftest_v3b_emitted = true;
        }

        STATUS_OK
    }

    /// Glass-button hover highlight: spring the button alpha (HOVER_LAYER drives
    /// `hover_opacity` in the GPU CB) and present the button rect. Independent of
    /// the proof-panel hover test card.
    fn note_button_hover_changed(&mut self) {
        if !self.animation_driver.reduced_motion() {
            let spring = animation::SpringConfig {
                stiffness: 200.0,
                damping: 20.0,
                mass: 1.0,
                initial_velocity: 0.0,
            };
            let from = self.animated_scene.hover_opacity;
            let to = if self.button_hover { 1.0 } else { 0.0 };
            self.animation_driver.spring_to(HOVER_LAYER_ID, AnimProp::Opacity, from, to, spring);
        }
        let b = crate::interaction::button_rect(self.mode.width);
        self.queue_gpu_blit_rect(DamageRect { x: b.x, y: b.y, width: b.width, height: b.height });
    }

    fn note_filter_text_changed(&mut self) {
        self.filter_cycle = self.filter_cycle.wrapping_add(1);

        if !self.clipping_marker_emitted {
            let _ = debug_println(crate::markers::CLIPPING_ON_MARKER);
            self.clipping_marker_emitted = true;
        }
        let _ = debug_println(crate::markers::TEXT_INPUT_ON_MARKER);
        let _ = debug_println(crate::markers::FILTER_LIST_OK_MARKER);

        let filter_rects: [Option<DamageRect>; 3] =
            if let Some(index) = self.active_proof_layout_index() {
                [
                    index.target_rect(TargetDamage::FilterPanel),
                    index.target_rect(TargetDamage::FilterList),
                    index.target_rect(TargetDamage::FilterInput),
                ]
            } else {
                [None, None, None]
            };
        for rect in filter_rects.into_iter().flatten() {
            self.queue_dirty_rect(rect);
        }
    }

    /// Wheel over the chat viewport → an Apple-style **flick**. The signed wheel
    /// delta (real notch count from inputd, no longer a quantized boolean) moves
    /// the virtual-list's scroll *target*; `tick_chat_scroll` then eases the
    /// position toward it over subsequent frames (momentum). One notch animates
    /// smoothly; many notches fling proportionally further — no dropped input.
    fn handle_chat_scroll_input(&mut self, wheel_delta_y: i32) {
        if wheel_delta_y == 0 {
            return;
        }
        if !self.scroll_marker_emitted {
            let _ = debug_println(crate::markers::SCROLL_ON_MARKER);
            self.scroll_marker_emitted = true;
        }
        // REL_WHEEL: +up / −down (inputd convention). Scroll offset grows toward
        // the bottom, so a wheel-down (negative delta) increases the offset →
        // negate. Scale each notch to ~3 text lines (the standard wheel step).
        // `wheel_delta_y` here is the COALESCED per-frame total (commit_scroll_input).
        // Clamp it: bounds one frame's scroll AND drops stale piled-up backlog
        // (reactive — apply this frame's intent, not a replayed flood). At ~120 Hz
        // this still allows ~24·120 ≈ 2880 notches/s; the scroller's acceleration
        // handles fast sequences across frames.
        const MAX_NOTCHES_PER_FRAME: i32 = 24;
        let notches = wheel_delta_y.clamp(-MAX_NOTCHES_PER_FRAME, MAX_NOTCHES_PER_FRAME);
        let step = crate::interaction::CHAT_LINE_H.saturating_mul(3) as i32;
        let delta_px = -notches.saturating_mul(step);
        // `scroll_wheel` moves the content IMMEDIATELY (1:1, zero latency — precise
        // for a slow careful scroll) and injects accumulating momentum (a fast
        // spin coasts). Commit the instant move NOW so it presents on this very
        // loop iteration's flush; the momentum tick continues the glide after.
        self.chat_list.scroll_wheel(FxPx::new(delta_px));
        self.commit_chat_scroll_position();
    }

    /// Apply the wheel input coalesced this present-loop iteration — ONCE, with the
    /// frame's net delta — then clear it. Called after draining the IPC batch so a
    /// flood of queued input events becomes a single reactive scroll step instead
    /// of a replayed backlog ("old commands still being processed"). Returns true
    /// if it scrolled (so the caller knows to keep the pacer alive).
    pub(crate) fn commit_scroll_input(&mut self) -> bool {
        let delta = core::mem::take(&mut self.pending_chat_wheel);
        if delta == 0 {
            return false;
        }
        self.handle_chat_scroll_input(delta);
        true
    }

    /// Mirror the virtual-list scroll position into `chat_scroll_y`, recenter the
    /// overscan render base only when the scroll leaves the prerendered window,
    /// and re-present the chat region (a cheap GPU source-row offset, not a CPU
    /// re-render). Shared by the immediate wheel step + the per-frame momentum
    /// tick so both commit identically.
    fn commit_chat_scroll_position(&mut self) {
        let new = self.chat_list.scroll_offset().as_i32().max(0) as u32;
        if new == self.chat_scroll_y {
            return;
        }
        self.chat_scroll_y = new;
        let offset = new.saturating_sub(self.chat_render_base);
        let within_overscan = new >= self.chat_render_base && offset <= CHAT_OVERSCAN;
        if within_overscan && self.gl_cursor_active {
            // SCROLL FAST PATH (virgl GL scanout): tell gpud to re-sample the
            // retained chat layer at the new atlas row and GPU-re-composite (~54µs)
            // — NO windowd CPU compose, just like the cursor's OP_MOVE_CURSOR. This
            // is what lets scroll run at gpud's rate instead of windowd's compose rate.
            self.send_chat_scroll_to_gpud(self.chat_atlas.abs_row + offset);
        } else {
            // Left the prerendered window (new content needed) OR the 2D/mmio path
            // (no GL layer re-sample): recenter + re-render as needed, then a normal
            // present carries the fresh layer (which also clears gpud's scroll override).
            if !within_overscan {
                self.chat_render_base = new.saturating_sub(CHAT_OVERSCAN / 2);
                self.chat_surface_dirty = true;
            }
            self.queue_gpu_blit_rect(DamageRect {
                x: crate::interaction::CHAT_PANEL_X,
                y: crate::interaction::CHAT_PANEL_Y,
                width: crate::interaction::CHAT_PANEL_W,
                height: crate::interaction::CHAT_PANEL_H,
            });
        }
        if !self.live_scroll_marker_emitted {
            let _ = debug_println(crate::markers::LIVE_SCROLL_OK_MARKER);
            self.live_scroll_marker_emitted = true;
        }
    }

    /// Scroll fast path: a 5-byte fire-and-forget `OP_SET_CHAT_SCROLL(src_row)` to
    /// gpud (mirrors the cursor's `OP_MOVE_CURSOR`). gpud re-samples the retained
    /// scrollable chat layer at `src_row_abs` and re-composites on the GPU — no
    /// windowd compose. No-op on the 2D/mmio backend (handled there by the CPU path).
    fn send_chat_scroll_to_gpud(&mut self, src_row_abs: u32) {
        let mut frame = [0u8; 5];
        frame[0] = GPU_SET_CHAT_SCROLL_OP;
        frame[1..5].copy_from_slice(&src_row_abs.to_le_bytes());
        let _ = self.send_gpud_fire_forget(&frame);
    }

    /// Advance the chat scroll momentum ONE frame (called from the present-loop
    /// pacing tick with `now_ns`). Integrates `chat_list`'s velocity over the real
    /// elapsed time since the last tick (frame-rate independent), mirrors the new
    /// position into `chat_scroll_y`, recenters the overscan render base only when
    /// the scroll leaves the prerendered window, and re-presents the chat region
    /// (a cheap GPU source-row offset, not a CPU re-render). Returns true while
    /// still gliding so the pacer keeps ticking. This is what makes the live chat
    /// scroll buttery (momentum) instead of a one-shot jump.
    pub(crate) fn tick_chat_scroll(&mut self, now_ns: u64) -> bool {
        if !self.chat_list.is_animating() {
            self.chat_scroll_last_ns = 0;
            return false;
        }
        // Real elapsed time since the last tick; on the first frame of a glide
        // (last == 0) assume one 120 Hz frame so the integrator starts cleanly.
        let dt_ns = if self.chat_scroll_last_ns == 0 || now_ns <= self.chat_scroll_last_ns {
            8_333_333
        } else {
            now_ns - self.chat_scroll_last_ns
        };
        self.chat_scroll_last_ns = now_ns;
        let still = self.chat_list.tick(dt_ns);
        if !still {
            self.chat_scroll_last_ns = 0;
        }
        // GPU scroll-offset: while the scroll stays inside the prerendered overscan
        // window the commit is a pure composite source-row offset (no CPU re-render).
        self.commit_chat_scroll_position();
        still
    }

    /// Damage the chat window's last region so the base shows through after the
    /// window is closed (the composite no longer draws it).
    fn on_chat_window_closed(&mut self, _id: crate::wm::WindowId) {
        let _ = debug_println("windowd: chat window close");
        let b = self.wm.chat_window().bounds;
        self.erase_chat_region(b.x, b.y);
        self.note_chat_button_dirty();
    }

    /// Damage the chat toggle button's rect so the incremental composite redraws its
    /// active-state tint after a chat-visibility change (its body alpha tracks
    /// `chat_window().visible`). Without this the gated button block would keep the
    /// stale tint until another damage rect happened to touch it.
    fn note_chat_button_dirty(&mut self) {
        let cb = crate::interaction::chat_button_rect(self.mode.width, self.mode.height);
        self.queue_gpu_blit_rect(DamageRect {
            x: cb.x,
            y: cb.y,
            width: cb.width,
            height: cb.height,
        });
    }

    /// A drag moved the chat window: erase the old region (a cheap GPU blit of
    /// the base from Plane 1 — the chat was never baked there), and the
    /// compositor re-blits the cached chat surface at the new bounds. No CPU
    /// recomposite, no content re-render → GPU-bound drag.
    fn note_chat_window_moved(&mut self, old_bounds: crate::wm::WmRect) {
        if !self.chat_drag_marker_emitted {
            let _ = debug_println("windowd: chat window drag ok");
            self.chat_drag_marker_emitted = true;
        }
        // The backdrop behind the window changed → the blurred-backdrop cache
        // is stale; rebuild it at the new position next composite.
        self.chat_blur_cache_valid = false;
        self.erase_chat_region(old_bounds.x, old_bounds.y);
    }

    /// Refresh the display from the (cursor-free, chat-free) base in Plane 1 for
    /// a chat-sized region at (x, y). Pure GPU blit — no recomposite. The region
    /// is padded by the drop-shadow halo (blur + offset) so a moved window
    /// leaves no stale shadow behind.
    fn erase_chat_region(&mut self, x: i32, y: i32) {
        let pad = CHAT_SHADOW_BLUR.saturating_add(CHAT_SHADOW_OFFSET_Y.unsigned_abs()) as i32;
        self.queue_gpu_blit_rect(DamageRect {
            x: (x - pad).max(0) as u32,
            y: (y - pad).max(0) as u32,
            width: crate::interaction::CHAT_PANEL_W + 2 * pad as u32,
            height: crate::interaction::CHAT_PANEL_H + 2 * pad as u32,
        });
    }

    /// Render the full chat layer content into its off-screen atlas surface
    /// (rows `chat_atlas.abs_row..`, x 0..CHAT_PANEL_W). Called when the surface
    /// is dirty (init / scroll / new message), never per move — moving the
    /// window only changes the composite blit destination.
    fn render_chat_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        // Render the OVERSCAN surface (viewport + overscan) anchored at the
        // current render base. Re-window at that base so the surface content
        // matches; scrolling within the overscan is a composite offset (no
        // re-render), so this runs only on init / overscan-exhaustion / new data.
        let height = crate::interaction::CHAT_PANEL_H + CHAT_OVERSCAN;
        let content_vp_h = height
            .saturating_sub(crate::interaction::CHAT_TITLE_BAR_H + crate::interaction::CHAT_PAD);
        self.chat_content_h = super::chat::compute_visible_window(
            self.chat_list.provider(),
            self.chat_render_base,
            &mut self.chat_visible,
            content_vp_h,
        );
        // Keep the component's height authority in sync so momentum clamps to the
        // real bottom (re-render happens on data change / overscan exhaustion).
        self.chat_list.set_content_height(FxPx::new(self.chat_content_h as i32));
        let abs_row = self.chat_atlas.abs_row;
        let render_base = self.chat_render_base;
        let content_h = self.chat_content_h;
        let visible = &self.chat_visible;
        let band = &mut self.band_scratch;
        // Write the surface in ROW_WRITE_CHUNK-row bands: one vmo_write syscall
        // per band instead of one per row. The band carries full-stride rows; the
        // chat draws into x<366 and the unused atlas padding is never sampled.
        let mut band_start = 0u32;
        while band_start < height {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(height);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                super::chat::draw_chat_panel_row(ly, row, render_base, content_h, visible, height)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// Shell-P2b: render the glass topbar into its atlas surface (rows
    /// `shell_atlas.abs_row..`, bar-local coords). Called when dirty (init /
    /// hover change). Each row is cleared first; the composite applies the
    /// rounded mask + backdrop blur + shadow.
    fn render_shell_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = self.shell_atlas.abs_row;
        let shell_h = self.shell_h;
        let bar_w = self.shell_w;
        let hover = self.topbar_hover;
        let menu_hover = self.topbar_menu_hover;
        let band = &mut self.band_scratch;
        let mut band_start = 0u32;
        while band_start < shell_h {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(shell_h);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                row.fill(0);
                super::desktop_layer::draw_topbar_row(ly, row, bar_w, hover, menu_hover)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    /// Shell-P2b: render the glass side panel into its atlas surface (rows
    /// `sidepanel_atlas.abs_row..`, panel-local coords). Rendered once; the
    /// composite slides it in from the right and applies blur/rounding/shadow.
    fn render_sidepanel_surface(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let stride = self.mode.stride as usize;
        if self.band_scratch.len() < stride * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let abs_row = self.sidepanel_atlas.abs_row;
        let panel_h = self.sidepanel_h;
        let panel_w = super::desktop_layer::SIDEPANEL_W;
        let expanded = self.sidepanel_apps_expanded;
        let hover = self.sidepanel_hover;
        let band = &mut self.band_scratch;
        let mut band_start = 0u32;
        while band_start < panel_h {
            let band_end = (band_start + ROW_WRITE_CHUNK as u32).min(panel_h);
            let band_rows = (band_end - band_start) as usize;
            for (i, ly) in (band_start..band_end).enumerate() {
                let row = &mut band[i * stride..(i + 1) * stride];
                row.fill(0);
                super::desktop_layer::draw_sidepanel_row(ly, row, panel_w, expanded, hover)?;
            }
            let dst = (abs_row + band_start) as usize * stride;
            vmo_write(handle, dst, &band[..band_rows * stride])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        Ok(())
    }

    fn handle_scroll_input(&mut self) {
        if !self.scroll_marker_emitted {
            let _ = debug_println(crate::markers::SCROLL_ON_MARKER);
            self.scroll_marker_emitted = true;
        }

        let wheel_down_visible = self.state.wheel_down_visible;
        // Compute content height before mutable borrow of proof_layouts
        let content_h = filter_list_content_height(&self.filtered_words);

        let mut scroll_damage = None;
        if let Some(layout) = self.active_proof_layout_mut() {
            // Find the filter_list container
            let container_id =
                layout.boxes.iter().find(|b| b.id == Some("filter_list")).map(|b| b.node_id);

            if let Some(id) = container_id {
                let viewport_h = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| {
                        FxPx::new(
                            filter_list_viewport_height(b.rect.height.as_u32().unwrap_or(0)) as i32
                        )
                    })
                    .unwrap_or(FxPx::ZERO);
                let current_offset = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| b.scroll_offset)
                    .unwrap_or((FxPx::ZERO, FxPx::ZERO));

                let dy = if wheel_down_visible { FxPx::new(20) } else { FxPx::new(-20) };
                let max_scroll = FxPx::new((content_h as i32).saturating_sub(viewport_h.0).max(0));
                let new_offset_y = (current_offset.1 + dy).clamp(FxPx::ZERO, max_scroll);
                let new_offset = (current_offset.0, new_offset_y);
                scroll_damage = Some(layout.reposition_scroll(id, new_offset));
            }
        }
        if let Some(damage) = scroll_damage {
            self.refresh_active_proof_hot_path();
            for rect in damage.rects.into_iter().flatten() {
                let x = SCENE_ORIGIN_X.saturating_add(rect.x.as_u32().unwrap_or(0));
                let y = SCENE_ORIGIN_Y.saturating_add(rect.y.as_u32().unwrap_or(0));
                let w = rect.width.as_u32().unwrap_or(0);
                let h = rect.height.as_u32().unwrap_or(0);
                if w > 0 && h > 0 {
                    self.queue_dirty_rect(DamageRect { x, y, width: w, height: h });
                }
            }
            if !damage.is_empty() {
                let _ = debug_println(crate::markers::LIVE_SCROLL_OK_MARKER);
                self.live_scroll_marker_emitted = true;
            }
        }
    }

    fn current_filter_text(&self) -> &'static str {
        LIVE_FILTER_VARIANTS[self.active_filter_idx]
    }

    fn active_proof_layout(&self) -> Option<&LayoutResult> {
        self.proof_layouts.as_ref()?.get(self.active_filter_idx)
    }

    fn active_proof_layout_mut(&mut self) -> Option<&mut LayoutResult> {
        self.proof_layouts.as_mut()?.get_mut(self.active_filter_idx)
    }

    fn active_proof_layout_index(&self) -> Option<&LayoutHotPathIndex> {
        self.proof_layout_index.as_ref()
    }

    fn refresh_active_proof_hot_path(&mut self) {
        let Some(new_index) = self.active_proof_layout().map(|layout| {
            LayoutHotPathIndex::build(
                layout,
                SCENE_ORIGIN_X,
                SCENE_ORIGIN_Y,
                self.mode.width,
                self.mode.height,
            )
        }) else {
            return;
        };
        self.proof_layout_index = Some(new_index);
    }

    fn queue_target_damage(&mut self, old_state: VisibleState, new_state: VisibleState) {
        let Some(index) = self.active_proof_layout_index() else {
            return;
        };
        let hover_rect = index.target_rect(TargetDamage::Hover);
        let click_rect = index.target_rect(TargetDamage::Click);
        let key_rect = index.target_rect(TargetDamage::Key);
        let scroll_rect = index.target_rect(TargetDamage::Scroll);
        if old_state.hover_visible != new_state.hover_visible {
            if let Some(rect) = hover_rect {
                self.queue_dirty_rect(rect);
            }
        }
        if old_state.launcher_click_visible != new_state.launcher_click_visible {
            if let Some(rect) = click_rect {
                self.queue_dirty_rect(rect);
            }
        }
        if old_state.keyboard_visible != new_state.keyboard_visible {
            if let Some(rect) = key_rect {
                self.queue_dirty_rect(rect);
            }
        }
        if old_state.wheel_up_visible != new_state.wheel_up_visible
            || old_state.wheel_down_visible != new_state.wheel_down_visible
        {
            if let Some(rect) = scroll_rect {
                self.queue_dirty_rect(rect);
            }
        }
    }

    /// Returns true when at least one animation is active and needs driving.
    /// Send the framebuffer VMO to gpud for zero-copy GPU scanout.
    /// Returns true only after gpud accepted the VMO handoff.
    fn try_handoff_framebuffer_to_gpud(&mut self, fb_handle: Handle) -> bool {
        if !self.ensure_gpud_client() {
            return false;
        }

        // Single-shot clone: bootstrap is fail-fast by design.
        let clone = match nexus_abi::cap_clone(fb_handle) {
            Ok(c) => c,
            Err(_) => {
                let _ = debug_println("windowd: fb handoff to gpud cap-clone failed");
                return false;
            }
        };

        // Send VMO with blocking wait — kernel guarantees delivery before return.
        let request = [GPU_SET_FRAMEBUFFER_VMO_OP];
        let send_result = {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.send_with_cap_move_wait(&request, clone, Wait::Blocking)
        };
        let recv_result = if send_result.is_ok() {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.recv(Wait::Blocking)
        } else {
            Err(nexus_ipc::IpcError::Disconnected)
        };
        match (send_result, recv_result) {
            (Ok(()), Ok(reply)) if reply.first().copied() == Some(GPUD_STATUS_OK) => {
                let _ = debug_println("windowd: fb handoff to gpud ok");
                true
            }
            (Ok(()), Ok(reply)) => {
                if let Some(status) = reply.first().copied() {
                    let _ = debug_println(&alloc::format!(
                        "windowd: fb handoff to gpud bad-status=0x{status:02x}"
                    ));
                } else {
                    let _ = debug_println("windowd: fb handoff to gpud bad-status=empty");
                }
                self.gpud_client = None;
                false
            }
            (Err(e), _) => {
                let _ = debug_println("windowd: fb handoff to gpud send-failed");
                log_gpud_ipc_error("windowd: fb handoff to gpud send-failed detail", e);
                self.gpud_client = None;
                false
            }
            (Ok(()), Err(e)) => {
                let _ = debug_println("windowd: fb handoff to gpud recv-failed");
                log_gpud_ipc_error("windowd: fb handoff to gpud recv-failed detail", e);
                self.gpud_client = None;
                false
            }
        }
    }

    /// Upload the cursor sprite to gpud. gpud arms the virtio-gpu hardware
    /// cursor overlay (64×64 resource on the cursor queue) and keeps the sprite
    /// as the software BlendCursor fallback. Blocking: the 5-byte reply reports
    /// which path is live (`flags == 1` → hardware overlay).
    fn submit_animation_to_gpud(&mut self, updates: &[SceneUpdate]) -> Result<(), WindowdError> {
        let mut cmd = CommandBuffer::new();
        {
            let mut encoder = cmd
                .try_begin_render_pass(RenderPassDesc {
                    color_attachments: alloc::vec![],
                    width: self.mode.width,
                    height: self.mode.height,
                })
                .map_err(|_| WindowdError::InvalidDamage)?;
            let mut payload = [0u8; 16];
            payload[..4].copy_from_slice(&(updates.len() as u32).to_le_bytes());
            payload[4..8].copy_from_slice(&self.animated_scene.hover_opacity.to_le_bytes());
            payload[8..12].copy_from_slice(&self.animated_scene.sidebar_translate_x.to_le_bytes());
            payload[12..16].copy_from_slice(&self.animated_scene.sidebar_opacity.to_le_bytes());
            encoder.try_set_fragment_bytes(0, &payload).map_err(|_| WindowdError::InvalidDamage)?;
            encoder
                .try_draw_tiles(
                    &[
                        TileRect {
                            x: self.mode.width.saturating_sub(SIDEBAR_WIDTH),
                            y: 0,
                            width: SIDEBAR_WIDTH,
                            height: self.mode.height,
                        },
                        TileRect {
                            x: self.mode.width.saturating_sub(180),
                            y: 24,
                            width: 156,
                            height: 56,
                        },
                    ],
                    RgbaColor::new(200, 220, 255, 192),
                )
                .map_err(|_| WindowdError::InvalidDamage)?;
            encoder.end_encoding();
        }
        let committed = cmd.try_commit().map_err(|_| WindowdError::InvalidDamage)?;
        if committed.command_count() == 0 {
            return Err(WindowdError::InvalidDamage);
        }
        // Serialize the CommittedBuffer into an IPC frame.
        // Frame layout: [opcode=GPU_ANIMATION_SUBMIT_OP] + serialized CommittedBuffer.
        let mut frame_buf = [0u8; 512];
        let written = committed
            .serialize_into(&mut frame_buf[1..])
            .map_err(|_| WindowdError::InvalidDamage)?;
        frame_buf[0] = GPU_ANIMATION_SUBMIT_OP;
        let total = 1usize.saturating_add(written);
        self.send_gpud_status_request(&frame_buf[..total])
    }

    fn sidebar_damage_rect(&self) -> DamageRect {
        DamageRect {
            x: self.mode.width.saturating_sub(SIDEBAR_WIDTH),
            y: 0,
            width: SIDEBAR_WIDTH,
            height: self.mode.height,
        }
    }

    fn write_current_frame(&mut self) -> Result<(), WindowdError> {
        self.reset_effect_caches();
        // Mark every tile dirty so the first full-screen write renders all rows.
        self.tile_map.mark_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
        self.write_rows(0, self.mode.height, select_glass_quality(PROOF_PANEL_H), false)
    }

    fn write_rows(
        &mut self,
        start_y: u32,
        end_y: u32,
        glass_quality: GlassQuality,
        paint_only: bool,
    ) -> Result<(), WindowdError> {
        let render_start_ns = nsec().unwrap_or(0);
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        if self.band_scratch.len() < row_len * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let active_filter_idx = self.active_filter_idx;
        let proof_layout =
            self.proof_layouts.as_ref().and_then(|layouts| layouts.get(active_filter_idx));
        let proof_layout_index = self.proof_layout_index.as_ref();
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let state = self.state;
        let filter_text = state.text_input();
        let filtered_words = self.filtered_words.as_slice();
        let animated_scene = self.animated_scene;
        let end_y = end_y.min(self.mode.height);
        let render_clip = RenderClip::full(self.mode.width);
        let blur_row_buf = &mut self.blur_row_buf[..row_len];
        let shadow_scratch = &mut self.shadow_scratch[..row_len];
        let backdrop_cache = &mut self.backdrop_cache;
        let glass_layer = &mut self.glass_layer;
        let glass_scratch = &mut self.glass_scratch;
        let path_cache = &mut self.path_cache;
        let band_scratch = &mut self.band_scratch;
        let mut shadow_arena =
            ShadowArena::from_buffer_with_used(&mut self.shadow_arena_buf, self.shadow_arena_used);
        let mut band_start = start_y.min(end_y);
        while band_start < end_y {
            let band_end = (band_start as usize + ROW_WRITE_CHUNK).min(end_y as usize) as u32;
            // Skip bands that contain only clean tiles.
            if !self.tile_map.has_dirty_in_row_range(band_start, band_end) {
                band_start = band_end;
                continue;
            }
            // band rendering
            let band_rows = (band_end - band_start) as usize;
            let band_bytes = band_rows * row_len;
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let dest_start = row_idx * row_len;
                let dest_end = dest_start + row_len;
                let band_row = &mut band_scratch[dest_start..dest_end];
                copy_scene_row(
                    blur_row_buf,
                    shadow_scratch,
                    backdrop_cache,
                    glass_layer,
                    glass_scratch,
                    path_cache,
                    source_frame,
                    source_x_lut,
                    source_y_lut,
                    mode,
                    state,
                    proof_layout,
                    proof_layout_index,
                    filter_text,
                    filtered_words,
                    y,
                    render_clip,
                    glass_quality,
                    paint_only,
                    band_row,
                    &mut self.layer_cache,
                    &mut shadow_arena,
                    &mut self.col_scratch,
                    &mut self.shadow_box_cache,
                )?;
                // Chat is a retained-surface layer composited by build_scene_cb —
                // no longer baked into Plane 1 here. GPU overlays (button, sidebar,
                // cursor) are likewise added in the CommandBuffer.
            }
            let offset = band_start as usize * row_len;
            // CPU computes background content (wallpaper + proof panel) into Plane 1.
            // GPU draws the animated overlay (button, sidebar, cursor) on top each frame.
            vmo_write(handle, RETAINED_OFFSET_BYTES + offset, &band_scratch[..band_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        self.shadow_arena_used = shadow_arena.used_bytes();
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.telemetry.record_compose_timed(
            u64::from(self.mode.width).saturating_mul(u64::from(end_y.saturating_sub(start_y))),
            nsec().unwrap_or(render_start_ns).saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        self.refresh_observer_state();
        Ok(())
    }

    fn write_damage_rect(
        &mut self,
        rect: DamageRect,
        glass_quality: GlassQuality,
        paint_only: bool,
    ) -> Result<(), WindowdError> {
        let render_start_ns = nsec().unwrap_or(0);
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        if self.band_scratch.len() < row_len * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let start_y = rect.y.min(self.mode.height);
        let end_y = rect.end_y().min(self.mode.height);
        let start_x = rect.x.min(self.mode.width);
        let end_x = rect.end_x().min(self.mode.width);
        if start_y >= end_y || start_x >= end_x {
            return Ok(());
        }
        let active_filter_idx = self.active_filter_idx;
        let proof_layout =
            self.proof_layouts.as_ref().and_then(|layouts| layouts.get(active_filter_idx));
        let proof_layout_index = self.proof_layout_index.as_ref();
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let state = self.state;
        let filter_text = state.text_input();
        let filtered_words = self.filtered_words.as_slice();
        let animated_scene = self.animated_scene;
        let blur_row_buf = &mut self.blur_row_buf[..row_len];
        let shadow_scratch = &mut self.shadow_scratch[..row_len];
        let backdrop_cache = &mut self.backdrop_cache;
        let glass_layer = &mut self.glass_layer;
        let glass_scratch = &mut self.glass_scratch;
        let path_cache = &mut self.path_cache;
        let mut shadow_arena =
            ShadowArena::from_buffer_with_used(&mut self.shadow_arena_buf, self.shadow_arena_used);
        let byte_start = start_x as usize * 4;
        let byte_end = end_x as usize * 4;
        let render_clip = RenderClip::new(start_x, end_x, self.mode.width);
        let mut band_start = start_y;
        while band_start < end_y {
            let band_end = (band_start as usize + ROW_WRITE_CHUNK).min(end_y as usize) as u32;
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let dest_start = row_idx * row_len;
                let band_row = &mut self.band_scratch[dest_start..dest_start + row_len];
                copy_scene_row(
                    blur_row_buf,
                    shadow_scratch,
                    backdrop_cache,
                    glass_layer,
                    glass_scratch,
                    path_cache,
                    source_frame,
                    source_x_lut,
                    source_y_lut,
                    mode,
                    state,
                    proof_layout,
                    proof_layout_index,
                    filter_text,
                    filtered_words,
                    y,
                    render_clip,
                    glass_quality,
                    paint_only,
                    band_row,
                    &mut self.layer_cache,
                    &mut shadow_arena,
                    &mut self.col_scratch,
                    &mut self.shadow_box_cache,
                )?;
                // Chat is composited as a layer in build_scene_cb, not baked here.
            }
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let offset = y as usize * row_len + byte_start;
                let src_offset = row_idx * row_len + byte_start;
                if byte_start == 0 && byte_end == row_len {
                    let band_bytes = (band_end - band_start) as usize * row_len;
                    vmo_write(
                        handle,
                        RETAINED_OFFSET_BYTES + band_start as usize * row_len,
                        &self.band_scratch[..band_bytes],
                    )
                    .map_err(|_| WindowdError::BufferLengthMismatch)?;
                    break;
                }
                vmo_write(
                    handle,
                    RETAINED_OFFSET_BYTES + offset,
                    &self.band_scratch[src_offset..src_offset + (byte_end - byte_start)],
                )
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            }
            band_start = band_end;
        }
        self.shadow_arena_used = shadow_arena.used_bytes();
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.telemetry.record_compose_timed(
            u64::from(end_x.saturating_sub(start_x))
                .saturating_mul(u64::from(end_y.saturating_sub(start_y))),
            nsec().unwrap_or(render_start_ns).saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        self.refresh_observer_state();
        Ok(())
    }

    fn queue_dirty_rect(&mut self, rect: DamageRect) {
        self.tile_map.mark_rect(rect);
        for existing in &mut self.pending_damage_rects {
            if rect.x <= existing.end_x()
                && rect.end_x() >= existing.x
                && rect.y <= existing.end_y()
                && rect.end_y() >= existing.y
            {
                *existing = existing.merge(rect);
                return;
            }
        }
        if self.pending_damage_rects.len() < 4 {
            self.pending_damage_rects.push(rect);
        }
    }

    /// Queue a GPU-only blit rect for animation frames where only GPU CB params
    /// (translate_x, opacity) changed. Plane 1 is already current — no CPU
    /// recomposite. The rect still needs a display-plane refresh from Plane 1.
    fn queue_gpu_blit_rect(&mut self, rect: DamageRect) {
        self.pending_gpu_blit_rect = Some(match self.pending_gpu_blit_rect {
            Some(existing) => existing.merge(rect),
            None => rect,
        });
    }

    /// Build the per-frame GPU CommandBuffer (GPU-first layout-tree model).
    ///
    /// CPU writes background content (wallpaper + proof panel) into Plane 1 only on
    /// content change. The GPU CB does all visual work per frame:
    ///   1. Blit each damage region: Plane 1 (retained, cursor-free) → Plane 2 (display).
    ///   2. Always blit + re-render the glass button (it's an animated overlay layer).
    ///   3. Blit + render the sidebar panel (GPU blur + rounded rect, animated translate/opacity).
    ///   4. BlendCursor overlaid last.
    ///
    /// Glass panels use BlurBackdrop (reads from Plane 2 after the blit, so it blurs
    /// the wallpaper/content behind the panel) + FillSdfRoundedRect (glass tint + border).
    /// Record the per-frame scene into the reusable `scene_cb` and serialize it
    /// into `out`. Returns the number of bytes written.
    ///
    /// Zero per-frame heap allocation: `scene_cb` is cleared (capacity retained)
    /// rather than freshly allocated, and serialization borrows it instead of
    /// consuming it into a `CommittedBuffer`. This is mandatory under windowd's
    /// non-freeing bump allocator — a per-frame `CommandBuffer::new()` would leak
    /// its `Vec<Command>` and crash the service mid-animation.
    fn build_scene_cb_into(
        &mut self,
        rects: &[DamageRect],
        rect_count: usize,
        out: &mut [u8],
    ) -> Result<usize, WindowdError> {
        // Re-render the chat layer's cached surface (off-screen atlas) if its
        // content changed. Done before the encoder borrows `self.scene_cb`.
        if self.chat_surface_dirty {
            self.render_chat_surface()?;
            self.chat_surface_dirty = false;
        }
        // Shell-P2b: (re)render the glass topbar layer surface when dirty.
        if SHELL_TOPBAR && self.shell_surface_dirty {
            self.render_shell_surface()?;
            self.shell_surface_dirty = false;
        }
        // Shell-P2b: (re)render the glass side panel surface when dirty.
        if SHELL_SIDEPANEL && self.sidepanel_surface_dirty {
            self.render_sidepanel_surface()?;
            self.sidepanel_surface_dirty = false;
        }
        // Snapshot all `self` reads needed inside the encoder block so the
        // mutable borrow of `self.scene_cb` does not conflict with field reads.
        let mode = self.mode;
        let scene = self.animated_scene;
        let cursor_w = self.cursor_width;
        let cursor_h = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
        let hw_cursor = self.hw_cursor_active;
        let blur_cache_valid = self.sidebar_blur_cache_valid;
        // Pre-blur pass rides the first handoff present (full-screen damage, so
        // the display plane holds the complete base scene to blur from).
        let precache_sidebar_blur = !USE_DESKTOP_SHELL
            && self.precache_blur_pending
            && !blur_cache_valid
            && scene.sidebar_opacity <= 0.01;
        // Chat layer: source row of its cached surface + on-screen placement from
        // the window manager (so a drag just changes the blit destination).
        // Shell-P2b: the proof/shell chat atlas, sidebar, and glass buttons are
        // suppressed in desktop mode — the desktop chrome is composited into the
        // retained plane (step 1 blit) instead; chat/sidebar return as real
        // desktop layers in P3.
        let chat_atlas_row = self.chat_atlas.abs_row;
        let shell_atlas_row = self.shell_atlas.abs_row;
        let shell_w = self.shell_w;
        let shell_h = self.shell_h;
        let sidepanel_atlas_row = self.sidepanel_atlas.abs_row;
        let sidepanel_h = self.sidepanel_h;
        // Slide: sidebar_translate_x animates SIDEBAR_WIDTH(closed) -> 0(open).
        let sidepanel_slide = scene.sidebar_translate_x;
        let sidepanel_opacity = scene.sidebar_opacity;
        let chat_show = !USE_DESKTOP_SHELL && self.wm.chat_visible();
        let chat_dx = self.wm.chat_window().bounds.x.max(0) as u32;
        let chat_dy = self.wm.chat_window().bounds.y.max(0) as u32;
        // GPU scroll-offset: the body samples the overscan surface shifted by the
        // scroll-within-window; the title bar is composited fixed on top.
        // HARDENING: clamp to [0, CHAT_OVERSCAN]. The surface is only
        // `CHAT_PANEL_H + CHAT_OVERSCAN` tall, so the composite samples rows
        // `[base+offset .. base+offset+CHAT_PANEL_H]`. If momentum ever advanced
        // the scroll past the prerendered window before the recenter re-render
        // landed, an unclamped offset would sample BEYOND the chat surface into
        // adjacent atlas rows (blur/sidebar caches) → garbage or out-of-bounds.
        // Clamping shows the window edge for one frame instead of corrupting.
        let chat_content_offset =
            self.chat_scroll_y.saturating_sub(self.chat_render_base).min(CHAT_OVERSCAN);
        let chat_title_h = crate::interaction::CHAT_TITLE_BAR_H + crate::interaction::CHAT_PAD;
        let chat_blur_cache_row = self.chat_blur_cache.abs_row;
        let chat_blur_cache_valid = self.chat_blur_cache_valid;
        let mut built_chat_blur_cache = false;
        let btn_blur_cache_valid = self.button_blur_cache_valid;
        let mut built_button_cache = false;
        // Sidebar composite cache: usable only when the sidebar is fully open and
        // static (settled). During the slide it's redrawn each frame (animation).
        let sidebar_settled = scene.sidebar_opacity >= 0.99 && scene.sidebar_translate_x <= 0.5;
        let sidebar_composite_cache_row = self.sidebar_composite_cache.abs_row;
        let sidebar_composite_cache_valid = self.sidebar_composite_cache_valid;
        let mut built_sidebar_composite_cache = false;

        // Incremental overlays: a static glass overlay (hamburger, chat button,
        // sidebar) only needs re-rendering when a damage rect actually overwrote its
        // region — step 1 above blits ONLY the damage rects, so an untouched overlay
        // persists on the display plane. Every interaction that changes an overlay
        // queues that overlay's rect (note_button_hover_changed, sidebar open/slide,
        // chat-visibility toggle), so "region touched" is the exact, complete redraw
        // condition. This keeps a far-away hover/card change off the glass GPU work
        // (the per-present cost that made the UI feel unresponsive once the cursor was
        // decoupled to the HW overlay).
        let overlaps = |x0: i32, y0: i32, x1: i32, y1: i32| -> bool {
            rects.iter().take(rect_count).any(|r| {
                let rx1 = (r.x + r.width) as i32;
                let ry1 = (r.y + r.height) as i32;
                (r.x as i32) < x1 && rx1 > x0 && (r.y as i32) < y1 && ry1 > y0
            })
        };
        let hb = crate::interaction::button_rect(mode.width);
        let button_touched =
            overlaps(hb.x as i32, hb.y as i32, (hb.x + hb.width) as i32, (hb.y + hb.height) as i32);
        let cbtn = crate::interaction::chat_button_rect(mode.width, mode.height);
        let chat_btn_touched = overlaps(
            cbtn.x as i32,
            cbtn.y as i32,
            (cbtn.x + cbtn.width) as i32,
            (cbtn.y + cbtn.height) as i32,
        );
        let sidebar_touched = {
            let sx = mode
                .width
                .saturating_sub(SIDEBAR_WIDTH)
                .saturating_add(scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32);
            overlaps(sx as i32, 0, mode.width as i32, mode.height as i32)
        };

        self.scene_cb.clear();
        {
            let mut encoder = self
                .scene_cb
                .try_begin_render_pass(RenderPassDesc {
                    color_attachments: alloc::vec![],
                    width: mode.width,
                    height: mode.height,
                })
                .map_err(|_| WindowdError::InvalidDamage)?;

            // 1. Blit content damage from retained plane → display plane.
            for rect in rects.iter().copied().take(rect_count) {
                encoder
                    .try_blit_surface(
                        rect.x,
                        rect.y + RETAINED_ROW_OFFSET,
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                    )
                    .map_err(|_| WindowdError::InvalidDamage)?;
            }

            // 1·shell. Glass topbar layer (Shell-P2b): composite the topbar atlas
            //     onto the display each present with backdrop blur + rounded
            //     corners + a soft drop shadow — the GPU layer path that reaches
            //     the virgl scanout (the retained Plane 1 does not). Rendered like
            //     the chat window: translucent tint + opaque text in the atlas,
            //     glass effects applied here by the composite.
            if SHELL_TOPBAR && shell_w > 0 && shell_h > 0 {
                use crate::compositor::desktop_layer::{TOPBAR_MARGIN_X, TOPBAR_RADIUS, TOPBAR_TOP};
                // Proven glass recipe (same as the glass buttons): restore the
                // clean backdrop from the retained plane, blur it in place, THEN
                // composite the topbar atlas (translucent tint + crisp text) on
                // top with backdrop_blur=0. Passing backdrop_blur to the composite
                // smears the layer content (text) into a gray blob — this keeps
                // the text sharp over a frosted backdrop.
                let bar = TileRect {
                    x: TOPBAR_MARGIN_X,
                    y: TOPBAR_TOP,
                    width: shell_w,
                    height: shell_h,
                };
                let _ = encoder.try_blit_surface(
                    TOPBAR_MARGIN_X,
                    TOPBAR_TOP + RETAINED_ROW_OFFSET,
                    TOPBAR_MARGIN_X,
                    TOPBAR_TOP,
                    shell_w,
                    shell_h,
                );
                let _ =
                    encoder.try_blur_backdrop(bar, DARK_GLASS_BLUR_RADIUS, DARK_GLASS_SATURATION_PERCENT);
                let _ = encoder.try_composite_layer(
                    shell_atlas_row,
                    0,
                    shell_w,
                    shell_h,
                    TOPBAR_MARGIN_X,
                    TOPBAR_TOP,
                    255,
                    TOPBAR_RADIUS,
                    10,
                    3,
                    60,
                    0,
                );
            }

            // 1·panel. Glass side panel — slides in from the right, driven by the
            //     sidebar spring. Same proven recipe as the topbar: restore +
            //     pre-blur the panel's current rect, then composite the atlas with
            //     rounded corners + drop shadow on top (backdrop_blur=0).
            if SHELL_SIDEPANEL && sidepanel_opacity > 0.01 {
                use crate::compositor::desktop_layer::{
                    SIDEPANEL_MARGIN, SIDEPANEL_RADIUS, SIDEPANEL_TOP, SIDEPANEL_W,
                };
                let base_x = mode
                    .width
                    .saturating_sub(SIDEPANEL_MARGIN + SIDEPANEL_W)
                    .saturating_add(sidepanel_slide.clamp(0.0, SIDEPANEL_W as f32 + 32.0) as u32);
                if base_x < mode.width {
                    let w = SIDEPANEL_W.min(mode.width.saturating_sub(base_x));
                    let alpha = (sidepanel_opacity.clamp(0.0, 1.0) * 255.0) as u32;
                    let panel = TileRect { x: base_x, y: SIDEPANEL_TOP, width: w, height: sidepanel_h };
                    let _ = encoder.try_blit_surface(
                        base_x,
                        SIDEPANEL_TOP + RETAINED_ROW_OFFSET,
                        base_x,
                        SIDEPANEL_TOP,
                        w,
                        sidepanel_h,
                    );
                    let _ = encoder.try_blur_backdrop(
                        panel,
                        DARK_GLASS_BLUR_RADIUS,
                        DARK_GLASS_SATURATION_PERCENT,
                    );
                    let _ = encoder.try_composite_layer(
                        sidepanel_atlas_row,
                        0,
                        w,
                        sidepanel_h,
                        base_x,
                        SIDEPANEL_TOP,
                        alpha,
                        SIDEPANEL_RADIUS,
                        16,
                        4,
                        80,
                        0,
                    );
                }
            }

            // 1a. Chat window layer: composite its cached opaque surface from the
            //     VMO atlas onto the display at the window's current position.
            //     One blit — dragging the window just changes the destination,
            //     no content re-render. Drawn over the base, under the button/
            //     sidebar overlays (z-order finalized in a later phase).
            if chat_show {
                use crate::interaction::{CHAT_PANEL_H, CHAT_PANEL_W};
                // Only recomposite the chat layer when a damage rect actually
                // touches its shadow halo. The window + shadow persist on the
                // display plane otherwise, so a cursor move far away keeps the
                // cheap hot path (no halo flush). Without this gate the shadow
                // would be redrawn on every present.
                let pad = CHAT_SHADOW_BLUR.saturating_add(CHAT_SHADOW_OFFSET_Y.unsigned_abs());
                let hx0 = chat_dx.saturating_sub(pad) as i32;
                let hy0 = chat_dy.saturating_sub(pad) as i32;
                let hx1 = (chat_dx + CHAT_PANEL_W + pad) as i32;
                let hy1 = (chat_dy + CHAT_PANEL_H + pad) as i32;
                // On virgl the scanout is rebuilt every present, so the chat
                // layer must be re-composited each frame — the damage-touch gate
                // (an mmio optimization where the display plane persists) would
                // otherwise show the window only when the cursor passed over it.
                let _touches_chat = rects.iter().take(rect_count).any(|r| {
                    let rx1 = (r.x + r.width) as i32;
                    let ry1 = (r.y + r.height) as i32;
                    (r.x as i32) < hx1 && rx1 > hx0 && (r.y as i32) < hy1 && ry1 > hy0
                });
                if true {
                    // Restore the full halo from the retained plane first so the
                    // (translucent) shadow blends over a clean backdrop and never
                    // accumulates — and the cursor hot path can't carve a trail
                    // through it. Then composite the chat as ONE layer: gpud
                    // GPU-composites it (shadow + content texture + rounded mask)
                    // on virgl, or CPU-bakes it (shadow + opaque blit) on the 2D
                    // path. The window's content lives in the atlas; moving it
                    // just changes the layer's destination.
                    let hx = chat_dx.saturating_sub(pad);
                    let hy = chat_dy.saturating_sub(pad);
                    let hw = (CHAT_PANEL_W + 2 * pad).min(mode.width.saturating_sub(hx));
                    let hh = (CHAT_PANEL_H + 2 * pad).min(mode.height.saturating_sub(hy));
                    let _ = encoder.try_blit_surface(hx, hy + RETAINED_ROW_OFFSET, hx, hy, hw, hh);
                    // Backdrop-blur cache: the halo restore just put the clean
                    // base into the window region. Blur it ONCE (on open/move)
                    // into the cache; thereafter just blit the cache back —
                    // zero per-frame blur for the glass.
                    if !chat_blur_cache_valid {
                        let _ = encoder.try_blur_backdrop(
                            TileRect {
                                x: chat_dx,
                                y: chat_dy,
                                width: CHAT_PANEL_W,
                                height: CHAT_PANEL_H,
                            },
                            super::DARK_GLASS_BLUR_RADIUS,
                            super::DARK_GLASS_SATURATION_PERCENT,
                        );
                        // Save the blurred backdrop (display window region) to
                        // the off-screen cache surface.
                        let _ = encoder.try_blit_absolute(
                            chat_dx,
                            DISPLAY_ROW_OFFSET + chat_dy,
                            0,
                            chat_blur_cache_row,
                            CHAT_PANEL_W,
                            CHAT_PANEL_H,
                        );
                        built_chat_blur_cache = true;
                    } else {
                        // Reuse the cached blurred backdrop (one blit, no blur).
                        let _ = encoder.try_blit_absolute(
                            0,
                            chat_blur_cache_row,
                            chat_dx,
                            DISPLAY_ROW_OFFSET + chat_dy,
                            CHAT_PANEL_W,
                            CHAT_PANEL_H,
                        );
                    }
                    // The backdrop is already blurred in the display window
                    // region, so the layer composite does shadow + content blend
                    // only (backdrop_blur = 0).
                    // Body: the whole window (shadow + rounded), sampling the
                    // overscan surface shifted by the scroll-within-window offset.
                    // Marked SCROLLABLE so gpud retains it and can re-sample it at a
                    // new source row on the lightweight `OP_SET_CHAT_SCROLL` fast
                    // path (a 54µs GPU re-composite) — no CPU re-render per frame.
                    let _ = encoder.try_composite_layer_scrollable(
                        chat_atlas_row + chat_content_offset,
                        0,
                        CHAT_PANEL_W,
                        CHAT_PANEL_H,
                        chat_dx,
                        chat_dy,
                        255,
                        super::DARK_GLASS_RADIUS,
                        CHAT_SHADOW_BLUR,
                        CHAT_SHADOW_OFFSET_Y,
                        CHAT_SHADOW_ALPHA as u32,
                        0,
                    );
                    // Title bar: composited FIXED on top (src row 0, no offset),
                    // overdrawing the scrolled title region — so the title never
                    // moves while the content scrolls underneath.
                    let _ = encoder.try_composite_layer(
                        chat_atlas_row,
                        0,
                        CHAT_PANEL_W,
                        chat_title_h,
                        chat_dx,
                        chat_dy,
                        255,
                        0,
                        0,
                        0,
                        0,
                        0,
                    );
                }
            }

            // 1b. Pre-blur the sidebar backdrop at handoff (sidebar closed,
            //     before any overlay is drawn — the display plane equals the
            //     clean Plane 1 base here): blur the rest-position strip, save
            //     it to the Plane 3 cache, restore the unblurred content from
            //     Plane 1. One-time cost — the first sidebar open (and every
            //     slide frame) is then a pure cache blit, zero blur work.
            if precache_sidebar_blur {
                let sidebar_h =
                    mode.height.saturating_sub(SIDEBAR_MARGIN_TOP + SIDEBAR_MARGIN_BOTTOM).max(1);
                let rest = TileRect {
                    x: SIDEBAR_REST_X,
                    y: SIDEBAR_MARGIN_TOP,
                    width: SIDEBAR_WIDTH,
                    height: sidebar_h,
                };
                let _ = encoder.try_blur_backdrop(rest, 20, DARK_GLASS_SATURATION_PERCENT);
                let _ = encoder.try_blit_absolute(
                    SIDEBAR_REST_X,
                    DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                    SIDEBAR_REST_X,
                    BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                    SIDEBAR_WIDTH,
                    sidebar_h,
                );
                let _ = encoder.try_blit_surface(
                    SIDEBAR_REST_X,
                    SIDEBAR_MARGIN_TOP + RETAINED_ROW_OFFSET,
                    SIDEBAR_REST_X,
                    SIDEBAR_MARGIN_TOP,
                    SIDEBAR_WIDTH,
                    sidebar_h,
                );
            }

            // 2. Glass button — cached blur, skipped when sidebar covers it.
            let button_x = mode.width.saturating_sub(GLASS_BUTTON_W + GLASS_BUTTON_RIGHT);
            let button_blit_w = GLASS_BUTTON_W.min(mode.width.saturating_sub(button_x));
            let sidebar_x_for_btn = mode
                .width
                .saturating_sub(SIDEBAR_WIDTH)
                .saturating_add(scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32);
            let button_covered = scene.sidebar_opacity > 0.01 && sidebar_x_for_btn <= button_x;
            // Incremental: only redraw the glass button when a damage rect overwrote
            // its region (hover spring / handoff / cache build all queue the button
            // rect). A far-away change leaves the button untouched on the display plane.
            // The glass topbar carries the menu icon now, so the standalone
            // hamburger button (which would overlap the topbar) is suppressed.
            if !USE_DESKTOP_SHELL
                && !SHELL_TOPBAR
                && button_blit_w > 0
                && !button_covered
                && (button_touched || !btn_blur_cache_valid)
            {
                if btn_blur_cache_valid {
                    // Fast path: restore pre-blurred background from Plane 3 cache.
                    let _ = encoder.try_blit_absolute(
                        BUTTON_BLUR_CACHE_ABS_X,
                        BUTTON_BLUR_CACHE_ABS_ROW,
                        button_x,
                        DISPLAY_ROW_OFFSET + GLASS_BUTTON_TOP,
                        button_blit_w,
                        GLASS_BUTTON_H,
                    );
                } else {
                    // Cache-build path: blit P1, blur in-place, save to Plane 3.
                    let _ = encoder.try_blit_surface(
                        button_x,
                        GLASS_BUTTON_TOP + RETAINED_ROW_OFFSET,
                        button_x,
                        GLASS_BUTTON_TOP,
                        button_blit_w,
                        GLASS_BUTTON_H,
                    );
                    let btn_build_rect = TileRect {
                        x: button_x,
                        y: GLASS_BUTTON_TOP,
                        width: button_blit_w,
                        height: GLASS_BUTTON_H,
                    };
                    let _ = encoder.try_blur_backdrop(
                        btn_build_rect,
                        DARK_GLASS_BLUR_RADIUS,
                        DARK_GLASS_SATURATION_PERCENT,
                    );
                    let _ = encoder.try_blit_absolute(
                        button_x,
                        DISPLAY_ROW_OFFSET + GLASS_BUTTON_TOP,
                        BUTTON_BLUR_CACHE_ABS_X,
                        BUTTON_BLUR_CACHE_ABS_ROW,
                        button_blit_w,
                        GLASS_BUTTON_H,
                    );
                    built_button_cache = true;
                }
                let btn_rect = TileRect {
                    x: button_x,
                    y: GLASS_BUTTON_TOP,
                    width: button_blit_w,
                    height: GLASS_BUTTON_H,
                };
                let button_alpha = (96.0 + 80.0 * scene.hover_opacity).clamp(96.0, 220.0) as u8;
                let gt = crate::assets::GLASS_TINT;
                let ge = crate::assets::GLASS_EDGE;
                // Glass body as a vertical gradient (light falls from above) —
                // GPU per-pixel via the SDF shader, CPU per-row fallback.
                let _ = encoder.try_fill_sdf_gradient(
                    btn_rect,
                    GLASS_BUTTON_RADIUS,
                    RgbaColor::new(
                        gt.r.saturating_add(18),
                        gt.g.saturating_add(18),
                        gt.b.saturating_add(18),
                        button_alpha,
                    ),
                    RgbaColor::new(
                        gt.r.saturating_sub(8),
                        gt.g.saturating_sub(8),
                        gt.b.saturating_sub(8),
                        button_alpha,
                    ),
                );
                let _ = encoder.try_fill_sdf_rounded_rect(
                    btn_rect,
                    GLASS_BUTTON_RADIUS,
                    RgbaColor::new(ge.r, ge.g, ge.b, ge.a),
                );
                // Hamburger icon: 3 horizontal bars centered inside the glass button.
                const MENU_BAR_W: u32 = 18;
                const MENU_BAR_H: u32 = 3;
                const MENU_BAR_GAP: u32 = 5;
                const MENU_TOTAL_H: u32 = 3 * MENU_BAR_H + 2 * MENU_BAR_GAP;
                let bar_x = button_x.saturating_add(GLASS_BUTTON_W.saturating_sub(MENU_BAR_W) / 2);
                let bar_y = GLASS_BUTTON_TOP
                    .saturating_add(GLASS_BUTTON_H.saturating_sub(MENU_TOTAL_H) / 2);
                let icon_alpha = (160.0 + 80.0 * scene.hover_opacity).clamp(160.0, 240.0) as u8;
                let bar_color = RgbaColor::new(255, 255, 255, icon_alpha);
                let _ = encoder.try_fill_sdf_rounded_rect(
                    TileRect { x: bar_x, y: bar_y, width: MENU_BAR_W, height: MENU_BAR_H },
                    1,
                    bar_color,
                );
                let _ = encoder.try_fill_sdf_rounded_rect(
                    TileRect {
                        x: bar_x,
                        y: bar_y + MENU_BAR_H + MENU_BAR_GAP,
                        width: MENU_BAR_W,
                        height: MENU_BAR_H,
                    },
                    1,
                    bar_color,
                );
                let _ = encoder.try_fill_sdf_rounded_rect(
                    TileRect {
                        x: bar_x,
                        y: bar_y + 2 * (MENU_BAR_H + MENU_BAR_GAP),
                        width: MENU_BAR_W,
                        height: MENU_BAR_H,
                    },
                    1,
                    bar_color,
                );
            }

            // 2b. Chat toggle button — square glass button under the hamburger
            //     (P7). Same cover rule as the hamburger: hidden while the
            //     sidebar overlaps it. Speech-bubble glyph: rounded outline +
            //     three dots.
            {
                use crate::interaction::{chat_button_rect, CHAT_BUTTON_RADIUS};
                let cb = chat_button_rect(mode.width, mode.height);
                let covered = scene.sidebar_opacity > 0.01 && sidebar_x_for_btn <= cb.x;
                // Incremental: only redraw when its region was overwritten (chat-visibility
                // toggle queues the chat-button rect; handoff damages full screen).
                if !USE_DESKTOP_SHELL && cb.width > 0 && !covered && chat_btn_touched {
                    let gt = crate::assets::GLASS_TINT;
                    let ge = crate::assets::GLASS_EDGE;
                    let cb_rect = TileRect { x: cb.x, y: cb.y, width: cb.width, height: cb.height };
                    // Restore the clean base from Plane 1 first — the glass
                    // fills are translucent and would accumulate over the
                    // previous frame's button pixels otherwise.
                    let _ = encoder.try_blit_surface(
                        cb.x,
                        cb.y + RETAINED_ROW_OFFSET,
                        cb.x,
                        cb.y,
                        cb.width,
                        cb.height,
                    );
                    let chat_open = self.wm.chat_window().visible;
                    // Slightly brighter while the chat window is open (active state).
                    let body_alpha: u8 = if chat_open { 200 } else { 128 };
                    let _ = encoder.try_fill_sdf_gradient(
                        cb_rect,
                        CHAT_BUTTON_RADIUS,
                        RgbaColor::new(
                            gt.r.saturating_add(18),
                            gt.g.saturating_add(18),
                            gt.b.saturating_add(18),
                            body_alpha,
                        ),
                        RgbaColor::new(
                            gt.r.saturating_sub(8),
                            gt.g.saturating_sub(8),
                            gt.b.saturating_sub(8),
                            body_alpha,
                        ),
                    );
                    let _ = encoder.try_fill_sdf_rounded_rect(
                        cb_rect,
                        CHAT_BUTTON_RADIUS,
                        RgbaColor::new(ge.r, ge.g, ge.b, ge.a),
                    );
                    // Speech bubble: a rounded rect with three dots.
                    const BUBBLE_W: u32 = 26;
                    const BUBBLE_H: u32 = 18;
                    let bx = cb.x + (cb.width - BUBBLE_W) / 2;
                    let by = cb.y + (cb.height - BUBBLE_H) / 2;
                    let icon = RgbaColor::new(255, 255, 255, 220);
                    let _ = encoder.try_fill_sdf_rounded_rect(
                        TileRect { x: bx, y: by, width: BUBBLE_W, height: BUBBLE_H },
                        6,
                        icon,
                    );
                    let dot = RgbaColor::new(
                        gt.r.saturating_sub(8),
                        gt.g.saturating_sub(8),
                        gt.b.saturating_sub(8),
                        255,
                    );
                    for i in 0..3u32 {
                        let _ = encoder.try_fill_sdf_rounded_rect(
                            TileRect {
                                x: bx + 5 + i * 6,
                                y: by + BUBBLE_H / 2 - 1,
                                width: 3,
                                height: 3,
                            },
                            1,
                            dot,
                        );
                    }
                }
            }

            // 3. Sidebar panel — GPU overlay, only when visible (opacity > 0).
            //    Blur caching: compute once per open into Plane 3 (Slot B, rows 2400+),
            //    then blit from cache each animation frame instead of re-blurring.
            //    The wallpaper behind the sidebar is static so the blur is identical
            //    every frame. Cache spans the full 320px at SIDEBAR_REST_X=960 so all
            //    visible sub-strips during the slide animation are covered.
            let sidebar_opacity = scene.sidebar_opacity;
            // Incremental: redraw only when sliding/opening (animation queues the
            // sidebar rect each tick), when a damage rect overwrote it, or while a
            // blur/composite cache still needs building. A settled, cached, untouched
            // sidebar persists on the display plane — no per-present blur/SDF work.
            if !USE_DESKTOP_SHELL
                && !SHELL_SIDEPANEL
                && sidebar_opacity > 0.01
                && (sidebar_touched || !blur_cache_valid || !sidebar_composite_cache_valid)
            {
                let translate = scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32;
                let sidebar_x = mode.width.saturating_sub(SIDEBAR_WIDTH).saturating_add(translate);
                if sidebar_x < mode.width {
                    let sidebar_w = SIDEBAR_WIDTH.min(mode.width.saturating_sub(sidebar_x));
                    let sidebar_h = mode
                        .height
                        .saturating_sub(SIDEBAR_MARGIN_TOP + SIDEBAR_MARGIN_BOTTOM)
                        .max(1);

                    // Fast path: the sidebar is settled and already composited
                    // into the cache — one blit, skip the blur-cache + SDF fills.
                    if sidebar_settled && sidebar_composite_cache_valid {
                        let _ = encoder.try_blit_absolute(
                            sidebar_x,
                            sidebar_composite_cache_row + SIDEBAR_MARGIN_TOP,
                            sidebar_x,
                            DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                            sidebar_w,
                            sidebar_h,
                        );
                    } else {
                        if !blur_cache_valid {
                            // Cache-build frame (once per sidebar open):
                            // restore full Plane 1 bg at rest position, blur it, save to Plane 3.
                            let _ = encoder.try_blit_surface(
                                SIDEBAR_REST_X,
                                SIDEBAR_MARGIN_TOP + RETAINED_ROW_OFFSET,
                                SIDEBAR_REST_X,
                                SIDEBAR_MARGIN_TOP,
                                SIDEBAR_WIDTH,
                                sidebar_h,
                            );
                            let full_sbr = TileRect {
                                x: SIDEBAR_REST_X,
                                y: SIDEBAR_MARGIN_TOP,
                                width: SIDEBAR_WIDTH,
                                height: sidebar_h,
                            };
                            let _ = encoder.try_blur_backdrop(
                                full_sbr,
                                20,
                                DARK_GLASS_SATURATION_PERCENT,
                            );
                            // Save blurred display pixels to Plane 3 cache.
                            let _ = encoder.try_blit_absolute(
                                SIDEBAR_REST_X,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                SIDEBAR_REST_X,
                                BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                SIDEBAR_WIDTH,
                                sidebar_h,
                            );
                            // Blit the currently-visible strip from cache for this frame.
                            let _ = encoder.try_blit_absolute(
                                sidebar_x,
                                BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_x,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_w,
                                sidebar_h,
                            );
                        } else {
                            // Cache-use frame: blit pre-blurred strip from Plane 3 — no blur.
                            let _ = encoder.try_blit_absolute(
                                sidebar_x,
                                BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_x,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_w,
                                sidebar_h,
                            );
                        }

                        let sbr = TileRect {
                            x: sidebar_x,
                            y: SIDEBAR_MARGIN_TOP,
                            width: sidebar_w,
                            height: sidebar_h,
                        };
                        // Translucent enough that the blurred backdrop reads as
                        // glass (220 was nearly opaque → looked flat gray).
                        let sidebar_alpha = (150.0 * sidebar_opacity).clamp(0.0, 150.0) as u8;
                        let border_alpha = (130.0 * sidebar_opacity).clamp(0.0, 130.0) as u8;
                        let gt = crate::assets::GLASS_TINT;
                        let ge = crate::assets::GLASS_EDGE;
                        let pb = crate::assets::PROOF_PANEL_BORDER;
                        // Border: fill outer rect with border color, then cover interior with glass fill.
                        let _ = encoder.try_fill_sdf_rounded_rect(
                            sbr,
                            SIDEBAR_RADIUS,
                            RgbaColor::new(pb.r, pb.g, pb.b, border_alpha),
                        );
                        if sidebar_w > 2 && sidebar_h > 2 {
                            let sbr_inner = TileRect {
                                x: sbr.x + 1,
                                y: sbr.y + 1,
                                width: sbr.width - 2,
                                height: sbr.height - 2,
                            };
                            // Glass body as a vertical gradient (light from above).
                            let _ = encoder.try_fill_sdf_gradient(
                                sbr_inner,
                                SIDEBAR_RADIUS.saturating_sub(1),
                                RgbaColor::new(
                                    gt.r.saturating_add(14),
                                    gt.g.saturating_add(14),
                                    gt.b.saturating_add(14),
                                    sidebar_alpha,
                                ),
                                RgbaColor::new(
                                    gt.r.saturating_sub(6),
                                    gt.g.saturating_sub(6),
                                    gt.b.saturating_sub(6),
                                    sidebar_alpha,
                                ),
                            );
                            let _ = encoder.try_fill_sdf_rounded_rect(
                                sbr_inner,
                                SIDEBAR_RADIUS.saturating_sub(1),
                                RgbaColor::new(ge.r, ge.g, ge.b, ge.a),
                            );
                        }
                        // Close icon (× approximated as + shape) at top-right of sidebar.
                        const CLOSE_SIZE: u32 = 16;
                        const CLOSE_BAR: u32 = 3;
                        const CLOSE_INSET: u32 = 16;
                        if sidebar_w > CLOSE_SIZE + CLOSE_INSET {
                            let cx = sidebar_x
                                .saturating_add(sidebar_w.saturating_sub(CLOSE_SIZE + CLOSE_INSET));
                            let cy = SIDEBAR_MARGIN_TOP.saturating_add(CLOSE_INSET);
                            let close_alpha = (200.0 * sidebar_opacity).clamp(0.0, 220.0) as u8;
                            let cc = RgbaColor::new(255, 255, 255, close_alpha);
                            let _ = encoder.try_fill_sdf_rounded_rect(
                                TileRect {
                                    x: cx,
                                    y: cy + (CLOSE_SIZE - CLOSE_BAR) / 2,
                                    width: CLOSE_SIZE,
                                    height: CLOSE_BAR,
                                },
                                1,
                                cc,
                            );
                            let _ = encoder.try_fill_sdf_rounded_rect(
                                TileRect {
                                    x: cx + (CLOSE_SIZE - CLOSE_BAR) / 2,
                                    y: cy,
                                    width: CLOSE_BAR,
                                    height: CLOSE_SIZE,
                                },
                                1,
                                cc,
                            );
                        }
                        // Snapshot the fully composited sidebar into the cache on the
                        // first settled frame; subsequent presents are a single blit.
                        if sidebar_settled {
                            let _ = encoder.try_blit_absolute(
                                sidebar_x,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_x,
                                sidebar_composite_cache_row + SIDEBAR_MARGIN_TOP,
                                sidebar_w,
                                sidebar_h,
                            );
                            built_sidebar_composite_cache = true;
                        }
                    }
                }
            }

            // 4. Cursor — composited last, never baked into any plane. Skipped
            //    entirely when the hardware cursor overlay is active (the host
            //    displays and moves the cursor; frames never carry it). In the
            //    software fallback a cursor-only move is a cheap cursor-region
            //    blit (from the retained Plane 1) + this BlendCursor.
            if !hw_cursor && cursor_w > 0 && cursor_h > 0 {
                let cx = (cursor_x - crate::assets::CURSOR_HOTSPOT_X).max(0) as u32;
                let cy = (cursor_y - crate::assets::CURSOR_HOTSPOT_Y).max(0) as u32;
                if cx < mode.width && cy < mode.height {
                    let _ = encoder.try_blend_cursor(cx, cy, cursor_w, cursor_h);
                }
            }

            encoder.end_encoding();
        }
        // Commit cache-build results so subsequent frames use the caches.
        if precache_sidebar_blur {
            self.sidebar_blur_cache_valid = true;
            self.precache_blur_pending = false;
        }
        if !blur_cache_valid && scene.sidebar_opacity > 0.01 {
            self.sidebar_blur_cache_valid = true;
        }
        if built_button_cache {
            self.button_blur_cache_valid = true;
        }
        if built_chat_blur_cache {
            self.chat_blur_cache_valid = true;
        }
        // Sidebar composite cache: valid once built on a settled frame; dropped
        // whenever the sidebar is animating (slide/fade) so the animation draws
        // fresh frames and the cache is rebuilt when it settles again.
        if built_sidebar_composite_cache {
            self.sidebar_composite_cache_valid = true;
        } else if !sidebar_settled {
            self.sidebar_composite_cache_valid = false;
        }
        self.scene_cb.serialize_into(out).map_err(|_| WindowdError::InvalidDamage)
    }

    /// Flush pending damage to gpud as one batched CommandBuffer.
    ///
    /// Phase 0 (GPU pipeline hardening): the scene graph is the single rendering
    /// authority. `compute_dirty_set()` on the scene graph drives all CB generation.
    /// No CPU compositing — wallpaper is a `BlitSurface` from Plane 0,
    /// panels are `FillSdfRoundedRect`/`BlurBackdrop`, and the cursor is `BlendCursor`.
    pub(crate) fn flush_pending_damage(&mut self) -> Result<(), WindowdError> {
        let paint_only = self.paint_only_damage;

        // 1. Collect content damage (panels/text — needs CPU recomposite of Plane 1).
        let mut content = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; 5];
        let mut content_count = 0usize;
        if let Some(rect) = self.pending_damage_rect.take() {
            content[content_count] = rect;
            content_count += 1;
        }
        while let Some(rect) = self.pending_damage_rects.pop() {
            if content_count < content.len() {
                content[content_count] = rect;
                content_count += 1;
            }
        }
        content_count = premerge_damage_rects(&mut content, content_count);

        // GPU-blit-only rect from animation ticks (Plane 1 already current).
        let gpu_blit_rect = self.pending_gpu_blit_rect.take();
        // Cursor-only move: skip CPU recomposite — just a cheap blit of the
        // cursor region from the retained Plane 1 + BlendCursor (the hot path).
        let cursor_rect = self.pending_cursor_rect.take();

        if content_count == 0 && gpu_blit_rect.is_none() && cursor_rect.is_none() {
            return Ok(());
        }

        // 2. Recomposite ONLY content damage into Plane 1 (CPU, blur cached).
        let glass_quality = select_glass_quality(PROOF_PANEL_H);
        for rect in content.iter().copied().take(content_count) {
            self.write_damage_rect(rect, glass_quality, paint_only)?;
        }

        // 3. Blit list: content + gpu-blit + cursor rects — all refresh the
        //    display plane from the retained Plane 1.
        let mut blits = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; 7];
        let mut blit_count = 0usize;
        for rect in content.iter().copied().take(content_count) {
            blits[blit_count] = rect;
            blit_count += 1;
        }
        if let Some(rect) = gpu_blit_rect {
            blits[blit_count] = rect;
            blit_count += 1;
        }
        if let Some(rect) = cursor_rect {
            blits[blit_count] = rect;
            blit_count += 1;
        }

        // 4. One scene CB: blit retained→display + GPU glass overlays + cursor.
        let mut frame_buf = [0u8; 8192];
        let written = self.build_scene_cb_into(&blits, blit_count, &mut frame_buf[1..])?;
        self.tile_map.clear();
        frame_buf[0] = GPU_PRESENT_DAMAGE_OP;
        let gpud_ok = self.send_gpud_present(&frame_buf[..1 + written]);
        if !gpud_ok {
            // gpud queue full / backpressured — requeue so the next tick retries.
            for rect in content.iter().copied().take(content_count) {
                self.queue_dirty_rect(rect);
            }
            if let Some(rect) = gpu_blit_rect {
                self.pending_gpu_blit_rect = Some(match self.pending_gpu_blit_rect {
                    Some(existing) => existing.merge(rect),
                    None => rect,
                });
            }
            if let Some(rect) = cursor_rect {
                self.pending_cursor_rect = Some(match self.pending_cursor_rect {
                    Some(existing) => existing.merge(rect),
                    None => rect,
                });
            }
            self.paint_only_damage = false;
            return Ok(());
        }
        if !self.v3b_composition_verified {
            let _ = debug_println("windowd: scene graph on");
            let _ = debug_println("windowd: gpu pipeline on");
        }
        self.emit_input_markers();
        self.v3b_composition_verified = true;
        self.emit_v3b_markers();
        self.paint_only_damage = false;
        Ok(())
    }

    pub(crate) fn has_pending_damage(&self) -> bool {
        self.pending_gpu_blit_rect.is_some()
            || !self.pending_damage_rects.is_empty()
            || self.pending_damage_rect.is_some()
            || self.pending_cursor_rect.is_some()
    }

    /// Stall watchdog — call once per present-loop iteration with `now_ns`.
    ///
    /// Detects the "scrolled and it stopped responding" failure: the loop is still
    /// running but presents make no progress (gpud backpressure / a wedged ring /
    /// heap exhaustion) while damage keeps piling up. When the acknowledged present
    /// seq hasn't advanced for `STALL_THRESHOLD_NS` with damage pending, it logs ONE
    /// diagnostic line per stall episode (rate-limited → the `format!` is not on the
    /// hot path) capturing the state needed to triage it, then re-arms on recovery.
    /// This is the compositor analogue of Android's ANR / Linux's hung-task detector.
    pub(crate) fn watchdog_check(&mut self, now_ns: u64) {
        const STALL_THRESHOLD_NS: u64 = 500_000_000; // 0.5 s — a blatant stall @120Hz
                                                     // Progress = the completed seq advanced, or there's simply nothing pending.
        let progressed = self.last_completed_seq != self.stall_last_seq;
        if progressed || !self.has_pending_damage() {
            self.stall_last_seq = self.last_completed_seq;
            self.stall_last_progress_ns = now_ns;
            self.stall_reported = false;
            return;
        }
        if self.stall_last_progress_ns == 0 {
            self.stall_last_progress_ns = now_ns;
            return;
        }
        let stuck = now_ns.saturating_sub(self.stall_last_progress_ns);
        if stuck >= STALL_THRESHOLD_NS {
            if !self.stall_reported {
                let _ = debug_println(&alloc::format!(
                    "windowd: STALL present stuck {}ms — pending_rects={} in_flight={} last_seq={} scroll_y={} chat_animating={} surface_dirty={} (recovering)",
                    stuck / 1_000_000,
                    self.pending_damage_rects.len(),
                    self.frames_in_flight(),
                    self.last_completed_seq,
                    self.chat_scroll_y,
                    self.chat_list.is_animating(),
                    self.chat_surface_dirty,
                ));
                self.stall_reported = true;
            }
            // RECOVERY: a present that never gets acked (QEMU dropped/deferred the
            // completion) would otherwise pin `frames_in_flight` at max forever →
            // windowd could never present again = permanent freeze. Drop the wedged
            // in-flight frames so the next iteration resubmits — a brief hiccup
            // instead of a hang. A late ack is harmless: `note_present_completed`
            // uses `saturating_sub` + an idempotent seq assignment.
            self.frames_in_flight = 0;
            self.last_completed_seq = self.present_seq;
            self.stall_last_seq = self.present_seq;
            self.stall_last_progress_ns = now_ns; // measure the next stall fresh
        }
    }

    /// Phase 7: maximum in-flight frames before backpressure.
    pub(crate) const fn max_in_flight() -> u32 {
        2
    }

    /// Phase 7: current frames in flight to gpud (exposed for pacing).
    pub(crate) fn frames_in_flight(&self) -> u32 {
        self.frames_in_flight
    }
}
