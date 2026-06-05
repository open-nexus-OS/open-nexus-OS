// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite display server main loop for `windowd` — retained-mode compositor with
//! tile-based damage tracking, two-pass renderer (shadow-pass → content-pass → cursor),
//! SDF anti-aliased shapes, backdrop blur via nexus-effects, coalesced cursor damage,
//! and paint-only fast-path. Part of TASK-0055/0056/0058/0059.
//!
//! OWNERS: @ui
//! STATUS: Functional (Phases 1–6a: TileMap, LayerCache, library blur, cursor bg, paint-only)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 31 tests (windowd unit) + 9 tests (headless) + 3 tests (tile_map unit)
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

mod backdrop;
mod blur;
mod cache;
mod cursor;
mod damage;
mod filter;
mod font;
mod path_cache;
mod primitives;
mod runtime;
mod scene;
mod sdf;
mod shadow;
mod source;
mod surface;
#[cfg(test)]
mod tests;
mod tile_map;
mod types;

use backdrop::*;
use blur::*;
use cache::*;
use cursor::*;
use damage::*;
use filter::*;
use font::bitmap_font_5x7;
use path_cache::*;
use primitives::*;
use runtime::*;
use scene::*;
use sdf::*;
use shadow::*;
use source::*;
use surface::*;
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
use nexus_abi::{debug_println, nsec, vmo_write, yield_, Handle};
use nexus_ipc::{IpcError, KernelServer, Server as _, Wait};

use crate::error::WindowdError;
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

use crate::layout_panel;
use crate::smoke::VisibleBootstrapMode;
use crate::telemetry::WindowdDisplayTelemetryReport;

pub(crate) const ROUTE_NAME: &str = "windowd";
pub(crate) const PROOF_PANEL_X: u32 = 56;
pub(crate) const PROOF_PANEL_Y: u32 = 440;
pub(crate) const PROOF_PANEL_H: u32 = crate::proof_panel_spec::PANEL_HEIGHT as u32;
pub(crate) const LIVE_FILTER_VARIANTS: [&str; 5] = ["", "a", "ap", "c", "b"];
pub(crate) const FILTER_LIST_PADDING_X: u32 = layout_panel::FILTER_LIST_PADDING;
pub(crate) const FILTER_LIST_PADDING_Y: u32 = layout_panel::FILTER_LIST_PADDING;
pub(crate) const FILTER_LIST_ROW_GAP: u32 = 2;
pub(crate) const FILTER_INPUT_PADDING_X: u32 = 8;
pub(crate) const FILTER_INPUT_FONT_W: u32 = 5;
pub(crate) const FILTER_INPUT_FONT_H: u32 = 7;
pub(crate) const FILTER_INPUT_FONT_SCALE: u32 = 2;
pub(crate) const FILTER_INPUT_FONT_ADVANCE: u32 =
    (FILTER_INPUT_FONT_W + 1) * FILTER_INPUT_FONT_SCALE;
#[cfg(nexus_env = "os")]
pub(crate) const ROW_WRITE_CHUNK: usize = 4;
#[cfg(not(nexus_env = "os"))]
pub(crate) const ROW_WRITE_CHUNK: usize = 32;
pub(crate) const IPC_BATCH_LIMIT: usize = 8;
pub(crate) const VISIBLE_UPDATE_FLUSH_LIMIT: usize = 2;
pub(crate) const BACKDROP_CACHE_ENTRIES: usize = 4;
pub(crate) const BACKDROP_CACHE_MAX_WIDTH: usize = crate::proof_panel_spec::PANEL_WIDTH as usize;
pub(crate) const COMBINED_PANEL_WIDTH: usize = (crate::proof_panel_spec::PANEL_WIDTH
    + crate::proof_panel_spec::PANEL_GAP
    + crate::proof_panel_spec::FILTER_PANEL_WIDTH)
    as usize;
