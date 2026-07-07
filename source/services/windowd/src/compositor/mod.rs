// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite display server main loop for `windowd` — retained-mode compositor with
//! tile-based damage tracking, two-pass renderer (shadow-pass → content-pass → cursor),
//! SDF anti-aliased shapes, backdrop blur via nexus-effects, coalesced cursor damage,
//! paint-only fast-path, and GPU-first rendering pipeline (Phase 6c).
//! control/data-plane separation: windowd heap = control plane,
//! shared 16MB VMO = data plane (4-plane: wallpaper / retained-scene / slot-A / slot-B).
//! gpud executes BlitSurface/FillSdfRoundedRect/BlurBackdrop/DrawTiles commands.
//! Part of TASK-0055/0056/0058/0059/0062.
//!
//! OWNERS: @ui
//! STATUS: Phase 6c closed (2026-06-05) — GPU wallpaper path, double-height VMO,
//!   deadline-driven VSync, honest fences, 10× vmo_write reduction
//! API_STABILITY: Unstable
//!
//! ARCHITECTURE:
//!   - Two-pass renderer: `compute_shadow_row` (shadow, zero-allocation),
//!     `draw_proof_surface_row` (content + backdrop blur, zero-allocation)
//!   - Tile-based damage: `TileMap` (64x64 tiles, 260 tiles) with `has_dirty_in_row_range`
//!     gating band writes in `write_rows`
//!   - Retained layer cache: `LayerCache` (insert/get/invalidate) with per-box blit
//!   - Cursor damage coalescing: old/new cursor bounds merge into the normal
//!     band flush path to avoid restore/new-frame flicker
//!   - Paint-only fast-path: `paint_only` flag skips non-paint boxes and backdrop blur
//!   - Zero-copy: `shadow_scratch` + `blur_row_buf` pre-allocated once
//!   - SDF integration: `fill_sdf_circle_row`, `fill_sdf_rounded_rect_row`
//!   - IPC: `KernelServer` receive loop for `OP_GET_VISIBLE_STATE`, `OP_UPDATE_VISIBLE_STATE`
//!
//! DEPENDENCIES:
//!   - nexus-layout, nexus-layout-types: layout computation
//!   - nexus-effects: shadow types, cache infrastructure (blur is zero-allocation inline)
//!   - nexus-sdf: rendering primitives
//!   - nexus-abi, nexus-ipc: kernel IPC
//!   - input-live-protocol: VisibleState wire format
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

// RFC-0067 P5-Final G3: `backdrop` (CPU glass blur/cache for the combined-panel
// glass) deleted — dead on both backends; glass is GPU-rendered.
mod blur;
mod cache;
mod chat;
mod damage;
mod desktop_layer;
mod filter;
mod path_cache;
mod primitives;
mod runtime;
mod scene;
mod sdf;
mod shadow;
mod shell_window;
mod source;
mod surface;
#[cfg(test)]
mod tests;
mod tile_map;
mod types;

use blur::*;
use cache::*;
use damage::*;
use filter::*;
use path_cache::*;
use primitives::*;
use runtime::*;
use sdf::*;
use tile_map::TileMap;
use types::*;

extern crate alloc;

use alloc::vec::Vec;
use core::fmt::Write as _;

use input_live_protocol::{
    decode_update_visible_state, encode_status, encode_visible_state_frame, frame_has_op,
    VisibleState, OP_GET_VISIBLE_STATE, OP_UPDATE_VISIBLE_STATE, STATUS_MALFORMED, STATUS_OK,
    STATUS_UNSUPPORTED,
};
#[cfg(nexus_env = "os")]
use nexus_abi::vmo_create;
use nexus_abi::{debug_println, debug_trace, nsec, vmo_write, yield_, Handle};
use nexus_ipc::{IpcError, KernelServer, Server as _, Wait};

use crate::fixed_sdf;
use crate::ids::CallerCtx;
use crate::live_runtime::{
    premerge_damage_rects, select_glass_quality, DamageRect, GlassQuality, LayoutHotPathIndex,
    TargetDamage,
};
use crate::markers::{
    COMPOSE_READY_MARKER, CURSOR_MOVE_VISIBLE_MARKER, DISPLAY_BOOTSTRAP_MARKER,
    DISPLAY_FIRST_SCANOUT_MARKER, DISPLAY_MODE_MARKER, FOCUS_VISIBLE_MARKER,
    FULL_WINDOW_VISIBLE_MARKER, HOVER_VISIBLE_MARKER, INPUT_ON_MARKER, INPUT_VISIBLE_ON_MARKER,
    KEYBOARD_VISIBLE_MARKER, LAUNCHER_CLICK_OK_MARKER, LAUNCHER_CLICK_VISIBLE_OK_MARKER,
    LAYOUT_ENGINE_ON_MARKER, PRESENT_QUEUED_MARKER, PRESENT_SCHEDULER_ON_MARKER,
    PRESENT_VISIBLE_MARKER, READY_MARKER, SELFTEST_UI_V2_INPUT_OK_MARKER,
    SELFTEST_UI_V2_PRESENT_OK_MARKER, SELFTEST_UI_VISIBLE_INPUT_OK_MARKER,
    SELFTEST_UI_VISIBLE_PRESENT_MARKER, SELFTEST_UI_VISIBLE_WHEEL_OK_MARKER,
    SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER, TEXT_WRAPPING_ON_MARKER, VISIBLE_BACKEND_MARKER,
    WALLPAPER_FAIL, WHEEL_VISIBLE_MARKER,
};
use nexus_effects::{blur_separable_zero_alloc, ShadowArena};
use nexus_layout::LayoutResult;
use nexus_layout_types::{FxPx, Rgba8};

use crate::smoke::VisibleBootstrapMode;
use crate::telemetry::WindowdDisplayTelemetryReport;

pub(crate) const ROUTE_NAME: &str = "windowd";
// Phase 6c: control-plane / data-plane separation.
// Data plane: all pixel data lives in shared VMOs, rendered by gpud.
//   VMO layout (16MB, 1280x3200, 4-plane):
//     Plane 0: rows    0.. 799 — wallpaper source  (offset 0x000000)
//     Plane 1: rows  800..1599 — retained scene    (offset 0x3E8000)
//     Plane 2: rows 1600..2399 — frame ring slot A  (offset 0x7D0000)
//     Plane 3: rows 2400..3199 — frame ring slot B  (offset 0xBB8000)
pub(crate) const DISPLAY_WIDTH: u32 = 1280;
pub(crate) const DISPLAY_HEIGHT: u32 = 800;
// 6400 rows: 4 display planes (3200) + surface atlas (3200) for cached layers.
// SSOT for the atlas layout is `crate::atlas`. gpud mirrors this value.
pub(crate) const RESOURCE_HEIGHT: u32 = crate::atlas::RESOURCE_HEIGHT;
pub(crate) const DISPLAY_OFFSET_BYTES: usize = 8_192_000; // Plane 2 / slot A
pub(crate) const DISPLAY_SLOT_B_OFFSET_BYTES: usize = 12_288_000;
/// Plane 1 — retained scene. The CPU compositor renders the full cursor-free
/// scene (wallpaper + panels + text + glass) here. gpud blits damage regions
/// from this plane to the display plane per frame and overlays the cursor.
pub(crate) const RETAINED_OFFSET_BYTES: usize = 4_096_000; // Plane 1 (0x3E8000)
/// Row offset of the retained plane within the VMO (RETAINED_OFFSET_BYTES / row_bytes).
/// 4_096_000 / (1280*4) = 800. Used as the BlitSurface source row base.
pub(crate) const RETAINED_ROW_OFFSET: u32 = 800;
/// Absolute VMO row where the display plane starts (DISPLAY_OFFSET_BYTES / stride).
/// 8_192_000 / (1280*4) = 1600. Used as the BlitAbsolute source/dst for blur cache writes.
pub(crate) const DISPLAY_ROW_OFFSET: u32 = 1600;
/// Absolute VMO row where Plane 3 (Slot B) starts — repurposed as blur cache.
/// 12_288_000 / (1280*4) = 2400.
pub(crate) const BLUR_CACHE_ROW_OFFSET: u32 = 2400;
/// X pixel offset of the sidebar at its resting (fully open) position.
/// 1280 - 320 = 960. Blur cache is always precomputed for the full 320px at this x.
pub(crate) const SIDEBAR_REST_X: u32 = 960;
/// Glass button blur cache in Plane 3 — stored at x=0 (leftmost columns).
/// Does not conflict with sidebar cache at x=960..1279.
pub(crate) const BUTTON_BLUR_CACHE_ABS_X: u32 = 0;
pub(crate) const BUTTON_BLUR_CACHE_ABS_ROW: u32 = BLUR_CACHE_ROW_OFFSET;
pub(crate) const PROOF_PANEL_X: u32 = 56;
pub(crate) const PROOF_PANEL_Y: u32 = 440;
pub(crate) const PROOF_PANEL_H: u32 = 260;