pub(crate) const COMBINED_PANEL_HEIGHT: usize = crate::proof_panel_spec::PANEL_HEIGHT as usize;
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
pub(crate) const CURSOR_BG_MAX_BYTES: usize = 32 * 32 * 4;
pub(crate) const DARK_GLASS_SATURATION_PERCENT: u32 = 140;

pub fn service_main_loop() -> Result<(), &'static str> {
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
        let byte_len: usize = 1280 * 800 * 4;
        if let Ok(handle) = vmo_create(byte_len) {
            let _ = debug_println("windowd: fb vmo create ok");
            runtime.register_framebuffer_vmo(handle);
            // Prioritize first-frame compose/handoff before IPC traffic can starve it.
            let _ = runtime.process_deferred_framebuffer_write();
        } else {
            let _ = debug_println("windowd: ERROR fb vmo create failed");
        }
    }

    let mut recv_frame = [0u8; 512];
    // ARCHITECTURE NOTE: The intended design is fully reactive:
    //   server.recv(Wait::Blocking) → on input → tick animations
    //   Timer/alarm capability → tick animation frames at display refresh rate
    //
    // CURRENT WORKAROUND: Without kernel timer/alarm support, we use
    // NonBlocking receive + yield_() to keep animation frames advancing.
    // This will be replaced with a proper timer-based VSync source
    // (TRACK-DRIVERS-ACCELERATORS, CAND-DRV-005 display timing).
    loop {
        // Keep first-frame handoff deterministic even under sustained inbox load.
        let _ = runtime.process_deferred_framebuffer_write();
        let mut visible_updates_since_flush = 0usize;
        for _ in 0..IPC_BATCH_LIMIT {
            match server.recv_request_with_meta_into(Wait::NonBlocking, &mut recv_frame) {
                Ok((frame_len, _sid, mut moved_cap)) => {
                    let frame = &recv_frame[..frame_len];
                    if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                        let response = encode_visible_state_frame(runtime.visible_state());
                        if let Some(reply) = moved_cap.take() {
                            let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
                        } else {
                            let _ = server.send(&response, Wait::Blocking);
                        }
                    } else if frame_has_op(&frame, OP_UPDATE_VISIBLE_STATE) {
                        let status = match decode_update_visible_state(&frame) {
                            Some(state) => runtime.apply_input_state(state),
                            None => STATUS_MALFORMED,
                        };
                        if runtime.has_pending_damage() {
                            visible_updates_since_flush =
                                visible_updates_since_flush.saturating_add(1);
                        }
                        if let Some(reply) = moved_cap.take() {
                            let response = encode_status(OP_UPDATE_VISIBLE_STATE, status);
                            let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
                        }
                        if runtime.has_pending_damage()
                            && visible_updates_since_flush >= VISIBLE_UPDATE_FLUSH_LIMIT
                        {
                            if let Err(err) = runtime.flush_pending_damage() {
                                let _ = debug_println(flush_error_label(err));
                            }
                            visible_updates_since_flush = 0;
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
                Err(IpcError::WouldBlock)
                | Err(IpcError::Timeout)
                | Err(IpcError::Disconnected)
                | Err(IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => break,
                Err(_) => {}
            }
        }
        if let Err(err) = runtime.flush_pending_damage() {
            let _ = debug_println(flush_error_label(err));
        }
        // Reactive: only drive animation timeline when animations are active.
        // No polling — when no animation is running, we skip the tick entirely.
        if runtime.has_active_animations() {
            runtime.tick(nsec().unwrap_or(0));
        }
        let _ = yield_();
    }
}

fn emit_windowd_telemetry(report: WindowdDisplayTelemetryReport) {
    let mut line = FixedDebugLine::new();
    if write!(
        &mut line,
        "fps: windowd compose_hz={} present_hz={} coalesced={} dropped={} damage_px={} avg_render_us={} max_render_us={}",
        report.compose_hz,
        report.present_hz,
        report.coalesced_events,
        report.dropped_events,
        report.damage_pixels,
        report.avg_render_us,
        report.max_render_us
    )
    .is_err()
    {
        return;
    }
    if let Some(line) = line.as_str() {
        let _ = debug_println(line);
    }
}