/// Shell-P2b: when `true`, source `proof_layouts` from the flat desktop-shell
/// scene and suppress the rich proof/glass overlays. The flat-rect render was a
/// regression (no glass/shadow/rounding), so this is `false`: we keep the rich
/// glass UI (chat window + buttons + sidebar) and add a real glass topbar instead.
/// Kept as a switch for the layout-driven path.
pub(crate) const USE_DESKTOP_SHELL: bool = false;

// The former `SHELL_TOPBAR` / `SHELL_SIDEPANEL` compile-time constants are gone:
// the glass topbar + side panel chrome is now driven at runtime by the shell
// configuration resolved from SystemUI's manifest registry
// (`DisplayServerRuntime.shell_config.desktop_chrome`), so the active shell —
// not a hardcoded constant — decides whether the desktop chrome is composited.

/// On-screen origin of the composited scene. The desktop shell sits near the
/// top-left with a small inset; the proof panel keeps its historic placement.
pub(crate) const SCENE_ORIGIN_X: u32 = if USE_DESKTOP_SHELL { 24 } else { PROOF_PANEL_X };
pub(crate) const SCENE_ORIGIN_Y: u32 = if USE_DESKTOP_SHELL { 24 } else { PROOF_PANEL_Y };
pub(crate) const LIVE_FILTER_VARIANTS: [&str; 5] = ["", "a", "ap", "c", "b"];
pub(crate) const FILTER_LIST_PADDING_X: u32 = 4;
pub(crate) const FILTER_LIST_PADDING_Y: u32 = 4;
pub(crate) const FILTER_LIST_ROW_GAP: u32 = 2;
pub(crate) const FILTER_INPUT_PADDING_X: u32 = 8;
pub(crate) const FILTER_INPUT_FONT_W: u32 = 5;
pub(crate) const FILTER_INPUT_FONT_H: u32 = 7;
pub(crate) const FILTER_INPUT_FONT_SCALE: u32 = 2;
pub(crate) const FILTER_INPUT_FONT_ADVANCE: u32 =
    (FILTER_INPUT_FONT_W + 1) * FILTER_INPUT_FONT_SCALE;
#[cfg(nexus_env = "os")]
pub(crate) const ROW_WRITE_CHUNK: usize = 40;
#[cfg(not(nexus_env = "os"))]
pub(crate) const ROW_WRITE_CHUNK: usize = 32;
// Drain a generous burst of input/IPC per loop iteration so a flood of pointer
// events (hidrawd can emit ~800/s during a drag) is consumed in one frame and the
// queue can't grow stale — the wheel deltas among them are coalesced into a single
// scroll step (`commit_scroll_input`), so a bigger batch costs no extra scrolling.
pub(crate) const IPC_BATCH_LIMIT: usize = 64;
pub(crate) const BACKDROP_CACHE_ENTRIES: usize = 4;
// C1: dimensions inlined (was `proof_panel_spec` PANEL_WIDTH 610 / PANEL_HEIGHT
// 260 / +GAP 16 +FILTER_PANEL_WIDTH 200 = 826). These now size the backdrop/
// layer caches only; the proof panel itself is deleted.
pub(crate) const BACKDROP_CACHE_MAX_WIDTH: usize = 610;
pub(crate) const COMBINED_PANEL_WIDTH: usize = 826;
pub(crate) const COMBINED_PANEL_HEIGHT: usize = 260;
#[cfg(nexus_env = "os")]
pub(crate) const GLASS_LAYER_SCALE: u32 = 8;
#[cfg(not(nexus_env = "os"))]
pub(crate) const GLASS_LAYER_SCALE: u32 = 4;
pub(crate) const GLASS_LAYER_MAX_WIDTH: usize =
    COMBINED_PANEL_WIDTH.div_ceil(GLASS_LAYER_SCALE as usize);
pub(crate) const GLASS_LAYER_MAX_HEIGHT: usize =
    COMBINED_PANEL_HEIGHT.div_ceil(GLASS_LAYER_SCALE as usize);
pub(crate) const GLASS_LAYER_MAX_BYTES: usize = GLASS_LAYER_MAX_WIDTH * GLASS_LAYER_MAX_HEIGHT * 4;
pub(crate) const DARK_GLASS_RADIUS: u32 = 12;
pub(crate) const DARK_GLASS_BLUR_RADIUS: u32 = 20;
pub(crate) const DARK_GLASS_TINT: Rgba8 = Rgba8::new(28, 28, 30, 178);
pub(crate) const DARK_GLASS_BORDER: Rgba8 = Rgba8::new(255, 255, 255, 26);
pub(crate) const SOFT_PANEL_SHADOW_OFFSET_Y: i32 = 4;
pub(crate) const SOFT_PANEL_SHADOW_BLUR_RADIUS: u32 = 30;
pub(crate) const SOFT_PANEL_SHADOW_ALPHA: u32 = 128;
/// Shared window drop shadow (subtle, macOS-like floating panel): a soft, not
/// heavy, penumbra offset slightly downward. Used by both the chat and search
/// ShellWindow frames so they cast an identical shadow. The compositor restores
/// this halo from the retained plane before each (translucent) redraw so it
/// never accumulates. See `build_scene_cb_into` step 1a.
pub(crate) const CHAT_SHADOW_BLUR: u32 = 18;
pub(crate) const CHAT_SHADOW_OFFSET_Y: i32 = 5;
pub(crate) const CHAT_SHADOW_ALPHA: u8 = 90;
pub(crate) const PATH_CACHE_ENTRIES: usize = 2;
pub(crate) const PATH_CACHE_MAX_SIDE: usize = 16;
pub(crate) const PATH_CACHE_MAX_PIXELS: usize = PATH_CACHE_MAX_SIDE * PATH_CACHE_MAX_SIDE * 4;
pub(crate) const LAYER_CACHE_MAX_BYTES: usize = 4 * 1024;
pub(crate) const LAYER_CACHE_MAX_LAYER_BYTES: usize = PATH_CACHE_MAX_PIXELS;
pub(crate) const TILE_SIZE: u32 = 64;
pub(crate) const TILES_X: usize = 20; // 1280 / 64
pub(crate) const TILES_Y: usize = 13; // 800 / 64 rounded up
pub(crate) const TILE_COUNT: usize = TILES_X * TILES_Y;
pub(crate) const TILE_DIRTY_WORDS: usize = (TILE_COUNT + 63) / 64;
#[cfg(nexus_env = "os")]
pub(crate) const WINDOWD_SHADOW_ARENA_SIZE: usize = 8 * 1024;
#[cfg(not(nexus_env = "os"))]
pub(crate) const WINDOWD_SHADOW_ARENA_SIZE: usize = 16 * 1024;
pub(crate) const COL_SCRATCH_SIZE: usize = WINDOWD_SHADOW_ARENA_SIZE;
pub(crate) const SHADOW_BOX_CACHE_ENTRIES: usize = 8;
pub(crate) const SHADOW_CACHE_MAX_DOWNSCALE: u8 = 16;
pub(crate) const DARK_GLASS_SATURATION_PERCENT: u32 = 140;
#[cfg(nexus_env = "os")]
const OP_TIMER_FIRED: u8 = 0x30;

#[cfg(nexus_env = "os")]
fn decode_timer_fired_now_ns(frame: &[u8]) -> Option<u64> {
    if frame.len() < 29 || frame[0] != OP_TIMER_FIRED {
        return None;
    }
    Some(u64::from_le_bytes([
        frame[21], frame[22], frame[23], frame[24], frame[25], frame[26], frame[27], frame[28],
    ]))
}

/// Dispatch ONE client request frame (input state, surface create/present/
/// destroy/events, or an unknown op). The SINGLE source of truth for the
/// windowd server protocol — called from BOTH recv sites (the drain batch AND
/// the idle blocking recv). The idle recv previously handled only the two
/// visible-state ops and answered everything else UNSUPPORTED: a client
/// surface present arriving while the desktop was idle (the exact state after
/// an app window opens and the user taps) was silently dropped — the "+ reacts
/// only once" bug. Factoring both sites through here makes a missing branch
/// impossible.
#[cfg(nexus_env = "os")]
fn dispatch_client_frame(
    runtime: &mut DisplayServerRuntime,
    server: &KernelServer,
    frame: &[u8],
    mut moved_cap: Option<nexus_ipc::ReplyCap>,
) {
    use nexus_ipc::Server as _;
    if frame_has_op(frame, OP_GET_VISIBLE_STATE) {
        let response = encode_visible_state_frame(runtime.visible_state());
        if let Some(reply) = moved_cap.take() {
            let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
        } else {
            let _ = server.send(&response, Wait::Blocking);
        }
    } else if frame_has_op(frame, OP_UPDATE_VISIBLE_STATE) {
        // Frame-aligned coalescing: STAGE the update (latest sample wins,
        // wheel sums); applied ONCE per frame by apply_staged_input. Reply
        // immediately so inputd is never blocked.
        let status = match decode_update_visible_state(frame) {
            Some(state) => runtime.stage_input_state(state),
            None => STATUS_MALFORMED,
        };
        if let Some(reply) = moved_cap.take() {
            let response = encode_status(OP_UPDATE_VISIBLE_STATE, status);
            let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
        }
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::client_surface::OP_SURFACE_EVENTS)
    {
        // ADR-0042 per-app event channel: the moved capability is the
        // channel's SEND half (execd-attached). All app-bound frames go out
        // on it. No reply frame; the attach marker is the proof.
        let send_slot = moved_cap.take().map(|cap| {
            let slot = cap.slot();
            core::mem::forget(cap); // keep the slot alive (no close)
            slot
        });
        runtime.attach_app_event_channel(send_slot);
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::client_surface::OP_SURFACE_CREATE)
    {
        // ADR-0042: the moved capability IS the app's surface VMO (gpud-attach
        // pattern), NOT a reply cap. Retain its slot; the ack returns over the
        // app's dedicated event channel (fallback: shared response endpoint).
        let vmo_slot = moved_cap.take().map(|cap| {
            let slot = cap.slot();
            core::mem::forget(cap); // keep the slot alive (no close)
            slot
        });
        let ack = runtime.handle_surface_create(frame, vmo_slot);
        if !runtime.send_app_frame(&ack) {
            let _ = server.send(&ack, Wait::Blocking);
        }
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::client_surface::OP_SURFACE_PRESENT)
    {
        let ack = runtime.handle_surface_present(frame);
        if runtime.send_app_frame(&ack) {
            // delivered on the dedicated channel
        } else if let Some(reply) = moved_cap.take() {
            let _ = reply.reply_and_close_wait(&ack, Wait::Blocking);
        } else {
            let _ = server.send(&ack, Wait::Blocking);
        }
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::client_surface::OP_SURFACE_DESTROY)
    {
        let ack = runtime.handle_surface_destroy(frame);
        if runtime.send_app_frame(&ack) {
            // delivered on the dedicated channel
        } else if let Some(reply) = moved_cap.take() {
            let _ = reply.reply_and_close_wait(&ack, Wait::Blocking);
        } else {
            let _ = server.send(&ack, Wait::Blocking);
        }
    } else {
        let op = frame.get(3).copied().unwrap_or(0);
        let response = encode_status(op, STATUS_UNSUPPORTED);
        if let Some(reply) = moved_cap.take() {
            let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
        } else {
            let _ = server.send(&response, Wait::Blocking);
        }
    }
}

pub fn service_main_loop() -> Result<(), &'static str> {
    // Verdict folding: fold windowd's scattered `debug_println` bring-up markers (route/shell/
    // wallpaper/handoff/present…) into one `windowd N/N` grid line in interactive boots. Flushed
    // once the present scheduler is on; FAIL lines print live; proof boots emit everything raw.
    nexus_abi::service_verdict_arm();
    let server = match KernelServer::new_for(ROUTE_NAME) {
        Ok(s) => s,
        Err(_) => {
            let _ = debug_println("windowd: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| "windowd: init fail kernel-server")?
        }
    };
    let mut runtime = match DisplayServerRuntime::new() {
        Ok(rt) => {
            let _ = debug_println(READY_MARKER);
            rt
        }
        Err(_) => {
            let _ = debug_println("windowd: init fail display-server (wallpaper?)");
            let _ = debug_println(WALLPAPER_FAIL);
            return Err("windowd: init fail display-server");
        }
    };

    // GPU-only architecture: windowd is the sole display owner.
    // It always creates its own framebuffer VMO — no fbdevd, no ramfb,
    // no handoff from another service. gpud provides scanout on demand.
    #[cfg(nexus_env = "os")]
    {
        let _ = debug_println("windowd: backend=gpu");
        let byte_len: usize = (DISPLAY_WIDTH as usize) * (RESOURCE_HEIGHT as usize) * 4;
        if let Ok(handle) = vmo_create(byte_len) {
            let _ = debug_println("windowd: fb vmo create ok");
            runtime.register_framebuffer_vmo(handle);
            // Write source frame (wallpaper) to VMO Plane 0 once.
            // Control-plane -> data-plane: 4MB wallpaper moves from heap to shared VMO.
            let _ = runtime.write_source_frame_to_vmo();
            let _ = runtime.process_deferred_framebuffer_write();
        } else {
            let _ = debug_println("windowd: ERROR fb vmo create failed");
        }
    }

    let mut recv_frame = [0u8; 512];
    // Phase D.1: Keep the NonBlocking batch for responsive message handling,
    // but replace the bottom yield_() with a kernel deadline-driven wait.
    //   - Idle:     Wait::Blocking           → zero CPU, wakes on input only
    //   - Active:   Wait::Timeout(interval)  → wakes on input or animation tick
    #[cfg(nexus_env = "os")]
    let (pacer_notify_slot, _) = server.slots();
    #[cfg(nexus_env = "os")]
    let mut pacer_timer_cap: Option<u32> = None;
    #[cfg(nexus_env = "os")]
    let mut pacer_timer_armed = false;
    #[cfg(nexus_env = "os")]
    let mut pacer_timer_log_emitted = false;
    // Phase 7: unified pacing timer drives frame submission at display refresh rate.
    const PACER_INTERVAL_NS: u64 = 8_333_333; // 120 Hz
                                              // Animation pacing. The supervisor-timer IRQ is now ENABLED in the kernel
                                              // (`timer_irq` default + `enable_timer_interrupts` in kmain), so the 120Hz
                                              // one-shot timer cap armed below delivers OP_TIMER_FIRED reactively while an
                                              // animation runs. The monotonic-clock self-pacing (the WouldBlock arm below,
                                              // reached via the NonBlocking recv) is retained as a robust fallback so a
                                              // missed/idle-time tick can never freeze the spring; `tick` integrates real
                                              // elapsed time, so the exact wake rate only affects how many frames we emit,
                                              // not the animation's duration or final state. (A fully poll-free wait
                                              // depends on idle-time timer-cap delivery and is a separate step.)
    let mut last_anim_tick_ns: u64 = 0;
    loop {
        runtime.drain_gpud_replies();
        // Stall watchdog: self-reports a "stopped responding" present stall to the
        // UART log (build/logs/*/uart.log). Cheap — one nsec() + integer checks per
        // iteration; only formats on an actual stall (rate-limited).
        #[cfg(nexus_env = "os")]
        runtime.watchdog_check(nexus_abi::nsec().unwrap_or(0));
        let _ = runtime.process_deferred_framebuffer_write();
        #[cfg(nexus_env = "os")]
        {
            // Reactive pacing: arm the 120Hz timer ONLY while an animation is
            // running. Cursor moves, hover, clicks and other input arrive as IPC
            // messages that wake the blocking recv below and are flushed directly —
            // they don't need the pacer. When idle (no animation), the timer stays
            // disarmed and windowd blocks on IPC: zero wakes, zero polling. This is
            // what eliminates the per-frame busy loop.
            //
            // Also keep the pacer alive while damage is still pending: gpud's ack
            // replies arrive on the gpud client, not the server, so a backpressured
            // flush needs a timer wake to retry. Once damage clears and no animation
            // runs, the pacer disarms and windowd goes fully idle.
            let handoff_done = !runtime.is_handoff_pending();
            // Session probe (TASK-0065B): after the handoff, ask sessiond for
            // the session decision on its own cadence. While unresolved it
            // needs the pacer's wakes (the loop otherwise blocks on IPC);
            // bounded — resolution or the auto-shell fallback disarms it.
            let session_pending =
                runtime.session_probe_tick(nexus_abi::nsec().unwrap_or(0));
            // Persisted-theme probe (TASK-0072 Phase 10): same cadence — restore
            // `ui.theme.mode` from settingsd once it binds; bounded, then default.
            let theme_pending = runtime.theme_probe_tick(nexus_abi::nsec().unwrap_or(0));
            // Un-acked presents keep the pacer alive too: gpud's ack/NACK replies
            // arrive on the gpud client, not the server recv below — an idle-blocked
            // windowd would otherwise only drain a present NACK (P0.3 requeue
            // self-heal) on the next unrelated input. Bounded: acks normally land
            // within a frame, so this costs at most a tick or two.
            let needs_pacing = runtime.has_active_animations()
                || runtime.has_pending_damage()
                || runtime.frames_in_flight() > 0
                || session_pending
                || theme_pending;
            if handoff_done && !pacer_timer_armed && needs_pacing {
                if pacer_timer_cap.is_none() {
                    // One-shot timer (interval_ns = 0): windowd rearms it every tick
                    // below. A periodic timer (non-zero interval) would auto-rearm in
                    // the kernel and keep firing at 120Hz forever after the animation
                    // ends — windowd would never go idle, and each manual timer_set
                    // would hit AlreadyArmed. One-shot auto-disarms on fire, so when
                    // pacing stops we simply stop rearming and the service goes fully
                    // idle (zero wakes), which is the whole point of reactive pacing.
                    match nexus_abi::timer_create(pacer_notify_slot, 0) {
                        Ok(cap) => pacer_timer_cap = Some(cap),
                        Err(_) => {
                            if !pacer_timer_log_emitted {
                                let _ = debug_println("windowd: pacer timer create failed");
                                pacer_timer_log_emitted = true;
                            }
                        }
                    }
                }
                if let Some(timer_cap) = pacer_timer_cap {
                    if let Ok(now) = nsec() {
                        let deadline = now.saturating_add(PACER_INTERVAL_NS);
                        match nexus_abi::timer_set(timer_cap, deadline) {
                            Ok(()) => pacer_timer_armed = true,
                            Err(_) => {
                                if !pacer_timer_log_emitted {
                                    let _ = debug_println("windowd: pacer timer arm failed");
                                    pacer_timer_log_emitted = true;
                                }
                            }
                        }
                    }
                }
            } else if pacer_timer_armed && !needs_pacing {
                // Pacing no longer needed but a one-shot is still armed (animation
                // ended mid-interval). Cancel it so the trailing tick never fires and
                // windowd blocks on IPC until the next real input. Idempotent: the
                // kernel disarms the timer and no OP_TIMER_FIRED is delivered.
                if let Some(timer_cap) = pacer_timer_cap {
                    let _ = nexus_abi::timer_cancel(timer_cap);
                }
                pacer_timer_armed = false;
            }
        }
        for _ in 0..IPC_BATCH_LIMIT {
            match server.recv_request_with_meta_into(Wait::NonBlocking, &mut recv_frame) {
                Ok((frame_len, _sid, mut moved_cap)) => {
                    let frame = &recv_frame[..frame_len];
                    #[cfg(nexus_env = "os")]
                    if let Some(now_ns) = decode_timer_fired_now_ns(frame) {
                        // Phase 7: Pacing tick — drive animation update AND frame flush.
                        pacer_timer_armed = false;
                        if runtime.has_active_animations() {
                            runtime.tick(now_ns);
                        }
                        // Submit frame if pending damage and a ring slot is free.
                        if runtime.has_pending_damage()
                            && runtime.frames_in_flight()
                                < runtime::DisplayServerRuntime::max_in_flight()
                        {
                            let _ = runtime.flush_pending_damage();
                        }
                        continue;
                    }
                    dispatch_client_frame(&mut runtime, &server, frame, moved_cap.take());
                }
                Err(IpcError::WouldBlock)
                | Err(IpcError::Timeout)
                | Err(IpcError::Disconnected)
                | Err(IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => break,
                Err(_) => {}
            }
        }
        // Frame-aligned input: apply the staged sample (latest cursor/buttons +
        // summed wheel) ONCE — one hit-test/hover/cursor-move/scroll per frame,
        // independent of how many raw events arrived (the Android Choreographer
        // model). Then commit the coalesced scroll step.
        let _ = runtime.apply_staged_input();
        let _ = runtime.commit_scroll_input();
        // Phase 4: skip present while handoff is pending — the VMO must arrive
        // at gpud before any present-damage frames.
        if !runtime.is_handoff_pending() {
            if let Err(err) = runtime.flush_pending_damage() {
                let _ = debug_println(flush_error_label(err));
            }
        }
        // Phase D.1 / RFC-0033: deadline-driven sleep instead of busy yield_().
        // During animation/present the supervisor timer IRQ is now ENABLED, so the one-shot
        // pacer timer-cap armed above delivers OP_TIMER_FIRED into our endpoint at the frame
        // deadline and wakes a BLOCKING recv — deterministic ~120 Hz with zero polling, paced
        // by the timer-cap's fixed deadline (process_expired_timers), not a recv-timeout clock
        // (see memory: recv-timeout self-pace can't hit 120 Hz). We block only when that timer
        // is actually armed to wake us; otherwise the monotonic self-pace fallback (the
        // WouldBlock arm below) keeps the frame alive so a failed/absent timer can't freeze it.
        // This replaces the old NonBlocking + yield_() spin (was written when timer IRQ was off)
        // — the source of the high `spin_hz` / low `present_hz`.
        #[cfg(nexus_env = "os")]
        let animation_wait = if pacer_timer_armed { Wait::Blocking } else { Wait::NonBlocking };
        #[cfg(not(nexus_env = "os"))]
        let animation_wait = Wait::NonBlocking;
        let wait = if runtime.is_handoff_pending() {
            Wait::NonBlocking
        } else if runtime.has_active_animations() || runtime.has_pending_damage() {
            animation_wait
        } else {
            // Fully idle: block until the next input message. Zero CPU.
            Wait::Blocking
        };
        match server.recv_request_with_meta_into(wait, &mut recv_frame) {
            Ok((frame_len, _sid, mut moved_cap)) => {
                let frame = &recv_frame[..frame_len];
                #[cfg(nexus_env = "os")]
                if let Some(now_ns) = decode_timer_fired_now_ns(frame) {
                    // One-shot timer auto-disarms on fire — mark as disarmed so
                    // the pacing arm block re-arms it for the next tick.
                    pacer_timer_armed = false;
                    if runtime.has_active_animations() {
                        runtime.tick(now_ns);
                    }
                    // Submit frame if pending damage and a ring slot is free.
                    if runtime.has_pending_damage()
                        && runtime.frames_in_flight()
                            < runtime::DisplayServerRuntime::max_in_flight()
                    {
                        let _ = runtime.flush_pending_damage();
                    }
                    continue;
                }
                // Same complete dispatch as the drain batch — surface
                // create/present/destroy/events are handled here too. The idle
                // recv used to answer them UNSUPPORTED (a client present while
                // the desktop was idle was dropped → the "+ reacts once" bug).
                dispatch_client_frame(&mut runtime, &server, frame, moved_cap.take());
            }
            Err(IpcError::Timeout) | Err(IpcError::WouldBlock) => {
                // No message ready. If an animation is running, this is our
                // self-paced frame tick: advance the springs on the monotonic
                // clock (gated to ~120Hz) and present. `tick` integrates real
                // elapsed time, so this converges correctly regardless of the
                // exact poll cadence.
                if runtime.has_active_animations() || runtime.has_pending_damage() {
                    let now_ns = nsec().unwrap_or(0);
                    if now_ns.saturating_sub(last_anim_tick_ns) >= PACER_INTERVAL_NS {
                        last_anim_tick_ns = now_ns;
                        if runtime.has_active_animations() {
                            runtime.tick(now_ns);
                        }
                        if runtime.has_pending_damage()
                            && runtime.frames_in_flight()
                                < runtime::DisplayServerRuntime::max_in_flight()
                        {
                            let _ = runtime.flush_pending_damage();
                        }
                    }
                    // Cooperative yield: hand the CPU to gpud (to render the frame
                    // we just submitted) and inputd (to deliver the next event)
                    // between polls. Without this, the NonBlocking loop would
                    // monopolize the single hart and gpud would never run.
                    //
                    // Count this empty wake-up: it's the busy-poll cost of having
                    // no timer IRQ (RFC-0062). Surfaced as `spin_hz` — idle ~= 0,
                    // high during animation = the work-vs-pacing diagnostic.
                    runtime.record_poll_spin();
                    let _ = yield_();
                }
            }
            Err(_) => {}
        }
    }
}

fn emit_windowd_telemetry(report: WindowdDisplayTelemetryReport) {
    let mut line = FixedDebugLine::new();
    if write!(
        &mut line,
        "fps: windowd compose_hz={} present_hz={} coalesced={} dropped={} damage_px={} avg_render_us={} max_render_us={} spin_hz={}",
        report.compose_hz,
        report.present_hz,
        report.coalesced_events,
        report.dropped_events,
        report.damage_pixels,
        report.avg_render_us,
        report.max_render_us,
        report.spin_hz
    )
    .is_err()
    {
        return;
    }
    if let Some(line) = line.as_str() {
        // Periodic compositor counters: off by default, one runtime flag away. Phase 3
        // promotes these to metricsd counters.
        let _ = debug_trace(line);
    }
}
