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
//!   - IPC: `KernelServer` receive loop for `OP_GET_VISIBLE_STATE`, `OP_SEND_COMPOSED_FRAME_VMO`, `OP_UPDATE_VISIBLE_STATE`
//!
//! DEPENDENCIES:
//!   - nexus-layout, nexus-layout-types: layout computation
//!   - nexus-effects: shadow types, cache infrastructure (blur is zero-allocation inline)
//!   - nexus-sdf: rendering primitives
//!   - nexus-abi, nexus-ipc: kernel IPC
//!   - input-live-protocol: VisibleState wire format
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md
extern crate alloc;

use alloc::vec::Vec;
use core::fmt::Write as _;

use input_live_protocol::{
    decode_update_visible_state, encode_status, encode_visible_state_frame, frame_has_op,
    VisibleState, OP_GET_VISIBLE_STATE, OP_SEND_COMPOSED_FRAME_VMO, OP_UPDATE_VISIBLE_STATE,
    STATUS_MALFORMED, STATUS_OK, STATUS_UNSUPPORTED,
};
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
    WHEEL_VISIBLE_MARKER,
};
use nexus_effects::{blur_separable_zero_alloc, ShadowArena};
use nexus_layout::LayoutResult;
use nexus_layout_types::{FxPx, Rgba8};

use crate::layout_panel;
use crate::smoke::VisibleBootstrapMode;
use crate::telemetry::WindowdDisplayTelemetryReport;

const ROUTE_NAME: &str = "windowd";
const PROOF_PANEL_X: u32 = 56;
const PROOF_PANEL_Y: u32 = 440;
const PROOF_PANEL_H: u32 = crate::proof_panel_spec::PANEL_HEIGHT as u32;
const LIVE_FILTER_VARIANTS: [&str; 5] = ["", "a", "ap", "c", "b"];
const FILTER_LIST_PADDING_X: u32 = layout_panel::FILTER_LIST_PADDING;
const FILTER_LIST_PADDING_Y: u32 = layout_panel::FILTER_LIST_PADDING;
const FILTER_LIST_ROW_GAP: u32 = 2;
const FILTER_INPUT_PADDING_X: u32 = 8;
const FILTER_INPUT_FONT_W: u32 = 5;
const FILTER_INPUT_FONT_H: u32 = 7;
const FILTER_INPUT_FONT_SCALE: u32 = 2;
const FILTER_INPUT_FONT_ADVANCE: u32 = (FILTER_INPUT_FONT_W + 1) * FILTER_INPUT_FONT_SCALE;
const ROW_WRITE_CHUNK: usize = 4;
const IPC_BATCH_LIMIT: usize = 8;
const VISIBLE_UPDATE_FLUSH_LIMIT: usize = 2;
const BACKDROP_CACHE_ENTRIES: usize = 4;
const BACKDROP_CACHE_MAX_WIDTH: usize = crate::proof_panel_spec::PANEL_WIDTH as usize;
const COMBINED_PANEL_WIDTH: usize = (crate::proof_panel_spec::PANEL_WIDTH
    + crate::proof_panel_spec::PANEL_GAP
    + crate::proof_panel_spec::FILTER_PANEL_WIDTH) as usize;
const COMBINED_PANEL_HEIGHT: usize = crate::proof_panel_spec::PANEL_HEIGHT as usize;
const GLASS_LAYER_SCALE: u32 = 4;
const GLASS_LAYER_MAX_WIDTH: usize = COMBINED_PANEL_WIDTH.div_ceil(GLASS_LAYER_SCALE as usize);
const GLASS_LAYER_MAX_HEIGHT: usize = COMBINED_PANEL_HEIGHT.div_ceil(GLASS_LAYER_SCALE as usize);
const GLASS_LAYER_MAX_BYTES: usize = GLASS_LAYER_MAX_WIDTH * GLASS_LAYER_MAX_HEIGHT * 4;
const DARK_GLASS_RADIUS: u32 = 12;
const DARK_GLASS_BLUR_RADIUS: u32 = 20;
const DARK_GLASS_TINT: Rgba8 = Rgba8::new(28, 28, 30, 178);
const DARK_GLASS_BORDER: Rgba8 = Rgba8::new(255, 255, 255, 26);
const SOFT_PANEL_SHADOW_OFFSET_Y: i32 = 4;
const SOFT_PANEL_SHADOW_BLUR_RADIUS: u32 = 30;
const SOFT_PANEL_SHADOW_ALPHA: u32 = 128;
const PATH_CACHE_ENTRIES: usize = 2;
const PATH_CACHE_MAX_SIDE: usize = 16;
const PATH_CACHE_MAX_PIXELS: usize = PATH_CACHE_MAX_SIDE * PATH_CACHE_MAX_SIDE * 4;
const LAYER_CACHE_MAX_BYTES: usize = 4 * 1024;
const LAYER_CACHE_MAX_LAYER_BYTES: usize = PATH_CACHE_MAX_PIXELS;
const TILE_SIZE: u32 = 64;
const TILES_X: usize = 20; // 1280 / 64
const TILES_Y: usize = 13; // 800 / 64 rounded up
const TILE_COUNT: usize = TILES_X * TILES_Y;
const TILE_DIRTY_WORDS: usize = (TILE_COUNT + 63) / 64;
const WINDOWD_SHADOW_ARENA_SIZE: usize = 16 * 1024;
const COL_SCRATCH_SIZE: usize = WINDOWD_SHADOW_ARENA_SIZE;
const SHADOW_BOX_CACHE_ENTRIES: usize = 8;
const SHADOW_CACHE_MAX_DOWNSCALE: u8 = 16;
const CURSOR_BG_MAX_BYTES: usize = 32 * 32 * 4;
const DARK_GLASS_SATURATION_PERCENT: u32 = 140;

struct FixedDebugLine {
    buf: [u8; 256],
    len: usize,
}

impl FixedDebugLine {
    const fn new() -> Self {
        Self {
            buf: [0; 256],
            len: 0,
        }
    }

    fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.buf[..self.len]).ok()
    }
}

impl core::fmt::Write for FixedDebugLine {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.saturating_add(s.len());
        if end > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub fn service_main_loop() -> Result<(), &'static str> {
    let server = match KernelServer::new_for(ROUTE_NAME) {
        Ok(s) => s,
        Err(_) => {
            let _ = debug_println("windowd: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| "windowd: init fail kernel-server")?
        }
    };
    let mut runtime =
        DisplayServerRuntime::new().map_err(|_| "windowd: init fail display-server")?;
    let _ = debug_println(READY_MARKER);
    let mut recv_frame = [0u8; 512];
    loop {
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
                    } else if frame_has_op(&frame, OP_SEND_COMPOSED_FRAME_VMO) {
                        let status = if let Some(cap) = moved_cap.take() {
                            let status = runtime.register_framebuffer(cap.slot());
                            if status != STATUS_OK {
                                cap.close();
                            }
                            status
                        } else {
                            STATUS_MALFORMED
                        };
                        let response = encode_status(OP_SEND_COMPOSED_FRAME_VMO, status);
                        let _ = server.send(&response, Wait::Blocking);
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
        runtime.tick(nsec().unwrap_or(0));
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

/// Per-box shadow cache entry: stores arena offset for pre-rendered full-box shadow.
/// Zero heap allocation — fixed-size array, linear-probe lookup.
#[derive(Clone, Copy)]
struct ShadowBoxCacheEntry {
    key: u64,
    arena_offset: usize,
    width: u32,
    height: u32,
    cache_width: u32,
    cache_height: u32,
    scale: u8,
    valid: bool,
}

impl ShadowBoxCacheEntry {
    const fn empty() -> Self {
        Self {
            key: 0,
            arena_offset: 0,
            width: 0,
            height: 0,
            cache_width: 0,
            cache_height: 0,
            scale: 1,
            valid: false,
        }
    }
}

#[derive(Clone, Copy)]
struct RenderClip {
    start_x: u32,
    end_x: u32,
}

impl RenderClip {
    const fn full(width: u32) -> Self {
        Self {
            start_x: 0,
            end_x: width,
        }
    }

    fn new(start_x: u32, end_x: u32, width: u32) -> Self {
        Self {
            start_x: start_x.min(width),
            end_x: end_x.min(width),
        }
    }
}

struct DisplayServerRuntime {
    mode: VisibleBootstrapMode,
    source_frame: SourceFrame,
    source_x_lut: Vec<u32>,
    source_y_lut: Vec<u32>,
    cursor_bitmap: Option<alloc::vec::Vec<u8>>,
    cursor_width: u32,
    cursor_height: u32,
    framebuffer: Option<Handle>,
    band_scratch: Vec<u8>,
    /// Shadow compositing row buffer (zero-copy — allocated once at startup).
    shadow_scratch: Vec<u8>,
    /// Temporary row buffer for horizontal blur (zero-copy — allocated once).
    blur_row_buf: Vec<u8>,
    /// Saved background pixels under the cursor for the dedicated cursor fast path.
    cursor_bg_saved: Vec<u8>,
    saved_cursor_rect: Option<DamageRect>,
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
}

#[derive(Clone, Copy)]
struct SourceFrame {
    width: u32,
    height: u32,
    stride: u32,
    pixels: &'static [u8],
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

#[derive(Clone)]
struct BackdropCacheEntry {
    y: u32,
    start_x: u32,
    width: u32,
    quality: GlassQuality,
    valid: bool,
    pixels: Vec<u8>,
}

impl BackdropCacheEntry {
    fn new() -> Self {
        Self {
            y: 0,
            start_x: 0,
            width: 0,
            quality: GlassQuality::High,
            valid: false,
            pixels: alloc::vec![0u8; BACKDROP_CACHE_MAX_WIDTH * 4],
        }
    }
}

#[derive(Clone)]
struct GlassLayerCache {
    key: u64,
    rect: DamageRect,
    width: u32,
    height: u32,
    valid: bool,
    pixels: Vec<u8>,
}

impl GlassLayerCache {
    fn new() -> Self {
        Self {
            key: 0,
            rect: DamageRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            width: 0,
            height: 0,
            valid: false,
            pixels: alloc::vec![0u8; GLASS_LAYER_MAX_BYTES],
        }
    }
}

#[derive(Clone)]
struct PathCacheEntry {
    id_hash: u64,
    width: u32,
    height: u32,
    color: [u8; 4],
    valid: bool,
    pixels: Vec<u8>,
}

impl PathCacheEntry {
    fn new() -> Self {
        Self {
            id_hash: 0,
            width: 0,
            height: 0,
            color: [0; 4],
            valid: false,
            pixels: alloc::vec![0u8; PATH_CACHE_MAX_PIXELS],
        }
    }
}

/// A retained render layer for a panel or UI element.
/// Holds pre-rendered pixel data so we can skip re-rendering when not dirty.
#[derive(Clone)]
struct Layer {
    id: u64,
    bounds: DamageRect,
    pixels: Vec<u8>,
    dirty: bool,
    rows_filled: u32,
    opacity: u8,
    backdrop_blur: Option<u32>,
}

impl Layer {
    fn new(id: u64, bounds: DamageRect, opacity: u8, backdrop_blur: Option<u32>) -> Self {
        let pixel_count = bounds.width as usize * bounds.height as usize * 4;
        Self {
            id,
            bounds,
            pixels: alloc::vec![0u8; pixel_count],
            dirty: true,
            rows_filled: 0,
            opacity,
            backdrop_blur,
        }
    }
}

/// Simple layer cache: retains pre-rendered pixel data per layer.
#[derive(Clone, Default)]
struct LayerCache {
    layers: Vec<Layer>,
}

impl LayerCache {
    fn clear(&mut self) {
        self.layers.clear();
    }
    fn len(&self) -> usize {
        self.layers.len()
    }
    fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    fn insert(&mut self, layer: Layer) {
        if let Some(existing) = self.layers.iter_mut().find(|l| l.id == layer.id) {
            *existing = layer;
            return;
        }
        self.layers.push(layer);
    }

    fn used_bytes(&self) -> usize {
        self.layers.iter().map(|layer| layer.pixels.len()).sum()
    }

    fn get(&self, id: u64) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    fn invalidate(&mut self, id: u64) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id == id) {
            layer.dirty = true;
            layer.rows_filled = 0;
        }
    }

    fn mark_clean(&mut self, id: u64) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id == id) {
            layer.dirty = false;
        }
    }
}

/// Tile-based damage map. Tracks which 64×64 tiles are dirty and need re-rendering.
#[derive(Clone)]
struct TileMap {
    dirty: [u64; TILE_DIRTY_WORDS],
}

impl TileMap {
    fn new() -> Self {
        Self {
            dirty: [0; TILE_DIRTY_WORDS],
        }
    }

    fn tile_index(x: u32, y: u32) -> usize {
        (y / TILE_SIZE) as usize * TILES_X + (x / TILE_SIZE) as usize
    }

    fn mark_rect(&mut self, rect: DamageRect) {
        let tx0 = rect.x / TILE_SIZE;
        let ty0 = rect.y / TILE_SIZE;
        let tx1 = (rect.end_x().saturating_sub(1) / TILE_SIZE).min(TILES_X as u32 - 1);
        let ty1 = (rect.end_y().saturating_sub(1) / TILE_SIZE).min(TILES_Y as u32 - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let idx = ty as usize * TILES_X + tx as usize;
                let word = idx / 64;
                let bit = idx % 64;
                self.dirty[word] |= 1u64 << bit;
            }
        }
    }

    fn is_dirty(&self, tx: usize, ty: usize) -> bool {
        let idx = ty * TILES_X + tx;
        let word = idx / 64;
        let bit = idx % 64;
        self.dirty[word] & (1u64 << bit) != 0
    }

    fn clear(&mut self) {
        for w in &mut self.dirty {
            *w = 0;
        }
    }

    fn has_dirty(&self) -> bool {
        self.dirty.iter().any(|w| *w != 0)
    }

    fn dirty_tiles(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..TILE_COUNT).filter_map(|idx| {
            let word = idx / 64;
            let bit = idx % 64;
            (self.dirty[word] & (1u64 << bit) != 0).then(|| (idx % TILES_X, idx / TILES_X))
        })
    }

    fn has_dirty_in_row_range(&self, start_y: u32, end_y: u32) -> bool {
        let ty0 = (start_y / TILE_SIZE) as usize;
        let ty1 = ((end_y.saturating_sub(1)) / TILE_SIZE).min(TILES_Y as u32 - 1) as usize;
        for ty in ty0..=ty1 {
            for tx in 0..TILES_X {
                let idx = ty * TILES_X + tx;
                let word = idx / 64;
                let bit = idx % 64;
                if self.dirty[word] & (1u64 << bit) != 0 {
                    return true;
                }
            }
        }
        false
    }
}

impl DisplayServerRuntime {
    fn new() -> Result<Self, WindowdError> {
        let mode = VisibleBootstrapMode::fixed()?.validate()?;
        let (source_width, source_height) = systemui::wallpaper_decoded_size();
        let source_frame = SourceFrame {
            width: source_width,
            height: source_height,
            stride: checked_stride(source_width)?,
            pixels: systemui::wallpaper_bgra(),
        };
        let source_x_lut = build_scale_lut(mode.width, source_width)?;
        let source_y_lut = build_scale_lut(mode.height, source_height)?;
        let cursor = crate::render_assets::render_cursor_surface(CallerCtx::system());
        let (cursor_bitmap, cursor_width, cursor_height) = match cursor {
            Some(cursor) => (Some(cursor.pixels), cursor.width, cursor.height),
            None => (None, 0, 0),
        };
        let initial_state = VisibleState {
            backend_visible: true,
            systemui_first_frame_visible: true,
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
        let proof_layouts = build_live_proof_layouts(initial_state);
        let proof_layout_index = proof_layouts
            .as_ref()
            .and_then(|layouts| layouts.first())
            .map(|layout| {
                LayoutHotPathIndex::build(
                    layout,
                    PROOF_PANEL_X,
                    PROOF_PANEL_Y,
                    mode.width,
                    mode.height,
                )
            });
        Ok(Self {
            mode,
            source_frame,
            source_x_lut,
            source_y_lut,
            cursor_bitmap,
            cursor_width,
            cursor_height,
            framebuffer: None,
            band_scratch: alloc::vec![0u8; mode.stride as usize * ROW_WRITE_CHUNK],
            shadow_scratch: alloc::vec![0u8; mode.stride as usize],
            blur_row_buf: alloc::vec![0u8; mode.stride as usize],
            cursor_bg_saved: alloc::vec![0u8; CURSOR_BG_MAX_BYTES],
            saved_cursor_rect: None,
            state: initial_state,
            observer_state: initial_state,
            markers_emitted: false,
            input_markers_emitted: InputMarkerState::default(),
            input_state_debug_emitted: false,
            pending_damage_rects: Vec::with_capacity(4),
            tile_map: TileMap::new(),
            layer_cache: LayerCache::default(),
            shadow_arena_buf: alloc::vec![0u8; WINDOWD_SHADOW_ARENA_SIZE],
            shadow_arena_used: 0,
            col_scratch: alloc::vec![0u8; COL_SCRATCH_SIZE],
            shadow_box_cache: [ShadowBoxCacheEntry::empty(); SHADOW_BOX_CACHE_ENTRIES],
            pending_damage_rect: None,
            paint_only_damage: false,
            proof_layouts,
            proof_layout_index,
            filtered_words,
            telemetry: crate::telemetry::WindowdDisplayTelemetry::default(),
            backdrop_cache: core::array::from_fn(|_| BackdropCacheEntry::new()),
            glass_layer: GlassLayerCache::new(),
            glass_scratch: alloc::vec![0u8; GLASS_LAYER_MAX_BYTES],
            path_cache: core::array::from_fn(|_| PathCacheEntry::new()),
            active_filter_idx: 0,
            filter_cycle: 0,
            clipping_marker_emitted: false,
            scroll_marker_emitted: false,
            live_scroll_marker_emitted: false,
            selftest_v3b_emitted: false,
        })
    }

    const fn visible_state(&self) -> VisibleState {
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
        self.observer_state.hover_visible |= self.state.hover_visible;
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

    fn register_framebuffer(&mut self, handle: Handle) -> u8 {
        self.framebuffer = Some(handle);
        self.state.display_scanout_ready = true;
        self.refresh_observer_state();
        if self.write_current_frame().is_err() {
            return STATUS_MALFORMED;
        }
        if self.active_proof_layout().is_some() {
            let _ = debug_println(LAYOUT_ENGINE_ON_MARKER);
            let _ = debug_println(TEXT_WRAPPING_ON_MARKER);
        }
        let _ = debug_println(DISPLAY_BOOTSTRAP_MARKER);
        let _ = debug_println(DISPLAY_MODE_MARKER);
        let _ = debug_println(VISIBLE_BACKEND_MARKER);
        let _ = debug_println(COMPOSE_READY_MARKER);
        let _ = debug_println(PRESENT_QUEUED_MARKER);
        let _ = debug_println(PRESENT_SCHEDULER_ON_MARKER);
        self.input_markers_emitted.scheduler = true;
        let _ = debug_println(SELFTEST_UI_V2_PRESENT_OK_MARKER);
        self.input_markers_emitted.v2_present = true;
        let _ = debug_println(DISPLAY_FIRST_SCANOUT_MARKER);
        let _ = debug_println(SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER);
        let _ = debug_println(PRESENT_VISIBLE_MARKER);
        let _ = debug_println(SELFTEST_UI_VISIBLE_PRESENT_MARKER);
        self.emit_asset_markers();
        self.emit_v3b_markers();
        STATUS_OK
    }

    fn apply_input_state(&mut self, upstream: VisibleState) -> u8 {
        if !self.input_state_debug_emitted {
            let _ = debug_println("dbg: windowd input state applied");
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
        self.state.hover_visible = upstream.hover_visible;
        self.state.focus_visible |= upstream.focus_visible;
        self.state.launcher_click_visible = upstream.launcher_click_visible;
        self.state.keyboard_visible |= upstream.keyboard_visible || upstream.keyboard_route_live;
        self.state.wheel_up_visible = upstream.wheel_up_visible;
        self.state.wheel_down_visible = upstream.wheel_down_visible;
        self.state.cursor_x = upstream.cursor_x;
        self.state.cursor_y = upstream.cursor_y;
        self.state.set_text_input(upstream.text_input());
        refill_filtered_words(&mut self.filtered_words, self.state.text_input());
        self.active_filter_idx = filter_layout_variant_index(self.state.text_input());
        if self.active_filter_idx != old_filter_idx {
            self.refresh_active_proof_hot_path();
        }
        self.refresh_observer_state();
        if self.state == old_state && self.active_filter_idx == old_filter_idx {
            return STATUS_OK;
        }
        self.queue_target_damage(old_state, self.state);
        // Detect paint-only: only hover/click/keyboard flags changed, not cursor or text
        let cursor_changed =
            old_cursor_x != self.state.cursor_x || old_cursor_y != self.state.cursor_y;
        let text_changed = old_state.text_input() != self.state.text_input();
        let filter_changed = old_filter_idx != self.active_filter_idx;
        let paint_flags_changed = old_state.hover_visible != self.state.hover_visible
            || old_state.launcher_click_visible != self.state.launcher_click_visible
            || old_state.keyboard_visible != self.state.keyboard_visible;
        let pointer_only_change =
            cursor_changed && !paint_flags_changed && !text_changed && !filter_changed;
        if pointer_only_change && self.saved_cursor_rect.is_some() {
            let old_cursor_rect = self.saved_cursor_rect;
            let new_cursor_rect = cursor_damage_rect(
                self.state.cursor_x,
                self.state.cursor_y,
                self.cursor_width,
                self.cursor_height,
                self.mode.width,
                self.mode.height,
            );
            let cursor_crosses_effect_region = old_cursor_rect
                .is_some_and(|rect| self.cursor_rect_intersects_effect_region(rect))
                || new_cursor_rect
                    .is_some_and(|rect| self.cursor_rect_intersects_effect_region(rect));
            if !cursor_crosses_effect_region && self.update_cursor_fast_path().is_ok() {
                self.emit_input_markers();
                return STATUS_OK;
            }
        }
        self.paint_only_damage =
            paint_flags_changed && !cursor_changed && !text_changed && !filter_changed;
        self.queue_cursor_damage(
            old_cursor_x,
            old_cursor_y,
            self.state.cursor_x,
            self.state.cursor_y,
        );

        // ── v3b: reflect real upstream text instead of synthetic keyboard cycling ──
        if old_state.text_input() != self.state.text_input() {
            self.note_filter_text_changed();
        }

        // ── v3b: scroll on wheel events ──
        if (upstream.wheel_up_visible || upstream.wheel_down_visible)
            && self.active_proof_layout().is_some()
        {
            self.handle_scroll_input();
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
            let container_id = layout
                .boxes
                .iter()
                .find(|b| b.id == Some("filter_list"))
                .map(|b| b.node_id);

            if let Some(id) = container_id {
                let viewport_h = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| {
                        FxPx::new(
                            filter_list_viewport_height(b.rect.height.as_u32().unwrap_or(0)) as i32,
                        )
                    })
                    .unwrap_or(FxPx::ZERO);
                let current_offset = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| b.scroll_offset)
                    .unwrap_or((FxPx::ZERO, FxPx::ZERO));

                let dy = if wheel_down_visible {
                    FxPx::new(20)
                } else {
                    FxPx::new(-20)
                };
                let max_scroll = FxPx::new((content_h as i32).saturating_sub(viewport_h.0).max(0));
                let new_offset_y = (current_offset.1 + dy).clamp(FxPx::ZERO, max_scroll);
                let new_offset = (current_offset.0, new_offset_y);
                scroll_damage = Some(layout.reposition_scroll(id, new_offset));
            }
        }
        if let Some(damage) = scroll_damage {
            self.refresh_active_proof_hot_path();
            for rect in damage.rects.into_iter().flatten() {
                let x = PROOF_PANEL_X.saturating_add(rect.x.as_u32().unwrap_or(0));
                let y = PROOF_PANEL_Y.saturating_add(rect.y.as_u32().unwrap_or(0));
                let w = rect.width.as_u32().unwrap_or(0);
                let h = rect.height.as_u32().unwrap_or(0);
                if w > 0 && h > 0 {
                    self.queue_dirty_rect(DamageRect {
                        x,
                        y,
                        width: w,
                        height: h,
                    });
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
                PROOF_PANEL_X,
                PROOF_PANEL_Y,
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

    fn tick(&mut self, now_ns: u64) {
        // The scanout VMO persists; avoid rewriting a full 1280x800 frame on idle ticks.
        if let Some(report) = self.telemetry.report_values_if_due(now_ns) {
            emit_windowd_telemetry(report);
        }
    }

    fn emit_asset_markers(&mut self) {
        if self.markers_emitted {
            return;
        }
        if self.state.cursor_svg_visible {
            let _ = debug_println(crate::markers::CURSOR_SVG_LOADED_MARKER);
        }
        if self.state.wallpaper_visible {
            let _ = debug_println(crate::markers::WALLPAPER_VISIBLE_MARKER);
        }
        if self.state.text_target_visible {
            let _ = debug_println(crate::markers::TEXT_TARGET_VISIBLE_MARKER);
        }
        if self.state.icon_target_visible {
            let _ = debug_println(crate::markers::ICON_TARGET_VISIBLE_MARKER);
        }
        self.markers_emitted = true;
    }

    fn emit_v3b_markers(&mut self) {
        let _ = debug_println(crate::markers::EFFECTS_ON_MARKER);
        let _ = debug_println(crate::markers::EFFECT_BLUR_OK_MARKER);
        let _ = debug_println(crate::markers::SELFTEST_UI_V3_EFFECT_OK_MARKER);
    }

    fn emit_input_markers(&mut self) {
        if self.state.input_visible_on && !self.input_markers_emitted.input {
            let _ = debug_println(INPUT_ON_MARKER);
            let _ = debug_println(INPUT_VISIBLE_ON_MARKER);
            self.input_markers_emitted.input = true;
        }
        if self.state.full_window_visible && !self.input_markers_emitted.full_window {
            let _ = debug_println(FULL_WINDOW_VISIBLE_MARKER);
            self.input_markers_emitted.full_window = true;
        }
        if self.state.cursor_move_visible && !self.input_markers_emitted.cursor {
            let _ = debug_println(CURSOR_MOVE_VISIBLE_MARKER);
            self.input_markers_emitted.cursor = true;
        }
        if self.state.hover_visible && !self.input_markers_emitted.hover {
            let _ = debug_println(HOVER_VISIBLE_MARKER);
            self.input_markers_emitted.hover = true;
        }
        if self.state.focus_visible && !self.input_markers_emitted.focus {
            if !self.input_markers_emitted.focus_route {
                let _ = debug_println("windowd: focus -> 1");
                self.input_markers_emitted.focus_route = true;
            }
            let _ = debug_println(FOCUS_VISIBLE_MARKER);
            self.input_markers_emitted.focus = true;
        }
        if self.state.launcher_click_visible && !self.input_markers_emitted.launcher_click {
            if !self.input_markers_emitted.launcher_click_route {
                let _ = debug_println(LAUNCHER_CLICK_OK_MARKER);
                self.input_markers_emitted.launcher_click_route = true;
            }
            let _ = debug_println(LAUNCHER_CLICK_VISIBLE_OK_MARKER);
            self.input_markers_emitted.launcher_click = true;
        }
        if self.state.input_visible_on
            && self.input_markers_emitted.launcher_click_route
            && !self.input_markers_emitted.v2_input
        {
            let _ = debug_println(SELFTEST_UI_V2_INPUT_OK_MARKER);
            self.input_markers_emitted.v2_input = true;
        }
        if self.state.keyboard_visible && !self.input_markers_emitted.keyboard {
            let _ = debug_println(KEYBOARD_VISIBLE_MARKER);
            self.input_markers_emitted.keyboard = true;
        }
        if (self.state.wheel_up_visible || self.state.wheel_down_visible)
            && !self.input_markers_emitted.wheel
        {
            let _ = debug_println(WHEEL_VISIBLE_MARKER);
            self.input_markers_emitted.wheel = true;
        }
        if self.input_markers_emitted.input
            && self.input_markers_emitted.full_window
            && self.input_markers_emitted.cursor
            && self.input_markers_emitted.hover
            && self.input_markers_emitted.focus
            && self.input_markers_emitted.launcher_click
            && self.input_markers_emitted.keyboard
            && !self.input_markers_emitted.visible_input_summary
        {
            let _ = debug_println(SELFTEST_UI_VISIBLE_INPUT_OK_MARKER);
            self.input_markers_emitted.visible_input_summary = true;
        }
        if self.input_markers_emitted.visible_input_summary
            && self.input_markers_emitted.wheel
            && !self.input_markers_emitted.visible_wheel_summary
        {
            let _ = debug_println(SELFTEST_UI_VISIBLE_WHEEL_OK_MARKER);
            self.input_markers_emitted.visible_wheel_summary = true;
        }
        if self.input_markers_emitted.visible_wheel_summary
            && self.markers_emitted
            && !self.input_markers_emitted.v2b_assets_summary
        {
            let _ = debug_println(crate::markers::SELFTEST_UI_V2B_ASSETS_OK_MARKER);
            self.input_markers_emitted.v2b_assets_summary = true;
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
        self.write_rows(
            0,
            self.mode.height,
            select_glass_quality(PROOF_PANEL_H),
            false,
        )?;
        self.write_cursor_overlay()
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
        let proof_layout = self
            .proof_layouts
            .as_ref()
            .and_then(|layouts| layouts.get(active_filter_idx));
        let proof_layout_index = self.proof_layout_index.as_ref();
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let state = self.state;
        let filter_text = state.text_input();
        let filtered_words = self.filtered_words.as_slice();
        let cursor_bitmap = self.cursor_bitmap.as_deref();
        let cursor_width = self.cursor_width;
        let cursor_height = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
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
                    cursor_bitmap,
                    cursor_width,
                    cursor_height,
                    cursor_x,
                    cursor_y,
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
            }
            let offset = band_start as usize * row_len;
            vmo_write(handle, offset, &band_scratch[..band_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        self.shadow_arena_used = shadow_arena.used_bytes();
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.telemetry.record_compose_timed(
            u64::from(self.mode.width).saturating_mul(u64::from(end_y.saturating_sub(start_y))),
            nsec()
                .unwrap_or(render_start_ns)
                .saturating_sub(render_start_ns),
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
        let proof_layout = self
            .proof_layouts
            .as_ref()
            .and_then(|layouts| layouts.get(active_filter_idx));
        let proof_layout_index = self.proof_layout_index.as_ref();
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let state = self.state;
        let filter_text = state.text_input();
        let filtered_words = self.filtered_words.as_slice();
        let cursor_bitmap = self.cursor_bitmap.as_deref();
        let cursor_width = self.cursor_width;
        let cursor_height = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
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
            // Render band rows into band_scratch
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
                    cursor_bitmap,
                    cursor_width,
                    cursor_height,
                    cursor_x,
                    cursor_y,
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
            }
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let offset = y as usize * row_len + byte_start;
                let src_offset = row_idx * row_len + byte_start;
                if byte_start == 0 && byte_end == row_len {
                    let band_bytes = (band_end - band_start) as usize * row_len;
                    vmo_write(
                        handle,
                        band_start as usize * row_len,
                        &self.band_scratch[..band_bytes],
                    )
                    .map_err(|_| WindowdError::BufferLengthMismatch)?;
                    break;
                }
                vmo_write(
                    handle,
                    offset,
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
            nsec()
                .unwrap_or(render_start_ns)
                .saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        self.refresh_observer_state();
        Ok(())
    }

    fn queue_cursor_damage(
        &mut self,
        old_cursor_x: i32,
        old_cursor_y: i32,
        new_cursor_x: i32,
        new_cursor_y: i32,
    ) {
        let old_rect = cursor_damage_rect(
            old_cursor_x,
            old_cursor_y,
            self.cursor_width,
            self.cursor_height,
            self.mode.width,
            self.mode.height,
        );
        let new_rect = cursor_damage_rect(
            new_cursor_x,
            new_cursor_y,
            self.cursor_width,
            self.cursor_height,
            self.mode.width,
            self.mode.height,
        );
        match (old_rect, new_rect) {
            (Some(old_rect), Some(new_rect)) => self.queue_dirty_rect(old_rect.merge(new_rect)),
            (Some(rect), None) | (None, Some(rect)) => self.queue_dirty_rect(rect),
            (None, None) => {}
        }
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

    fn flush_pending_damage(&mut self) -> Result<(), WindowdError> {
        let paint_only = self.paint_only_damage;
        let mut wrote_any = false;
        let mut rects = [DamageRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }; 5];
        let mut rect_count = 0usize;

        if let Some(rect) = self.pending_damage_rect.take() {
            rects[rect_count] = rect;
            rect_count += 1;
        }
        while let Some(rect) = self.pending_damage_rects.pop() {
            if rect_count < rects.len() {
                rects[rect_count] = rect;
                rect_count += 1;
            }
        }
        rect_count = premerge_damage_rects(&mut rects, rect_count);
        for rect in rects.iter().copied().take(rect_count) {
            self.write_damage_rect(rect, GlassQuality::High, paint_only)?;
            wrote_any = true;
        }

        self.tile_map.clear();
        if wrote_any {
            let _ = self.write_cursor_overlay();
            self.emit_input_markers();
            self.paint_only_damage = false;
        }
        Ok(())
    }

    fn update_cursor_fast_path(&mut self) -> Result<(), WindowdError> {
        self.restore_cursor_bg()?;
        self.write_cursor_overlay()?;
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.refresh_observer_state();
        Ok(())
    }

    fn cursor_rect_intersects_effect_region(&self, rect: DamageRect) -> bool {
        let Some(layout) = self.active_proof_layout() else {
            return false;
        };
        layout.boxes.iter().any(|layout_box| {
            if layout_box.id != Some("combined_panels") {
                return false;
            }
            let Some(panel_rect) = proof_box_rect(layout_box) else {
                return false;
            };
            damage_rects_intersect(rect, inflate_effect_rect(panel_rect, self.mode))
        })
    }

    fn restore_cursor_bg(&mut self) -> Result<(), WindowdError> {
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(rect) = self.saved_cursor_rect.take() else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        let byte_len = rect.width as usize * 4;
        for (row_idx, y) in (rect.y..rect.end_y().min(self.mode.height)).enumerate() {
            let src_offset = row_idx.saturating_mul(byte_len);
            let src_end = src_offset.saturating_add(byte_len);
            if src_end > self.cursor_bg_saved.len() {
                continue;
            }
            let dst_offset = y as usize * row_len + rect.x as usize * 4;
            vmo_write(
                handle,
                dst_offset,
                &self.cursor_bg_saved[src_offset..src_end],
            )
            .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        self.telemetry.record_present();
        Ok(())
    }

    fn write_cursor_overlay(&mut self) -> Result<(), WindowdError> {
        let render_start_ns = nsec().unwrap_or(0);
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let Some(cursor_bitmap) = self.cursor_bitmap.as_deref() else {
            self.saved_cursor_rect = None;
            return Ok(());
        };
        let Some(rect) = cursor_damage_rect(
            self.state.cursor_x,
            self.state.cursor_y,
            self.cursor_width,
            self.cursor_height,
            self.mode.width,
            self.mode.height,
        ) else {
            self.saved_cursor_rect = None;
            return Ok(());
        };
        let cursor_over_effect = self.cursor_rect_intersects_effect_region(rect);
        if cursor_over_effect {
            // Keep the logical previous cursor rect for the next damage merge.
            // No background bytes are saved for effect regions, so this rect must
            // not be restored through the cursor fast path; `apply_input_state`
            // skips that path whenever old or new cursor bounds cross glass.
            self.saved_cursor_rect = Some(rect);
            return Ok(());
        }

        let row_len = self.mode.stride as usize;
        if self.band_scratch.len() < row_len {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let active_filter_idx = self.active_filter_idx;
        let proof_layout = self
            .proof_layouts
            .as_ref()
            .and_then(|layouts| layouts.get(active_filter_idx));
        let proof_layout_index = self.proof_layout_index.as_ref();
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let state = self.state;
        let filter_text = state.text_input();
        let filtered_words = self.filtered_words.as_slice();
        let cursor_left = self.state.cursor_x - crate::assets::CURSOR_HOTSPOT_X;
        let cursor_top = self.state.cursor_y - crate::assets::CURSOR_HOTSPOT_Y;
        let blur_row_buf = &mut self.blur_row_buf[..row_len];
        let shadow_scratch = &mut self.shadow_scratch[..row_len];
        let backdrop_cache = &mut self.backdrop_cache;
        let glass_layer = &mut self.glass_layer;
        let glass_scratch = &mut self.glass_scratch;
        let path_cache = &mut self.path_cache;
        let mut shadow_arena =
            ShadowArena::from_buffer_with_used(&mut self.shadow_arena_buf, self.shadow_arena_used);

        let byte_start = rect.x as usize * 4;
        let byte_len = rect.width as usize * 4;
        let render_clip = RenderClip::new(rect.x, rect.end_x(), self.mode.width);
        for y in rect.y..rect.end_y().min(self.mode.height) {
            let band_row = &mut self.band_scratch[..row_len];
            copy_cursor_background_row(
                blur_row_buf,
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
                band_row,
                &mut self.layer_cache,
                shadow_scratch,
                &mut shadow_arena,
                &mut self.col_scratch,
                &mut self.shadow_box_cache,
            )?;
            let bg_offset = (y - rect.y) as usize * byte_len;
            let bg_end = bg_offset.saturating_add(byte_len);
            if !cursor_over_effect && bg_end <= self.cursor_bg_saved.len() {
                self.cursor_bg_saved[bg_offset..bg_end]
                    .copy_from_slice(&band_row[byte_start..byte_start + byte_len]);
            }

            blend_cursor_row(
                band_row,
                y,
                cursor_bitmap,
                self.cursor_width,
                self.cursor_height,
                cursor_left,
                cursor_top,
            );
            let offset = y as usize * row_len + byte_start;
            // Cursor rect rows are separated by framebuffer stride; a packed
            // rect write would corrupt the gap between rows.
            vmo_write(handle, offset, &band_row[byte_start..byte_start + byte_len])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        self.shadow_arena_used = shadow_arena.used_bytes();
        self.saved_cursor_rect = Some(rect);
        self.telemetry.record_compose_timed(
            u64::from(rect.width).saturating_mul(u64::from(rect.height)),
            nsec()
                .unwrap_or(render_start_ns)
                .saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        Ok(())
    }

    fn has_pending_damage(&self) -> bool {
        !self.pending_damage_rects.is_empty() || self.pending_damage_rect.is_some()
    }
}

fn cursor_damage_rect(
    cursor_x: i32,
    cursor_y: i32,
    cursor_width: u32,
    cursor_height: u32,
    mode_width: u32,
    mode_height: u32,
) -> Option<DamageRect> {
    if cursor_width == 0 || cursor_height == 0 || mode_width == 0 || mode_height == 0 {
        return None;
    }
    let x0 = cursor_x.saturating_sub(crate::assets::CURSOR_HOTSPOT_X);
    let y0 = cursor_y.saturating_sub(crate::assets::CURSOR_HOTSPOT_Y);
    let x1 = x0.saturating_add(cursor_width as i32);
    let y1 = y0.saturating_add(cursor_height as i32);
    let start_x = x0.max(0).min(mode_width as i32) as u32;
    let start_y = y0.max(0).min(mode_height as i32) as u32;
    let end_x = x1.max(0).min(mode_width as i32) as u32;
    let end_y = y1.max(0).min(mode_height as i32) as u32;
    if end_x <= start_x || end_y <= start_y {
        return None;
    }
    Some(DamageRect {
        x: start_x,
        y: start_y,
        width: end_x - start_x,
        height: end_y - start_y,
    })
}

fn inflate_effect_rect(rect: ProofBoxRect, mode: VisibleBootstrapMode) -> DamageRect {
    let pad = SOFT_PANEL_SHADOW_BLUR_RADIUS
        .saturating_add(SOFT_PANEL_SHADOW_OFFSET_Y.unsigned_abs())
        .saturating_add(2);
    let x = rect.x.saturating_sub(pad);
    let y = rect.y.saturating_sub(pad);
    let end_x = rect
        .x
        .saturating_add(rect.width)
        .saturating_add(pad)
        .min(mode.width);
    let end_y = rect
        .y
        .saturating_add(rect.height)
        .saturating_add(pad)
        .min(mode.height);
    DamageRect {
        x,
        y,
        width: end_x.saturating_sub(x),
        height: end_y.saturating_sub(y),
    }
}

fn damage_rects_intersect(a: DamageRect, b: DamageRect) -> bool {
    a.x < b.end_x() && b.x < a.end_x() && a.y < b.end_y() && b.y < a.end_y()
}

fn flush_error_label(err: WindowdError) -> &'static str {
    match err {
        WindowdError::BufferLengthMismatch => "windowd: flush rows fail buffer-len",
        WindowdError::ArithmeticOverflow => "windowd: flush rows fail arith",
        _ => "windowd: flush rows fail",
    }
}

fn filter_layout_variant_index(filter_text: &str) -> usize {
    let mut best_idx = 0;
    let mut best_len = 0;
    for (idx, candidate) in LIVE_FILTER_VARIANTS.iter().enumerate() {
        if filter_text.starts_with(candidate) && candidate.len() >= best_len {
            best_idx = idx;
            best_len = candidate.len();
        }
    }
    best_idx
}

fn build_live_proof_layouts(state: VisibleState) -> Option<Vec<LayoutResult>> {
    let mut layouts = Vec::with_capacity(LIVE_FILTER_VARIANTS.len());
    for filter_text in LIVE_FILTER_VARIANTS {
        layouts.push(layout_panel::compute_proof_layout(state, filter_text).ok()?);
    }
    Some(layouts)
}

fn copy_scene_row(
    blur_row_buf: &mut [u8],
    shadow_scratch: &mut [u8],
    backdrop_cache: &mut [BackdropCacheEntry],
    glass_layer: &mut GlassLayerCache,
    glass_scratch: &mut [u8],
    path_cache: &mut [PathCacheEntry],
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    proof_layout_index: Option<&LayoutHotPathIndex>,
    filter_text: &str,
    filtered_words: &[&'static str],
    _cursor_bitmap: Option<&[u8]>,
    _cursor_width: u32,
    _cursor_height: u32,
    _cursor_x: i32,
    _cursor_y: i32,
    y: u32,
    render_clip: RenderClip,
    glass_quality: GlassQuality,
    paint_only: bool,
    row: &mut [u8],
    layer_cache: &mut LayerCache,
    shadow_arena: &mut ShadowArena<'_>,
    col_scratch: &mut [u8],
    shadow_box_cache: &mut [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row_clipped(
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        y,
        row,
        render_clip,
    )?;
    if !paint_only {
        compute_shadow_row(
            state,
            proof_layout,
            proof_layout_index,
            y,
            row,
            shadow_scratch,
            blur_row_buf,
            shadow_arena,
            col_scratch,
            shadow_box_cache,
        )?;
    }
    draw_proof_surface_row(
        state,
        proof_layout,
        proof_layout_index,
        filter_text,
        filtered_words,
        y,
        row,
        render_clip,
        backdrop_cache,
        glass_layer,
        glass_scratch,
        path_cache,
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        glass_quality,
        blur_row_buf,
        layer_cache,
        paint_only,
    )?;
    Ok(())
}

fn copy_cursor_background_row(
    blur_row_buf: &mut [u8],
    backdrop_cache: &mut [BackdropCacheEntry],
    glass_layer: &mut GlassLayerCache,
    glass_scratch: &mut [u8],
    path_cache: &mut [PathCacheEntry],
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    proof_layout_index: Option<&LayoutHotPathIndex>,
    filter_text: &str,
    filtered_words: &[&'static str],
    y: u32,
    render_clip: RenderClip,
    row: &mut [u8],
    layer_cache: &mut LayerCache,
    shadow_scratch: &mut [u8],
    shadow_arena: &mut ShadowArena<'_>,
    col_scratch: &mut [u8],
    shadow_box_cache: &mut [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES],
) -> Result<(), WindowdError> {
    copy_scaled_systemui_row_clipped(
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        y,
        row,
        render_clip,
    )?;
    compute_shadow_row(
        state,
        proof_layout,
        proof_layout_index,
        y,
        row,
        shadow_scratch,
        blur_row_buf,
        shadow_arena,
        col_scratch,
        shadow_box_cache,
    )?;
    draw_proof_surface_row(
        state,
        proof_layout,
        proof_layout_index,
        filter_text,
        filtered_words,
        y,
        row,
        render_clip,
        backdrop_cache,
        glass_layer,
        glass_scratch,
        path_cache,
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        GlassQuality::High,
        blur_row_buf,
        layer_cache,
        true,
    )
}

/// Zero-copy shadow compositing pass — per-box caching with ShadowArena.
///
/// Phase 1 (once per box, if cache miss): render full box alpha into arena,
/// apply 2D separable blur (zero-alloc), store offset in cache.
/// Phase 2 (per row): blit from arena, tint, composite onto target.
///
/// Falls back to per-row inline rendering if the arena is full.
/// Zero heap allocations in the hot path — all buffers pre-allocated at startup.
fn shadow_cache_key(
    box_id_hash: u64,
    width: u32,
    height: u32,
    blur_radius: u32,
    spread: i32,
    color: Rgba8,
) -> u64 {
    let mut key = box_id_hash;
    key ^= (width as u64).rotate_left(7);
    key ^= (height as u64).rotate_left(17);
    key ^= (blur_radius as u64).rotate_left(29);
    key ^= (spread as u32 as u64).rotate_left(37);
    key ^= (color.r as u64).rotate_left(3);
    key ^= (color.g as u64).rotate_left(11);
    key ^= (color.b as u64).rotate_left(19);
    key ^ (color.a as u64).rotate_left(47)
}

fn shadow_cache_scale(width: u32, height: u32, budget_bytes: usize) -> Option<u8> {
    if width == 0 || height == 0 {
        return None;
    }
    for scale in 1..=SHADOW_CACHE_MAX_DOWNSCALE {
        let scale_u32 = scale as u32;
        let cache_w = width.div_ceil(scale_u32).max(1);
        let cache_h = height.div_ceil(scale_u32).max(1);
        let bytes = cache_w as usize * cache_h as usize * 4;
        if bytes <= budget_bytes {
            return Some(scale);
        }
    }
    None
}

fn composite_shadow_layer_row(
    target: &mut [u8],
    layer_row: &[u8],
    cached_width: u32,
    logical_width: u32,
    scale: u8,
    shadow_x: i32,
    color: Rgba8,
) {
    let row_pixels = target.len() / 4;
    let scale = scale.max(1) as usize;
    let segment_start = shadow_x.max(0).min(row_pixels as i32) as usize;
    let segment_end = shadow_x
        .saturating_add(logical_width as i32)
        .max(0)
        .min(row_pixels as i32) as usize;
    let sr = color.r as u32;
    let sg = color.g as u32;
    let sb = color.b as u32;
    let shadow_alpha = color.a as u32;
    for px in segment_start..segment_end {
        let source_px = (px as i32).saturating_sub(shadow_x) as usize / scale;
        if source_px >= cached_width as usize {
            continue;
        }
        let ci = source_px.saturating_mul(4);
        if ci + 4 > layer_row.len() {
            continue;
        }
        let sa = layer_row[ci + 3] as u32;
        if sa == 0 {
            continue;
        }
        let tinted_a = (sa * shadow_alpha) / 255;
        if tinted_a == 0 {
            continue;
        }
        let inv = 255 - tinted_a;
        let idx = px * 4;
        target[idx] = ((sr * tinted_a + target[idx] as u32 * inv) / 255) as u8;
        target[idx + 1] = ((sg * tinted_a + target[idx + 1] as u32 * inv) / 255) as u8;
        target[idx + 2] = ((sb * tinted_a + target[idx + 2] as u32 * inv) / 255) as u8;
    }
}

fn draw_soft_panel_shadow_row(y: u32, target: &mut [u8], rect: ProofBoxRect) {
    let row_pixels = target.len() / 4;
    let blur = SOFT_PANEL_SHADOW_BLUR_RADIUS as i32;
    let shadow_y = rect.y as i32 + SOFT_PANEL_SHADOW_OFFSET_Y;
    let start_x = (rect.x as i32)
        .saturating_sub(blur)
        .max(0)
        .min(row_pixels as i32) as usize;
    let end_x = (rect.x as i32)
        .saturating_add(rect.width as i32)
        .saturating_add(blur)
        .max(0)
        .min(row_pixels as i32) as usize;
    let row_y = y as i32;
    if row_y < shadow_y.saturating_sub(blur)
        || row_y
            > shadow_y
                .saturating_add(rect.height as i32)
                .saturating_add(blur)
    {
        return;
    }
    let min_x = fixed_sdf::px_u32(rect.x);
    let min_y = fixed_sdf::px_i32(shadow_y);
    let max_x = fixed_sdf::px_u32(rect.x.saturating_add(rect.width));
    let max_y = fixed_sdf::px_i32(shadow_y.saturating_add(rect.height as i32));
    let radius = fixed_sdf::px_u32(DARK_GLASS_RADIUS);
    let point_y = fixed_sdf::pixel_center(y);
    for px in start_x..end_x {
        let sd = fixed_sdf::rounded_rect_sd(
            fixed_sdf::pixel_center(px as u32),
            point_y,
            min_x,
            min_y,
            max_x,
            max_y,
            radius,
        );
        let alpha = fixed_sdf::shadow_alpha_from_distance(
            sd.max(0),
            SOFT_PANEL_SHADOW_BLUR_RADIUS,
            SOFT_PANEL_SHADOW_ALPHA,
        );
        if alpha == 0 {
            continue;
        }
        let idx = px * 4;
        let inv = 255u32.saturating_sub(alpha);
        target[idx] = (target[idx] as u32 * inv / 255) as u8;
        target[idx + 1] = (target[idx + 1] as u32 * inv / 255) as u8;
        target[idx + 2] = (target[idx + 2] as u32 * inv / 255) as u8;
    }
}

fn compute_shadow_row(
    _state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    proof_layout_index: Option<&LayoutHotPathIndex>,
    y: u32,
    target: &mut [u8],
    shadow_scratch: &mut [u8],
    blur_row_buf: &mut [u8],
    shadow_arena: &mut ShadowArena<'_>,
    col_scratch: &mut [u8],
    shadow_box_cache: &mut [ShadowBoxCacheEntry; SHADOW_BOX_CACHE_ENTRIES],
) -> Result<(), WindowdError> {
    let Some(layout) = proof_layout else {
        return Ok(());
    };
    let row_pixels = target.len() / 4;
    if shadow_scratch.len() < target.len() || blur_row_buf.len() < target.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }

    let row_mask = proof_layout_index.and_then(|index| {
        if index.overflow_boxes() {
            return None;
        }
        let mask = index.row_mask(y);
        (mask != 0).then_some(mask)
    });

    let mut draw_shadow = |layout_box: &nexus_layout::LayoutBox| {
        if layout_box.id == Some("combined_panels") {
            if let Some(rect) = proof_box_rect(layout_box) {
                draw_soft_panel_shadow_row(y, target, rect);
            }
            return;
        }
        let shadow = match &layout_box.visual.shadow {
            Some(shadow) => shadow,
            None => return,
        };
        let Some(rect) = proof_box_rect(layout_box) else {
            return;
        };

        let blur_r = shadow.blur_radius.0.max(0) as u32;
        let blur_i32 = blur_r as i32;
        let sx = (rect.x as i32)
            .saturating_add(shadow.offset_x.0)
            .saturating_sub(shadow.spread.0);
        let sy = (rect.y as i32)
            .saturating_add(shadow.offset_y.0)
            .saturating_sub(shadow.spread.0);
        let sw = (rect.width as i32).saturating_add(2 * shadow.spread.0);
        let sh = (rect.height as i32).saturating_add(2 * shadow.spread.0);

        if sw <= 0 || sh <= 0 {
            return;
        }

        let shadow_w = (sw + 2 * blur_i32).max(0) as u32;
        let shadow_h = (sh + 2 * blur_i32).max(0) as u32;

        // Per-box cache key
        let box_id_hash = layout_box
            .id
            .map(|id| {
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for b in id.as_bytes() {
                    h ^= u64::from(*b);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
                h
            })
            .unwrap_or(0);
        let cache_key = shadow_cache_key(
            box_id_hash,
            shadow_w,
            shadow_h,
            blur_r,
            shadow.spread.0,
            shadow.color,
        );

        // Check per-box cache
        for entry in shadow_box_cache.iter() {
            if entry.valid && entry.key == cache_key {
                // Cache hit: blit this row from the cached full-box shadow
                let shadow_sy = (rect.y as i32)
                    .saturating_add(shadow.offset_y.0)
                    .saturating_sub(shadow.spread.0)
                    .saturating_sub(blur_i32);
                let rel_y = if (y as i32) < shadow_sy {
                    0
                } else {
                    ((y as i32).saturating_sub(shadow_sy)) as u32
                };
                if rel_y < entry.height {
                    let cached_y = (rel_y / u32::from(entry.scale.max(1)))
                        .min(entry.cache_height.saturating_sub(1));
                    let src_start =
                        entry.arena_offset + cached_y as usize * entry.cache_width as usize * 4;
                    if let Some(cached_row) =
                        shadow_arena.get(src_start, entry.cache_width as usize * 4)
                    {
                        composite_shadow_layer_row(
                            target,
                            cached_row,
                            entry.cache_width,
                            entry.width,
                            entry.scale,
                            sx.saturating_sub(blur_i32),
                            shadow.color,
                        );
                    }
                }
                return;
            }
        }

        // Cache miss: keep the layer inside the fixed arena by downscaling if needed.
        let remaining_arena = shadow_arena
            .capacity()
            .saturating_sub(shadow_arena.used_bytes());
        let scratch_budget = remaining_arena.min(col_scratch.len());
        let Some(cache_scale) = shadow_cache_scale(shadow_w, shadow_h, scratch_budget) else {
            return draw_shadow_row_fallback(
                y,
                target,
                shadow_scratch,
                blur_row_buf,
                row_pixels,
                sx,
                sy,
                sw,
                sh,
                blur_i32,
                blur_r,
                shadow.color,
            );
        };
        let cache_scale_u32 = u32::from(cache_scale);
        let cache_w = shadow_w.div_ceil(cache_scale_u32).max(1);
        let cache_h = shadow_h.div_ceil(cache_scale_u32).max(1);
        let arena_bytes = cache_w as usize * cache_h as usize * 4;

        // Try to allocate arena slot for the compact box shadow.
        if let Some((arena_off, arena_slot)) = shadow_arena.alloc(arena_bytes) {
            // Render compact alpha into arena, then blur in compact space.
            let stride = (cache_w * 4) as u32;
            for row in 0..cache_h {
                let row_off = row as usize * stride as usize;
                let logical_y = row as i32 * i32::from(cache_scale);
                let abs_y = sy + logical_y - blur_i32;
                let dy = if abs_y < sy {
                    sy.saturating_sub(abs_y)
                } else if abs_y >= sy.saturating_add(sh) {
                    abs_y.saturating_sub(sy.saturating_add(sh).saturating_sub(1))
                } else {
                    0
                };
                let vertical_alpha = if blur_r == 0 {
                    255u32
                } else if dy > blur_i32 {
                    0u32
                } else {
                    let remaining = blur_i32.saturating_add(1).saturating_sub(dy) as u32;
                    (remaining * 255) / (blur_r + 1)
                };
                for px in 0..cache_w {
                    let logical_x = px as i32 * i32::from(cache_scale);
                    let core_x = logical_x - blur_i32;
                    let in_core = core_x >= 0 && core_x < sw;
                    let a = if in_core && vertical_alpha > 0 {
                        vertical_alpha as u8
                    } else {
                        0
                    };
                    let idx = row_off + px as usize * 4;
                    arena_slot[idx] = 0;
                    arena_slot[idx + 1] = 0;
                    arena_slot[idx + 2] = 0;
                    arena_slot[idx + 3] = a;
                }
            }

            // Apply 2D separable blur (zero-alloc)
            if blur_r > 0 {
                blur_separable_zero_alloc(
                    arena_slot,
                    cache_w,
                    cache_h,
                    stride,
                    blur_r.div_ceil(cache_scale_u32).max(1),
                    blur_row_buf,
                    col_scratch,
                );
            }

            // Store in the fixed cache, replacing deterministically on collision.
            let slot = (cache_key as usize) % shadow_box_cache.len();
            shadow_box_cache[slot] = ShadowBoxCacheEntry {
                key: cache_key,
                arena_offset: arena_off,
                width: shadow_w,
                height: shadow_h,
                cache_width: cache_w,
                cache_height: cache_h,
                scale: cache_scale,
                valid: true,
            };

            let shadow_sy = sy.saturating_sub(blur_i32);
            let rel_y = (y as i32).saturating_sub(shadow_sy);
            if rel_y >= 0 && (rel_y as u32) < shadow_h {
                let cache_y = (rel_y as u32 / cache_scale_u32).min(cache_h.saturating_sub(1));
                let row_start = cache_y as usize * cache_w as usize * 4;
                let row_end = row_start + cache_w as usize * 4;
                if row_end <= arena_slot.len() {
                    composite_shadow_layer_row(
                        target,
                        &arena_slot[row_start..row_end],
                        cache_w,
                        shadow_w,
                        cache_scale,
                        sx.saturating_sub(blur_i32),
                        shadow.color,
                    );
                }
            }
            return;
        }
        // If the fixed arena is full, fall through to deterministic per-row
        // degraded rendering. Never allocate to recover from arena exhaustion.

        // Per-row inline rendering (fallback or immediate composite after cache miss)
        let y_i32 = y as i32;
        let dy = if y_i32 < sy {
            sy.saturating_sub(y_i32)
        } else if y_i32 >= sy.saturating_add(sh) {
            y_i32.saturating_sub(sy.saturating_add(sh).saturating_sub(1))
        } else {
            0
        };
        if dy > blur_i32 {
            return;
        }

        let vertical_alpha = if blur_r == 0 {
            255u32
        } else {
            let remaining = blur_i32.saturating_add(1).saturating_sub(dy) as u32;
            (remaining * 255) / (blur_r + 1)
        };
        if vertical_alpha == 0 {
            return;
        }

        let segment_start = sx.saturating_sub(blur_i32).max(0).min(row_pixels as i32) as usize;
        let segment_end = sx
            .saturating_add(sw)
            .saturating_add(blur_i32)
            .max(0)
            .min(row_pixels as i32) as usize;
        if segment_start >= segment_end {
            return;
        }
        let segment_start_byte = segment_start * 4;
        let segment_end_byte = segment_end * 4;
        shadow_scratch[segment_start_byte..segment_end_byte].fill(0);

        let core_start = sx.max(0).min(row_pixels as i32) as usize;
        let core_end = sx.saturating_add(sw).max(0).min(row_pixels as i32) as usize;
        for px in core_start..core_end {
            shadow_scratch[px * 4 + 3] = vertical_alpha as u8;
        }

        let segment_len = segment_end_byte.saturating_sub(segment_start_byte);
        if blur_r > 0 && segment_len != 0 {
            blur_row_horizontal(
                &mut shadow_scratch[segment_start_byte..segment_end_byte],
                segment_len,
                blur_r,
                blur_row_buf,
            );
        }

        let sr = shadow.color.r as u32;
        let sg = shadow.color.g as u32;
        let sb = shadow.color.b as u32;
        let shadow_alpha = shadow.color.a as u32;
        for px in segment_start..segment_end {
            let idx = px * 4;
            let sa = shadow_scratch[idx + 3] as u32;
            if sa == 0 {
                continue;
            }
            let tinted_a = (sa * shadow_alpha) / 255;
            if tinted_a == 0 {
                continue;
            }
            let inv = 255 - tinted_a;
            target[idx] = ((sr * tinted_a + target[idx] as u32 * inv) / 255) as u8;
            target[idx + 1] = ((sg * tinted_a + target[idx + 1] as u32 * inv) / 255) as u8;
            target[idx + 2] = ((sb * tinted_a + target[idx + 2] as u32 * inv) / 255) as u8;
        }
    };

    match row_mask {
        Some(mut mask) => {
            while mask != 0 {
                let box_idx = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                draw_shadow(&layout.boxes[box_idx]);
            }
        }
        None => {
            for layout_box in &layout.boxes {
                draw_shadow(layout_box);
            }
        }
    }

    Ok(())
}

fn draw_shadow_row_fallback(
    y: u32,
    target: &mut [u8],
    shadow_scratch: &mut [u8],
    blur_row_buf: &mut [u8],
    row_pixels: usize,
    sx: i32,
    sy: i32,
    sw: i32,
    sh: i32,
    blur_i32: i32,
    blur_r: u32,
    color: Rgba8,
) {
    let y_i32 = y as i32;
    let dy = if y_i32 < sy {
        sy.saturating_sub(y_i32)
    } else if y_i32 >= sy.saturating_add(sh) {
        y_i32.saturating_sub(sy.saturating_add(sh).saturating_sub(1))
    } else {
        0
    };
    if dy > blur_i32 {
        return;
    }

    let vertical_alpha = if blur_r == 0 {
        255u32
    } else {
        let remaining = blur_i32.saturating_add(1).saturating_sub(dy) as u32;
        (remaining * 255) / (blur_r + 1)
    };
    if vertical_alpha == 0 {
        return;
    }

    let segment_start = sx.saturating_sub(blur_i32).max(0).min(row_pixels as i32) as usize;
    let segment_end = sx
        .saturating_add(sw)
        .saturating_add(blur_i32)
        .max(0)
        .min(row_pixels as i32) as usize;
    if segment_start >= segment_end {
        return;
    }
    let segment_start_byte = segment_start * 4;
    let segment_end_byte = segment_end * 4;
    shadow_scratch[segment_start_byte..segment_end_byte].fill(0);

    let core_start = sx.max(0).min(row_pixels as i32) as usize;
    let core_end = sx.saturating_add(sw).max(0).min(row_pixels as i32) as usize;
    for px in core_start..core_end {
        shadow_scratch[px * 4 + 3] = vertical_alpha as u8;
    }

    let segment_len = segment_end_byte.saturating_sub(segment_start_byte);
    if blur_r > 0 && segment_len != 0 {
        blur_row_horizontal(
            &mut shadow_scratch[segment_start_byte..segment_end_byte],
            segment_len,
            blur_r,
            blur_row_buf,
        );
    }

    let sr = color.r as u32;
    let sg = color.g as u32;
    let sb = color.b as u32;
    let shadow_alpha = color.a as u32;
    for px in segment_start..segment_end {
        let idx = px * 4;
        let sa = shadow_scratch[idx + 3] as u32;
        if sa == 0 {
            continue;
        }
        let tinted_a = (sa * shadow_alpha) / 255;
        if tinted_a == 0 {
            continue;
        }
        let inv = 255 - tinted_a;
        target[idx] = ((sr * tinted_a + target[idx] as u32 * inv) / 255) as u8;
        target[idx + 1] = ((sg * tinted_a + target[idx + 1] as u32 * inv) / 255) as u8;
        target[idx + 2] = ((sb * tinted_a + target[idx + 2] as u32 * inv) / 255) as u8;
    }
}

fn copy_scaled_systemui_row_clipped(
    frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    y: u32,
    row: &mut [u8],
    render_clip: RenderClip,
) -> Result<(), WindowdError> {
    let row_len = mode.stride as usize;
    if row.len() < row_len || frame.width == 0 || frame.height == 0 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let src_y = *source_y_lut
        .get(y as usize)
        .ok_or(WindowdError::BufferLengthMismatch)? as usize;
    let src_row_base = src_y
        .checked_mul(frame.stride as usize)
        .ok_or(WindowdError::ArithmeticOverflow)?;
    let mut x = render_clip.start_x.min(mode.width) as usize;
    let end_x = render_clip.end_x.min(mode.width) as usize;
    while x < end_x {
        let src_x = *source_x_lut
            .get(x)
            .ok_or(WindowdError::BufferLengthMismatch)? as usize;
        let mut run = 1usize;
        while x + run < end_x {
            let next = *source_x_lut
                .get(x + run)
                .ok_or(WindowdError::BufferLengthMismatch)? as usize;
            if next != src_x.saturating_add(run) {
                break;
            }
            run += 1;
        }
        if run >= 4 {
            let src = src_row_base
                .checked_add(
                    src_x
                        .checked_mul(4)
                        .ok_or(WindowdError::ArithmeticOverflow)?,
                )
                .ok_or(WindowdError::ArithmeticOverflow)?;
            let dst = x.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
            let byte_len = run.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
            row[dst..dst + byte_len].copy_from_slice(
                frame
                    .pixels
                    .get(src..src + byte_len)
                    .ok_or(WindowdError::BufferLengthMismatch)?,
            );
            x += run;
            continue;
        }
        let src = src_y
            .checked_mul(frame.stride as usize)
            .and_then(|base| base.checked_add(src_x.checked_mul(4)?))
            .ok_or(WindowdError::ArithmeticOverflow)?;
        let dst = x.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        row[dst..dst + 4].copy_from_slice(
            frame
                .pixels
                .get(src..src + 4)
                .ok_or(WindowdError::BufferLengthMismatch)?,
        );
        x += 1;
    }
    Ok(())
}

fn build_scale_lut(dest_len: u32, source_len: u32) -> Result<Vec<u32>, WindowdError> {
    if dest_len == 0 || source_len == 0 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let mut lut = Vec::with_capacity(dest_len as usize);
    for dest in 0..dest_len {
        let src = ((u64::from(dest) * u64::from(source_len)) / u64::from(dest_len)) as u32;
        lut.push(src.min(source_len.saturating_sub(1)));
    }
    Ok(lut)
}

fn backdrop_cache_slot(
    y: u32,
    start_x: u32,
    width: u32,
    quality: GlassQuality,
    cache_len: usize,
) -> usize {
    if cache_len == 0 {
        return 0;
    }
    let quality_key = match quality {
        GlassQuality::High => 0usize,
        GlassQuality::Low => 1,
        GlassQuality::Opaque => 2,
    };
    (y as usize)
        .wrapping_mul(131)
        .wrapping_add(start_x as usize * 17)
        .wrapping_add(width as usize * 3)
        .wrapping_add(quality_key)
        % cache_len
}

fn path_cache_slot(
    id_hash: u64,
    width: u32,
    height: u32,
    color: [u8; 4],
    cache_len: usize,
) -> usize {
    if cache_len == 0 {
        return 0;
    }
    (id_hash as usize)
        .wrapping_mul(131)
        .wrapping_add(width as usize * 17)
        .wrapping_add(height as usize * 7)
        .wrapping_add(u32::from_le_bytes(color) as usize)
        % cache_len
}

fn path_id_hash(id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn blend_cached_path_row(
    y: u32,
    row: &mut [u8],
    id: Option<&str>,
    rect: ProofBoxRect,
    path: &nexus_layout_types::PathShape,
    color: [u8; 4],
    path_cache: &mut [PathCacheEntry],
) -> Result<bool, WindowdError> {
    let Some(id) = id else {
        return Ok(false);
    };
    if rect.width as usize > PATH_CACHE_MAX_SIDE || rect.height as usize > PATH_CACHE_MAX_SIDE {
        return Ok(false);
    }
    let id_hash = path_id_hash(id);
    let slot = path_cache_slot(id_hash, rect.width, rect.height, color, path_cache.len());
    let entry = &mut path_cache[slot];
    let pixel_len = rect.width as usize * rect.height as usize * 4;
    if !entry.valid
        || entry.id_hash != id_hash
        || entry.width != rect.width
        || entry.height != rect.height
        || entry.color != color
    {
        entry.pixels[..pixel_len].fill(0);
        for cached_y in 0..rect.height {
            let row_start = cached_y as usize * rect.width as usize * 4;
            let row_end = row_start + rect.width as usize * 4;
            draw_path_row(
                cached_y,
                &mut entry.pixels[row_start..row_end],
                0,
                0,
                rect.width,
                rect.height,
                path,
                color,
            )?;
        }
        entry.id_hash = id_hash;
        entry.width = rect.width;
        entry.height = rect.height;
        entry.color = color;
        entry.valid = true;
    }
    blend_asset_row(
        y,
        row,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        &entry.pixels[..pixel_len],
    )?;
    Ok(true)
}

fn checked_stride(width: u32) -> Result<u32, WindowdError> {
    let bytes = width
        .checked_mul(4)
        .ok_or(WindowdError::ArithmeticOverflow)?;
    bytes
        .checked_add(63)
        .ok_or(WindowdError::ArithmeticOverflow)
        .map(|v| v / 64 * 64)
}

/// Single-row horizontal box blur with variable radius.
/// Zero-allocation: uses `row_buf` (pre-allocated) for the temporary copy.
/// Sliding window: O(width) operations regardless of radius.
fn blur_row_horizontal(pixels: &mut [u8], row_bytes: usize, radius: u32, row_buf: &mut [u8]) {
    if row_bytes == 0 || radius == 0 {
        return;
    }
    let w = row_bytes / 4;
    let r = radius as usize;
    let window = 2 * r + 1;

    row_buf[..row_bytes].copy_from_slice(&pixels[..row_bytes]);

    let (mut r_sum, mut g_sum, mut b_sum, mut a_sum) = (0u64, 0u64, 0u64, 0u64);
    for i in 0..window.min(w) {
        let idx = i * 4;
        let a = row_buf[idx + 3] as u64;
        r_sum += row_buf[idx] as u64 * a;
        g_sum += row_buf[idx + 1] as u64 * a;
        b_sum += row_buf[idx + 2] as u64 * a;
        a_sum += a;
    }

    for x in 0..w {
        let idx = x * 4;
        if a_sum > 0 {
            pixels[idx] = ((r_sum / a_sum).min(255)) as u8;
            pixels[idx + 1] = ((g_sum / a_sum).min(255)) as u8;
            pixels[idx + 2] = ((b_sum / a_sum).min(255)) as u8;
        }
        pixels[idx + 3] = ((a_sum / window as u64).min(255)) as u8;

        let left = x.saturating_sub(r);
        if let Some(lidx) = left.checked_mul(4) {
            let la = row_buf[lidx + 3] as u64;
            r_sum = r_sum.saturating_sub(row_buf[lidx] as u64 * la);
            g_sum = g_sum.saturating_sub(row_buf[lidx + 1] as u64 * la);
            b_sum = b_sum.saturating_sub(row_buf[lidx + 2] as u64 * la);
            a_sum = a_sum.saturating_sub(la);
        }
        let right = x + r + 1;
        if right < w {
            let ridx = right * 4;
            let ra = row_buf[ridx + 3] as u64;
            r_sum += row_buf[ridx] as u64 * ra;
            g_sum += row_buf[ridx + 1] as u64 * ra;
            b_sum += row_buf[ridx + 2] as u64 * ra;
            a_sum += ra;
        }
    }
}

fn draw_proof_surface_row(
    state: VisibleState,
    proof_layout: Option<&LayoutResult>,
    proof_layout_index: Option<&LayoutHotPathIndex>,
    filter_text: &str,
    filtered_words: &[&'static str],
    y: u32,
    row: &mut [u8],
    render_clip: RenderClip,
    backdrop_cache: &mut [BackdropCacheEntry],
    glass_layer: &mut GlassLayerCache,
    glass_scratch: &mut [u8],
    path_cache: &mut [PathCacheEntry],
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    glass_quality: GlassQuality,
    backdrop_scratch: &mut [u8],
    layer_cache: &mut LayerCache,
    paint_only: bool,
) -> Result<(), WindowdError> {
    let Some(layout) = proof_layout else {
        return Ok(());
    };
    let mut filter_input_rect = None;
    let mut filter_list_rect = None;
    let mut filter_list_scroll_y = 0;
    let row_mask =
        proof_layout_index.and_then(|index| (!index.overflow_boxes()).then(|| index.row_mask(y)));
    let mut draw_row_box = |layout_box: &nexus_layout::LayoutBox| -> Result<(), WindowdError> {
        let Some(rect) = proof_box_rect(layout_box) else {
            return Ok(());
        };
        if !rect.contains_y(y) {
            return Ok(());
        }
        let paint_role = layout_box.id.and_then(proof_paint_role);
        draw_layout_box_row(
            state,
            y,
            row,
            layout_box,
            rect,
            paint_role,
            render_clip,
            backdrop_cache,
            glass_layer,
            glass_scratch,
            path_cache,
            source_frame,
            source_x_lut,
            source_y_lut,
            mode,
            glass_quality,
            backdrop_scratch,
            layer_cache,
            paint_only,
        )?;
        if let Some(id) = layout_box.id {
            if id == "filter_text_input" {
                filter_input_rect = Some(rect);
                let asset_id = crate::proof_panel_spec::filter_input_asset_id(filter_text);
                if let Some(asset) = crate::assets::proof_text_asset(asset_id) {
                    blend_asset_row(
                        y,
                        row,
                        rect.x,
                        rect.y,
                        asset.width,
                        asset.height,
                        asset.bgra,
                    )?;
                }
                return Ok(());
            }
            if id == "filter_list" {
                filter_list_rect = Some(rect);
                filter_list_scroll_y = layout_box.scroll_offset.1.as_u32().unwrap_or(0);
                return Ok(());
            }
            if id.starts_with("filter_") {
                return Ok(());
            }
            if let Some(asset) = crate::assets::proof_text_asset(id) {
                blend_asset_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    asset.width,
                    asset.height,
                    asset.bgra,
                )?;
            }
        }
        Ok(())
    };
    match row_mask {
        Some(mut mask) => {
            while mask != 0 {
                let box_idx = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                draw_row_box(&layout.boxes[box_idx])?;
            }
        }
        None => {
            for layout_box in &layout.boxes {
                draw_row_box(layout_box)?;
            }
        }
    }
    if let Some(rect) = filter_input_rect {
        draw_filter_input_text_row(y, row, rect, filter_text)?;
    }
    if let Some(rect) = filter_list_rect {
        draw_filter_word_list_row(y, row, rect, filter_list_scroll_y, filtered_words)?;
    }
    Ok(())
}

fn refill_filtered_words(out: &mut Vec<&'static str>, filter_text: &str) {
    out.clear();
    for &word in crate::proof_panel_spec::FILTER_WORDS {
        if ascii_prefix_matches(word, filter_text) {
            out.push(word);
        }
    }
}

fn filter_word_asset_id(word: &str) -> &'static str {
    match word {
        "apple" => "filter_apple",
        "application" => "filter_application",
        "apt" => "filter_apt",
        "arrow" => "filter_arrow",
        "asset" => "filter_asset",
        "batch" => "filter_batch",
        "binary" => "filter_binary",
        "block" => "filter_block",
        "buffer" => "filter_buffer",
        "build" => "filter_build",
        "cache" => "filter_cache",
        "clock" => "filter_clock",
        "compile" => "filter_compile",
        "component" => "filter_component",
        "config" => "filter_config",
        _ => "filter_word",
    }
}

fn ascii_prefix_matches(word: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let mut word_bytes = word.bytes();
    for prefix_byte in prefix.bytes() {
        let Some(word_byte) = word_bytes.next() else {
            return false;
        };
        if !word_byte.eq_ignore_ascii_case(&prefix_byte) {
            return false;
        }
    }
    true
}

/// Total content height of the filter word list (words + gaps + padding).
fn filter_list_content_height(filtered_words: &[&'static str]) -> u32 {
    let mut h: u32 = 0;
    for &word in filtered_words {
        if let Some(asset) = crate::assets::proof_text_asset(filter_word_asset_id(word)) {
            h = h
                .saturating_add(asset.height)
                .saturating_add(FILTER_LIST_ROW_GAP);
        }
    }
    h.saturating_sub(FILTER_LIST_ROW_GAP) // remove trailing gap
}

fn filter_list_viewport_height(list_height: u32) -> u32 {
    list_height.saturating_sub(FILTER_LIST_PADDING_Y.saturating_mul(2))
}

fn filter_list_viewport_width(list_width: u32) -> u32 {
    list_width
        .saturating_sub(FILTER_LIST_PADDING_X.saturating_mul(2))
        .saturating_sub(layout_panel::FILTER_SCROLLBAR_GUTTER)
}

fn draw_filter_word_list_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    scroll_y: u32,
    filtered_words: &[&'static str],
) -> Result<(), WindowdError> {
    if !rect.contains_y(y) {
        return Ok(());
    }
    let viewport_x = rect.x + FILTER_LIST_PADDING_X;
    let viewport_y = rect.y + FILTER_LIST_PADDING_Y;
    let viewport_height = filter_list_viewport_height(rect.height);
    let viewport_width = filter_list_viewport_width(rect.width);
    let mut word_top = viewport_y;
    for &word in filtered_words {
        let Some(asset) = crate::assets::proof_text_asset(filter_word_asset_id(word)) else {
            continue;
        };
        let asset_top = word_top.saturating_sub(scroll_y);
        if y >= asset_top && y < asset_top.saturating_add(asset.height) {
            blend_asset_row_clipped(
                y,
                row,
                viewport_x,
                asset_top,
                asset.width,
                asset.height,
                asset.bgra,
                viewport_x,
                viewport_width,
            )?;
        }
        word_top = word_top
            .saturating_add(asset.height)
            .saturating_add(FILTER_LIST_ROW_GAP);
    }

    // ── Scrollbar ──
    let content_h = filter_list_content_height(filtered_words);
    if content_h > viewport_height {
        draw_filter_scrollbar_row(y, row, rect, scroll_y, content_h)?;
    }

    Ok(())
}

fn draw_filter_scrollbar_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    scroll_y: u32,
    content_h: u32,
) -> Result<(), WindowdError> {
    let viewport_y = rect.y + FILTER_LIST_PADDING_Y;
    let viewport_height = filter_list_viewport_height(rect.height);
    let strip_x = layout_panel::filter_scrollbar_strip_x(rect.x, rect.width);
    let track_x = layout_panel::filter_scrollbar_track_x(rect.x, rect.width);
    let gutter_width = rect.x.saturating_add(rect.width).saturating_sub(strip_x);
    let track_bgra = rgba_to_bgra(crate::assets::PROOF_PANEL_BG);
    if y >= viewport_y && y < viewport_y.saturating_add(viewport_height) {
        fill_row_rect(
            y,
            row,
            strip_x,
            viewport_y,
            gutter_width,
            viewport_height,
            track_bgra,
        )?;
    }

    let Some((thumb_y, thumb_height)) = layout_panel::filter_scrollbar_thumb_bounds(
        viewport_y,
        viewport_height,
        content_h,
        scroll_y,
    ) else {
        return Ok(());
    };

    let thumb_bgra = rgba_to_bgra(crate::assets::PROOF_SCROLL);
    fill_row_rect(
        y,
        row,
        track_x,
        thumb_y,
        layout_panel::FILTER_SCROLLBAR_WIDTH,
        thumb_height,
        thumb_bgra,
    )
}

fn draw_filter_input_text_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    filter_text: &str,
) -> Result<(), WindowdError> {
    if filter_text.is_empty() {
        return Ok(());
    }
    let glyph_height = FILTER_INPUT_FONT_H * FILTER_INPUT_FONT_SCALE;
    if rect.height <= glyph_height {
        return Ok(());
    }
    let text_top = rect.y + (rect.height - glyph_height) / 2;
    if y < text_top || y >= text_top.saturating_add(glyph_height) {
        return Ok(());
    }
    let glyph_row = ((y - text_top) / FILTER_INPUT_FONT_SCALE) as usize;
    let color = rgba_to_bgra(crate::assets::PROOF_PANEL_TITLE);
    let max_x = rect
        .x
        .saturating_add(rect.width.saturating_sub(FILTER_INPUT_PADDING_X));
    let mut pen_x = rect.x + FILTER_INPUT_PADDING_X;
    for ch in filter_text.chars() {
        if pen_x.saturating_add(FILTER_INPUT_FONT_W * FILTER_INPUT_FONT_SCALE) > max_x {
            break;
        }
        draw_bitmap_glyph_row(y, row, pen_x, glyph_row, ch, color)?;
        pen_x = pen_x.saturating_add(FILTER_INPUT_FONT_ADVANCE);
    }
    if pen_x + 1 < max_x {
        fill_row_rect(
            y,
            row,
            pen_x,
            text_top,
            2,
            glyph_height,
            rgba_to_bgra(crate::assets::PROOF_KEYBOARD),
        )?;
    }
    Ok(())
}

fn draw_bitmap_glyph_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    glyph_row: usize,
    ch: char,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let bits = bitmap_font_5x7(ch)[glyph_row];
    for col in 0..FILTER_INPUT_FONT_W {
        if bits & (1 << (FILTER_INPUT_FONT_W - 1 - col)) == 0 {
            continue;
        }
        fill_row_rect(
            y,
            row,
            x + col * FILTER_INPUT_FONT_SCALE,
            y,
            FILTER_INPUT_FONT_SCALE,
            1,
            bgra,
        )?;
    }
    Ok(())
}

fn bitmap_font_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_lowercase() {
        'a' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'b' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'd' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'e' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'f' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'g' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'h' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'j' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'k' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'm' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'r' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        's' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'w' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'x' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10011, 0b10101, 0b10101, 0b10101, 0b11001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        ' ' => [0; 7],
        _ => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    }
}

/// Blur a horizontal segment of a row (backdrop blur for glass effect).
/// Uses a lightweight 1D box blur with the given radius.
/// Zero-allocation: reuses the shared scanline scratch buffer.
fn blur_backdrop_segment(
    dst: &mut [u8],
    start_x: u32,
    end_x: u32,
    radius: u32,
    scratch: &mut [u8],
) -> Result<(), WindowdError> {
    if end_x <= start_x || radius == 0 {
        return Ok(());
    }
    let r = radius as usize;
    let start = start_x as usize * 4;
    let end = end_x as usize * 4;
    let segment_len = end.saturating_sub(start);
    if end > dst.len() || segment_len > scratch.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }
    scratch[..segment_len].copy_from_slice(&dst[start..end]);
    let pixels = segment_len / 4;
    if pixels == 0 {
        return Ok(());
    }
    let mut sums = [0u32; 4];
    let mut left = 0usize;
    let mut right = r.min(pixels - 1);
    for j in left..=right {
        let bi = j * 4;
        sums[0] += scratch[bi] as u32;
        sums[1] += scratch[bi + 1] as u32;
        sums[2] += scratch[bi + 2] as u32;
        sums[3] += scratch[bi + 3] as u32;
    }
    for i in 0..pixels {
        let count = (right - left + 1) as u32;
        let di = start + i * 4;
        for c in 0..4 {
            dst[di + c] = (sums[c] / count).min(255) as u8;
        }

        if i + 1 < pixels {
            let next_left = (i + 1).saturating_sub(r);
            if next_left > left {
                let bi = left * 4;
                sums[0] = sums[0].saturating_sub(scratch[bi] as u32);
                sums[1] = sums[1].saturating_sub(scratch[bi + 1] as u32);
                sums[2] = sums[2].saturating_sub(scratch[bi + 2] as u32);
                sums[3] = sums[3].saturating_sub(scratch[bi + 3] as u32);
                left = next_left;
            }
            let next_right = (i + 1 + r).min(pixels - 1);
            if next_right > right {
                right = next_right;
                let bi = right * 4;
                sums[0] += scratch[bi] as u32;
                sums[1] += scratch[bi + 1] as u32;
                sums[2] += scratch[bi + 2] as u32;
                sums[3] += scratch[bi + 3] as u32;
            }
        }
    }
    Ok(())
}

fn apply_backdrop_cache_row(
    row: &mut [u8],
    y: u32,
    start_x: u32,
    end_x: u32,
    quality: GlassQuality,
    cache_entries: &mut [BackdropCacheEntry],
    scratch: &mut [u8],
) -> Result<(), WindowdError> {
    if end_x <= start_x {
        return Ok(());
    }
    let width = end_x.saturating_sub(start_x);
    let segment_len = width as usize * 4;
    if segment_len > BACKDROP_CACHE_MAX_WIDTH * 4 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let row_start = start_x as usize * 4;
    let row_end = end_x as usize * 4;
    if row_end > row.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }
    if let Some(entry) = cache_entries.iter().find(|entry| {
        entry.valid
            && entry.y == y
            && entry.start_x == start_x
            && entry.width == width
            && entry.quality == quality
    }) {
        row[row_start..row_end].copy_from_slice(&entry.pixels[..segment_len]);
        return Ok(());
    }
    let slot = backdrop_cache_slot(y, start_x, width, quality, cache_entries.len());
    let entry = &mut cache_entries[slot];
    entry.pixels[..segment_len].copy_from_slice(&row[row_start..row_end]);
    blur_backdrop_segment(
        &mut entry.pixels[..segment_len],
        0,
        width,
        quality.blur_radius(),
        scratch,
    )?;
    saturate_bgra_segment(
        &mut entry.pixels[..segment_len],
        0,
        width,
        DARK_GLASS_SATURATION_PERCENT,
    );
    entry.y = y;
    entry.start_x = start_x;
    entry.width = width;
    entry.quality = quality;
    entry.valid = true;
    row[row_start..row_end].copy_from_slice(&entry.pixels[..segment_len]);
    Ok(())
}

fn glass_layer_key(rect: ProofBoxRect, quality: GlassQuality) -> u64 {
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    key ^= rect.x as u64;
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^= (rect.y as u64).rotate_left(7);
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^= (rect.width as u64).rotate_left(17);
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^= (rect.height as u64).rotate_left(29);
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^ ((quality.blur_radius() as u64).rotate_left(41))
}

fn sample_wallpaper_pixel(
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    x: u32,
    y: u32,
) -> Result<[u8; 4], WindowdError> {
    let x = x.min(mode.width.saturating_sub(1));
    let y = y.min(mode.height.saturating_sub(1));
    let src_x = *source_x_lut
        .get(x as usize)
        .ok_or(WindowdError::BufferLengthMismatch)? as usize;
    let src_y = *source_y_lut
        .get(y as usize)
        .ok_or(WindowdError::BufferLengthMismatch)? as usize;
    let src = src_y
        .checked_mul(source_frame.stride as usize)
        .and_then(|base| base.checked_add(src_x.checked_mul(4)?))
        .ok_or(WindowdError::ArithmeticOverflow)?;
    let px = source_frame
        .pixels
        .get(src..src + 4)
        .ok_or(WindowdError::BufferLengthMismatch)?;
    Ok([px[0], px[1], px[2], px[3]])
}

fn ensure_glass_layer(
    layer: &mut GlassLayerCache,
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    rect: ProofBoxRect,
    quality: GlassQuality,
    row_scratch: &mut [u8],
    glass_scratch: &mut [u8],
) -> Result<(), WindowdError> {
    let key = glass_layer_key(rect, quality);
    let bounds = DamageRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    if layer.valid && layer.key == key && layer.rect == bounds {
        return Ok(());
    }

    let cache_w = rect.width.div_ceil(GLASS_LAYER_SCALE).max(1);
    let cache_h = rect.height.div_ceil(GLASS_LAYER_SCALE).max(1);
    let layer_len = cache_w as usize * cache_h as usize * 4;
    if layer_len > layer.pixels.len() || layer_len > glass_scratch.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }

    for py in 0..cache_h {
        for px in 0..cache_w {
            let sample_x = rect
                .x
                .saturating_add(px.saturating_mul(GLASS_LAYER_SCALE))
                .saturating_add(GLASS_LAYER_SCALE / 2)
                .min(rect.x.saturating_add(rect.width.saturating_sub(1)));
            let sample_y = rect
                .y
                .saturating_add(py.saturating_mul(GLASS_LAYER_SCALE))
                .saturating_add(GLASS_LAYER_SCALE / 2)
                .min(rect.y.saturating_add(rect.height.saturating_sub(1)));
            let src = sample_wallpaper_pixel(
                source_frame,
                source_x_lut,
                source_y_lut,
                mode,
                sample_x,
                sample_y,
            )?;
            let idx = (py as usize * cache_w as usize + px as usize) * 4;
            layer.pixels[idx..idx + 4].copy_from_slice(&src);
        }
    }

    if quality != GlassQuality::Opaque {
        let blur_radius = DARK_GLASS_BLUR_RADIUS
            .min(quality.blur_radius())
            .div_ceil(GLASS_LAYER_SCALE)
            .max(1);
        blur_separable_zero_alloc(
            &mut layer.pixels[..layer_len],
            cache_w,
            cache_h,
            cache_w * 4,
            blur_radius,
            row_scratch,
            glass_scratch,
        );
        saturate_bgra_segment(
            &mut layer.pixels[..layer_len],
            0,
            cache_w,
            DARK_GLASS_SATURATION_PERCENT,
        );
    }

    layer.key = key;
    layer.rect = bounds;
    layer.width = cache_w;
    layer.height = cache_h;
    layer.valid = true;
    Ok(())
}

fn sample_glass_layer(layer: &GlassLayerCache, x: u32, y: u32) -> [u8; 4] {
    let local_x = x.saturating_sub(layer.rect.x);
    let local_y = y.saturating_sub(layer.rect.y);
    let sx = local_x / GLASS_LAYER_SCALE;
    let sy = local_y / GLASS_LAYER_SCALE;
    let x0 = sx.min(layer.width.saturating_sub(1));
    let y0 = sy.min(layer.height.saturating_sub(1));
    let x1 = x0.saturating_add(1).min(layer.width.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(layer.height.saturating_sub(1));
    let fx = local_x % GLASS_LAYER_SCALE;
    let fy = local_y % GLASS_LAYER_SCALE;
    let wx1 = fx;
    let wx0 = GLASS_LAYER_SCALE.saturating_sub(fx);
    let wy1 = fy;
    let wy0 = GLASS_LAYER_SCALE.saturating_sub(fy);
    let sample = |px: u32, py: u32, c: usize| -> u32 {
        let idx = (py as usize * layer.width as usize + px as usize) * 4 + c;
        layer.pixels.get(idx).copied().unwrap_or(0) as u32
    };
    let denom = GLASS_LAYER_SCALE * GLASS_LAYER_SCALE;
    let mut out = [0u8; 4];
    for (c, dst) in out.iter_mut().enumerate() {
        let v = sample(x0, y0, c) * wx0 * wy0
            + sample(x1, y0, c) * wx1 * wy0
            + sample(x0, y1, c) * wx0 * wy1
            + sample(x1, y1, c) * wx1 * wy1;
        *dst = (v / denom).min(255) as u8;
    }
    out
}

fn stroke_dark_glass_border_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    render_clip: RenderClip,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if stroke == 0 {
        return Ok(());
    }
    let row_pixels = (row.len() / 4) as u32;
    let start = rect.x.max(render_clip.start_x).min(row_pixels);
    let end = rect
        .x
        .saturating_add(rect.width)
        .min(render_clip.end_x)
        .min(row_pixels);
    if start >= end {
        return Ok(());
    }
    let min_x = fixed_sdf::px_u32(rect.x);
    let min_y = fixed_sdf::px_u32(rect.y);
    let max_x = fixed_sdf::px_u32(rect.x.saturating_add(rect.width));
    let max_y = fixed_sdf::px_u32(rect.y.saturating_add(rect.height));
    let radius = fixed_sdf::px_u32(DARK_GLASS_RADIUS);
    let point_y = fixed_sdf::pixel_center(y);
    for px in start..end {
        let sd = fixed_sdf::rounded_rect_sd(
            fixed_sdf::pixel_center(px),
            point_y,
            min_x,
            min_y,
            max_x,
            max_y,
            radius,
        );
        let alpha = fixed_sdf::border_alpha(sd, stroke).saturating_mul(u32::from(bgra[3])) / 255;
        if alpha == 0 {
            continue;
        }
        let idx = px as usize * 4;
        let inv = 255u32.saturating_sub(alpha);
        row[idx] = ((u32::from(bgra[0]) * alpha + u32::from(row[idx]) * inv) / 255) as u8;
        row[idx + 1] = ((u32::from(bgra[1]) * alpha + u32::from(row[idx + 1]) * inv) / 255) as u8;
        row[idx + 2] = ((u32::from(bgra[2]) * alpha + u32::from(row[idx + 2]) * inv) / 255) as u8;
    }
    Ok(())
}

fn draw_combined_panel_glass_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    render_clip: RenderClip,
    quality: GlassQuality,
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    glass_layer: &mut GlassLayerCache,
    row_scratch: &mut [u8],
    glass_scratch: &mut [u8],
) -> Result<(), WindowdError> {
    if !rect.contains_y(y) {
        return Ok(());
    }
    ensure_glass_layer(
        glass_layer,
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        rect,
        quality,
        row_scratch,
        glass_scratch,
    )?;
    let row_pixels = (row.len() / 4) as u32;
    let start = rect.x.max(render_clip.start_x).min(row_pixels);
    let end = rect
        .x
        .saturating_add(rect.width)
        .min(render_clip.end_x)
        .min(row_pixels);
    if start >= end {
        return Ok(());
    }
    let tint_a = DARK_GLASS_TINT.a as u32;
    let inv_tint = 255u32.saturating_sub(tint_a);
    let interior_left = rect.x.saturating_add(DARK_GLASS_RADIUS);
    let interior_right = rect
        .x
        .saturating_add(rect.width.saturating_sub(DARK_GLASS_RADIUS));
    let interior_top = rect.y.saturating_add(DARK_GLASS_RADIUS);
    let interior_bottom = rect
        .y
        .saturating_add(rect.height.saturating_sub(DARK_GLASS_RADIUS));
    if start >= interior_left && end <= interior_right && y >= interior_top && y < interior_bottom {
        for px in start..end {
            let blurred = sample_glass_layer(glass_layer, px, y);
            let idx = px as usize * 4;
            row[idx] =
                ((blurred[0] as u32 * inv_tint + DARK_GLASS_TINT.b as u32 * tint_a) / 255) as u8;
            row[idx + 1] =
                ((blurred[1] as u32 * inv_tint + DARK_GLASS_TINT.g as u32 * tint_a) / 255) as u8;
            row[idx + 2] =
                ((blurred[2] as u32 * inv_tint + DARK_GLASS_TINT.r as u32 * tint_a) / 255) as u8;
        }
        return Ok(());
    }
    let min_x = fixed_sdf::px_u32(rect.x);
    let min_y = fixed_sdf::px_u32(rect.y);
    let max_x = fixed_sdf::px_u32(rect.x.saturating_add(rect.width));
    let max_y = fixed_sdf::px_u32(rect.y.saturating_add(rect.height));
    let radius = fixed_sdf::px_u32(DARK_GLASS_RADIUS);
    let point_y = fixed_sdf::pixel_center(y);
    for px in start..end {
        let sd = fixed_sdf::rounded_rect_sd(
            fixed_sdf::pixel_center(px),
            point_y,
            min_x,
            min_y,
            max_x,
            max_y,
            radius,
        );
        let mask = fixed_sdf::fill_alpha(sd);
        if mask == 0 {
            continue;
        }
        let blurred = sample_glass_layer(glass_layer, px, y);
        let final_b = (blurred[0] as u32 * inv_tint + DARK_GLASS_TINT.b as u32 * tint_a) / 255;
        let final_g = (blurred[1] as u32 * inv_tint + DARK_GLASS_TINT.g as u32 * tint_a) / 255;
        let final_r = (blurred[2] as u32 * inv_tint + DARK_GLASS_TINT.r as u32 * tint_a) / 255;
        let inv_mask = 255u32.saturating_sub(mask);
        let idx = px as usize * 4;
        row[idx] = ((final_b * mask + row[idx] as u32 * inv_mask) / 255) as u8;
        row[idx + 1] = ((final_g * mask + row[idx + 1] as u32 * inv_mask) / 255) as u8;
        row[idx + 2] = ((final_r * mask + row[idx + 2] as u32 * inv_mask) / 255) as u8;
    }
    stroke_dark_glass_border_row(
        y,
        row,
        rect,
        render_clip,
        1,
        rgba_to_bgra(DARK_GLASS_BORDER),
    )
}

fn saturate_bgra_segment(row: &mut [u8], start_x: u32, end_x: u32, saturation_percent: u32) {
    if end_x <= start_x || saturation_percent == 100 {
        return;
    }
    let start = start_x as usize * 4;
    let end = (end_x as usize * 4).min(row.len());
    let sat = saturation_percent as i32;
    let mut idx = start;
    while idx + 3 < end {
        let b = row[idx] as i32;
        let g = row[idx + 1] as i32;
        let r = row[idx + 2] as i32;
        let gray = (29 * b + 150 * g + 77 * r) >> 8;
        row[idx] = (gray + ((b - gray) * sat) / 100).clamp(0, 255) as u8;
        row[idx + 1] = (gray + ((g - gray) * sat) / 100).clamp(0, 255) as u8;
        row[idx + 2] = (gray + ((r - gray) * sat) / 100).clamp(0, 255) as u8;
        idx += 4;
    }
}

/// SDF-based filled circle row renderer (anti-aliased edges).
fn fill_sdf_circle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let row_pixels = (row.len() / 4) as u32;
    let cx = x as f32 + width as f32 * 0.5;
    let cy = rect_y as f32 + height as f32 * 0.5;
    let radius = width.min(height) as f32 * 0.5;
    let start = x.max(0);
    let end = (x + width).min(row_pixels);
    for px in start..end {
        let sd = nexus_sdf::sd_circle((px as f32 + 0.5, y as f32 + 0.5), (cx, cy), radius);
        let alpha = nexus_sdf::fill_alpha(sd, 1.0);
        if alpha > 0.0 {
            let idx = px as usize * 4;
            let a = (alpha * bgra[3] as f32) as u32;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            row[idx] = ((bgra[0] as u32 * a + row[idx] as u32 * inv) / 255) as u8;
            row[idx + 1] = ((bgra[1] as u32 * a + row[idx + 1] as u32 * inv) / 255) as u8;
            row[idx + 2] = ((bgra[2] as u32 * a + row[idx + 2] as u32 * inv) / 255) as u8;
        }
    }
    Ok(())
}

/// SDF-based stroked circle row renderer (anti-aliased border).
fn stroke_sdf_circle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if stroke == 0 {
        return Ok(());
    }
    let row_pixels = (row.len() / 4) as u32;
    let cx = x as f32 + width as f32 * 0.5;
    let cy = rect_y as f32 + height as f32 * 0.5;
    let radius = width.min(height) as f32 * 0.5;
    let start = x.max(0);
    let end = (x + width).min(row_pixels);
    for px in start..end {
        let sd = nexus_sdf::sd_circle((px as f32 + 0.5, y as f32 + 0.5), (cx, cy), radius);
        let alpha = nexus_sdf::border_alpha(sd, stroke as f32, 1.0);
        if alpha > 0.0 {
            let idx = px as usize * 4;
            let a = (alpha * bgra[3] as f32) as u32;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            row[idx] = ((bgra[0] as u32 * a + row[idx] as u32 * inv) / 255) as u8;
            row[idx + 1] = ((bgra[1] as u32 * a + row[idx + 1] as u32 * inv) / 255) as u8;
            row[idx + 2] = ((bgra[2] as u32 * a + row[idx + 2] as u32 * inv) / 255) as u8;
        }
    }
    Ok(())
}

/// SDF-based filled rounded rectangle row renderer.
fn fill_sdf_rounded_rect_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    cr: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let row_pixels = (row.len() / 4) as u32;
    let cr_f = cr as f32;
    let min = (rect.x as f32, rect.y as f32);
    let max = ((rect.x + rect.width) as f32, (rect.y + rect.height) as f32);
    let start = rect.x.max(0);
    let end = (rect.x + rect.width).min(row_pixels);
    for px in start..end {
        let sd = nexus_sdf::sd_rounded_rect((px as f32 + 0.5, y as f32 + 0.5), min, max, cr_f);
        let alpha = nexus_sdf::fill_alpha(sd, 1.0);
        if alpha > 0.0 {
            let idx = px as usize * 4;
            let a = (alpha * bgra[3] as f32) as u32;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            row[idx] = ((bgra[0] as u32 * a + row[idx] as u32 * inv) / 255) as u8;
            row[idx + 1] = ((bgra[1] as u32 * a + row[idx + 1] as u32 * inv) / 255) as u8;
            row[idx + 2] = ((bgra[2] as u32 * a + row[idx + 2] as u32 * inv) / 255) as u8;
        }
    }
    Ok(())
}

/// SDF-based stroked rounded rectangle row renderer.
fn stroke_sdf_rounded_rect_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    cr: u32,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if stroke == 0 {
        return Ok(());
    }
    let row_pixels = (row.len() / 4) as u32;
    let cr_f = cr as f32;
    let min = (rect.x as f32, rect.y as f32);
    let max = ((rect.x + rect.width) as f32, (rect.y + rect.height) as f32);
    let start = rect.x.max(0);
    let end = (rect.x + rect.width).min(row_pixels);
    for px in start..end {
        let sd = nexus_sdf::sd_rounded_rect((px as f32 + 0.5, y as f32 + 0.5), min, max, cr_f);
        let alpha = nexus_sdf::border_alpha(sd, stroke as f32, 1.0);
        if alpha > 0.0 {
            let idx = px as usize * 4;
            let a = (alpha * bgra[3] as f32) as u32;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            row[idx] = ((bgra[0] as u32 * a + row[idx] as u32 * inv) / 255) as u8;
            row[idx + 1] = ((bgra[1] as u32 * a + row[idx + 1] as u32 * inv) / 255) as u8;
            row[idx + 2] = ((bgra[2] as u32 * a + row[idx + 2] as u32 * inv) / 255) as u8;
        }
    }
    Ok(())
}

fn draw_layout_box_row(
    state: VisibleState,
    y: u32,
    row: &mut [u8],
    layout_box: &nexus_layout::LayoutBox,
    rect: ProofBoxRect,
    paint_role: Option<ProofPaintRole>,
    render_clip: RenderClip,
    backdrop_cache: &mut [BackdropCacheEntry],
    glass_layer: &mut GlassLayerCache,
    glass_scratch: &mut [u8],
    path_cache: &mut [PathCacheEntry],
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    glass_quality: GlassQuality,
    backdrop_scratch: &mut [u8],
    layer_cache: &mut LayerCache,
    paint_only: bool,
) -> Result<(), WindowdError> {
    // Phase 2: check retained layer cache — skip rendering if layer is clean.
    let cache_key = layout_box.id.map(layer_cache_key);
    if let Some(cached) = cache_key.and_then(|key| layer_cache.get(key)) {
        if !cached.dirty {
            // Layer is clean: blit this row from the cached pixels
            let row_pixels = (row.len() / 4) as u32;
            let cache_stride = cached.bounds.width as usize * 4;
            let local_y = y.saturating_sub(cached.bounds.y);
            let src_start = local_y as usize * cache_stride;
            let start_x = cached.bounds.x.min(row_pixels);
            let end_x = cached.bounds.end_x().min(row_pixels);
            let local_start_x = start_x.saturating_sub(cached.bounds.x);
            let local_end_x = end_x
                .saturating_sub(cached.bounds.x)
                .min(cached.bounds.width);
            let dst_start = start_x as usize * 4;
            let dst_end = end_x as usize * 4;
            let src_byte_start = src_start + local_start_x as usize * 4;
            let src_byte_end = src_start + local_end_x as usize * 4;
            if dst_end > dst_start && src_byte_end <= cached.pixels.len() {
                row[dst_start..dst_end]
                    .copy_from_slice(&cached.pixels[src_byte_start..src_byte_end]);
            }
            return Ok(());
        }
    }

    if layout_box.id == Some("combined_panels") {
        return draw_combined_panel_glass_row(
            y,
            row,
            rect,
            render_clip,
            glass_quality,
            source_frame,
            source_x_lut,
            source_y_lut,
            mode,
            glass_layer,
            backdrop_scratch,
            glass_scratch,
        );
    }

    // Paint-only updates redraw only active target content. Existing glass,
    // shadow, and wallpaper remain in the framebuffer outside the target rect.
    if paint_only && paint_role.is_none() {
        // This box is unchanged; skip re-rendering.
        return Ok(());
    }

    let opacity_alpha: u32 = match layout_box.visual.opacity {
        Some(f) => f.as_u8() as u32,
        None => 255,
    };
    let want_backdrop = opacity_alpha < 255 && layout_box.visual.background.is_some();
    let cache_static_layer = cache_key.is_some()
        && paint_role.is_none()
        && !want_backdrop
        && static_layer_has_cacheable_paint(layout_box)
        && layout_box.visual.shadow.is_none()
        && layout_box.id.is_some_and(static_layer_cacheable_id);
    if want_backdrop {
        let row_pixels = (row.len() / 4) as u32;
        let start = rect.x.max(0);
        let end = (rect.x + rect.width).min(row_pixels);
        if glass_quality == GlassQuality::Opaque {
            // Deterministic degrade: skip blur entirely under wide dirty spans and let the
            // translucent fill below become the only panel treatment for this frame.
        } else if rect.width as usize <= BACKDROP_CACHE_MAX_WIDTH {
            apply_backdrop_cache_row(
                row,
                y,
                start,
                end,
                glass_quality,
                backdrop_cache,
                backdrop_scratch,
            )?;
        } else {
            blur_backdrop_segment(
                row,
                start,
                end,
                glass_quality.blur_radius(),
                backdrop_scratch,
            )?;
            saturate_bgra_segment(row, start, end, DARK_GLASS_SATURATION_PERCENT);
        }
    }

    let get_effective_bgra = |layout_box: &nexus_layout::LayoutBox| -> Option<[u8; 4]> {
        let bg = proof_box_background(layout_box, state, paint_role)?;
        let mut bgra = rgba_to_bgra(bg);
        if opacity_alpha < 255 {
            bgra[3] = ((bgra[3] as u32 * opacity_alpha) / 255) as u8;
        }
        Some(bgra)
    };

    match &layout_box.visual.shape {
        nexus_layout_types::ShapeKind::Rect => {
            let cr = layout_box
                .visual
                .corner_radius
                .top_left
                .as_u32()
                .unwrap_or(0);
            if cr > 0 {
                // SDF rounded rect path (anti-aliased corners)
                if let Some(bgra) = get_effective_bgra(layout_box) {
                    fill_sdf_rounded_rect_row(y, row, rect, cr, bgra)?;
                }
                if let Some((border_width, border_color)) =
                    proof_box_border(layout_box, state, paint_role)
                {
                    stroke_sdf_rounded_rect_row(
                        y,
                        row,
                        rect,
                        cr,
                        border_width,
                        rgba_to_bgra(border_color),
                    )?;
                }
            } else {
                // Fast path: hard-edged rect
                if let Some(bgra) = get_effective_bgra(layout_box) {
                    fill_row_rect(y, row, rect.x, rect.y, rect.width, rect.height, bgra)?;
                }
                if let Some((border_width, border_color)) =
                    proof_box_border(layout_box, state, paint_role)
                {
                    stroke_row_rect_width(
                        y,
                        row,
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        border_width,
                        rgba_to_bgra(border_color),
                    )?;
                }
            }
        }
        nexus_layout_types::ShapeKind::Circle => {
            // SDF circle path (anti-aliased edges)
            if let Some(bgra) = get_effective_bgra(layout_box) {
                fill_sdf_circle_row(y, row, rect.x, rect.y, rect.width, rect.height, bgra)?;
            }
            if let Some((border_width, border_color)) =
                proof_box_border(layout_box, state, paint_role)
            {
                stroke_sdf_circle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    border_width,
                    rgba_to_bgra(border_color),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::TriangleUp => {
            if let Some(background) = proof_box_background(layout_box, state, paint_role) {
                fill_triangle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    true,
                    rgba_to_bgra(background),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::TriangleDown => {
            if let Some(background) = proof_box_background(layout_box, state, paint_role) {
                fill_triangle_row(
                    y,
                    row,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    false,
                    rgba_to_bgra(background),
                )?;
            }
        }
        nexus_layout_types::ShapeKind::Path(path) => {
            let color = proof_box_border(layout_box, state, paint_role)
                .map(|(_, color)| rgba_to_bgra(color))
                .or_else(|| proof_box_background(layout_box, state, paint_role).map(rgba_to_bgra))
                .unwrap_or([0xff, 0xff, 0xff, 0xff]);
            if !blend_cached_path_row(y, row, layout_box.id, rect, path, color, path_cache)? {
                draw_path_row(y, row, rect.x, rect.y, rect.width, rect.height, path, color)?;
            }
        }
    }
    if cache_static_layer {
        if let Some(cache_key) = cache_key {
            record_layer_cache_row(
                layer_cache,
                cache_key,
                rect,
                y,
                row,
                opacity_alpha as u8,
                None,
            )?;
        }
    }
    Ok(())
}

fn layer_cache_key(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn static_layer_cacheable_id(id: &str) -> bool {
    !matches!(
        id,
        "combined_panels"
            | "filter_text_input"
            | "filter_list"
            | "card_hover"
            | "card_click"
            | "card_scroll"
            | "card_key"
    ) && !id.starts_with("filter_")
}

fn static_layer_has_cacheable_paint(layout_box: &nexus_layout::LayoutBox) -> bool {
    layout_box.visual.background.is_some()
        || layout_box.visual.border.top.is_some()
        || matches!(
            layout_box.visual.shape,
            nexus_layout_types::ShapeKind::Path(_)
        )
}

fn record_layer_cache_row(
    layer_cache: &mut LayerCache,
    id: u64,
    rect: ProofBoxRect,
    y: u32,
    row: &[u8],
    opacity: u8,
    backdrop_blur: Option<u32>,
) -> Result<(), WindowdError> {
    if !rect.contains_y(y) {
        return Ok(());
    }
    let bounds = DamageRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    let needs_insert = layer_cache
        .get(id)
        .map(|layer| {
            layer.bounds != bounds
                || layer.pixels.len() != rect.width as usize * rect.height as usize * 4
        })
        .unwrap_or(true);
    if needs_insert {
        let pixel_count = rect.width as usize * rect.height as usize * 4;
        if pixel_count > LAYER_CACHE_MAX_LAYER_BYTES
            || layer_cache.used_bytes().saturating_add(pixel_count) > LAYER_CACHE_MAX_BYTES
        {
            return Ok(());
        }
        layer_cache.insert(Layer::new(id, bounds, opacity, backdrop_blur));
    }
    let row_pixels = (row.len() / 4) as u32;
    let start_x = bounds.x.min(row_pixels);
    let end_x = bounds.end_x().min(row_pixels);
    if start_x >= end_x {
        return Ok(());
    }
    let Some(layer) = layer_cache.get_mut(id) else {
        return Ok(());
    };
    layer.opacity = opacity;
    layer.backdrop_blur = backdrop_blur;
    let local_y = y.saturating_sub(bounds.y);
    if local_y >= bounds.height {
        return Ok(());
    }
    let local_start_x = start_x.saturating_sub(bounds.x);
    let local_end_x = end_x.saturating_sub(bounds.x).min(bounds.width);
    let dst_start =
        (local_y as usize * bounds.width as usize + local_start_x as usize).saturating_mul(4);
    let dst_end =
        (local_y as usize * bounds.width as usize + local_end_x as usize).saturating_mul(4);
    let src_start = start_x as usize * 4;
    let src_end = end_x as usize * 4;
    if dst_end <= layer.pixels.len() && src_end <= row.len() {
        layer.pixels[dst_start..dst_end].copy_from_slice(&row[src_start..src_end]);
        layer.rows_filled = layer.rows_filled.saturating_add(1).min(bounds.height);
        if layer.rows_filled >= bounds.height {
            layer.dirty = false;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ProofBoxRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ProofBoxRect {
    fn contains_y(self, y: u32) -> bool {
        y >= self.y && y < self.y.saturating_add(self.height)
    }
}

fn proof_box_rect(layout_box: &nexus_layout::LayoutBox) -> Option<ProofBoxRect> {
    let width = layout_box.rect.width.as_u32().unwrap_or(0);
    let height = layout_box.rect.height.as_u32().unwrap_or(0);
    if width == 0 || height == 0 {
        return None;
    }
    let x = PROOF_PANEL_X + layout_box.rect.x.as_u32().unwrap_or(0);
    let y = PROOF_PANEL_Y + layout_box.rect.y.as_u32().unwrap_or(0);
    // Clip to clip_rect: if the box has a scissor rect, intersect with it
    if let Some(clip) = layout_box.clip_rect {
        let clip_x = PROOF_PANEL_X + clip.x.as_u32().unwrap_or(0);
        let clip_y = PROOF_PANEL_Y + clip.y.as_u32().unwrap_or(0);
        let clip_w = clip.width.as_u32().unwrap_or(0);
        let clip_h = clip.height.as_u32().unwrap_or(0);
        if clip_w == 0 || clip_h == 0 {
            return None;
        }
        // Intersect: box must overlap clip rect
        if x + width <= clip_x
            || clip_x + clip_w <= x
            || y + height <= clip_y
            || clip_y + clip_h <= y
        {
            return None; // completely outside clip rect
        }
    }
    Some(ProofBoxRect {
        x,
        y,
        width,
        height,
    })
}

fn proof_box_background(
    layout_box: &nexus_layout::LayoutBox,
    state: VisibleState,
    paint_role: Option<ProofPaintRole>,
) -> Option<Rgba8> {
    let Some(role) = paint_role else {
        if layout_box.id == Some("combined_panels") {
            return Some(Rgba8::new(28, 28, 30, 178));
        }
        return layout_box.visual.background;
    };
    let card = role.card.paint(state);
    match role.part {
        ProofPaintPart::Root => Some(if card.active {
            crate::assets::PROOF_CARD_ACTIVE_BG
        } else {
            crate::assets::PROOF_CARD_BG
        }),
        ProofPaintPart::Icon => Some(card.accent),
        ProofPaintPart::Dot => Some(if card.active {
            crate::assets::PROOF_ICON_FG
        } else {
            crate::assets::PROOF_CARD_BG
        }),
        ProofPaintPart::Glyph => Some(if card.active {
            crate::assets::PROOF_ICON_FG
        } else {
            crate::assets::PROOF_CARD_BORDER
        }),
        ProofPaintPart::ScrollUp => Some(if state.wheel_up_visible {
            crate::assets::PROOF_ICON_FG
        } else {
            card.accent
        }),
        ProofPaintPart::ScrollDown => Some(if state.wheel_down_visible {
            crate::assets::PROOF_ICON_FG
        } else {
            crate::assets::PROOF_CARD_BORDER
        }),
        ProofPaintPart::FilterContent => Some(crate::assets::PROOF_CARD_BG),
        // Keep filter text nodes transparent and let the text renderer provide the glyphs.
        // Filling these text boxes produced long bar-like artifacts during scroll.
        ProofPaintPart::FilterWord => layout_box.visual.background,
    }
}

fn proof_box_border(
    layout_box: &nexus_layout::LayoutBox,
    state: VisibleState,
    paint_role: Option<ProofPaintRole>,
) -> Option<(u32, Rgba8)> {
    let border = layout_box.visual.border.top?;
    let width = border.width.as_u32().unwrap_or(1);
    let color = match paint_role {
        Some(ProofPaintRole {
            card,
            part: ProofPaintPart::Root | ProofPaintPart::Icon,
        }) => {
            let paint = card.paint(state);
            if paint.active {
                paint.accent
            } else {
                crate::assets::PROOF_CARD_BORDER
            }
        }
        _ => border.color,
    };
    Some((width, color))
}

#[derive(Clone, Copy)]
struct ProofCardPaint {
    active: bool,
    accent: Rgba8,
}

#[derive(Clone, Copy)]
struct ProofPaintRole {
    card: ProofCard,
    part: ProofPaintPart,
}

#[derive(Clone, Copy)]
enum ProofCard {
    Hover,
    Click,
    Scroll,
    Key,
    Filter,
}

impl ProofCard {
    fn paint(self, state: VisibleState) -> ProofCardPaint {
        match self {
            Self::Hover => ProofCardPaint {
                active: state.hover_visible,
                accent: crate::assets::PROOF_HOVER,
            },
            Self::Click => ProofCardPaint {
                active: state.launcher_click_visible,
                accent: crate::assets::PROOF_CLICK,
            },
            Self::Scroll => ProofCardPaint {
                active: state.wheel_up_visible || state.wheel_down_visible,
                accent: crate::assets::PROOF_SCROLL,
            },
            Self::Key => ProofCardPaint {
                active: state.keyboard_visible,
                accent: crate::assets::PROOF_KEYBOARD,
            },
            Self::Filter => ProofCardPaint {
                active: true,
                accent: crate::assets::PROOF_PANEL_TITLE,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum ProofPaintPart {
    Root,
    Icon,
    Dot,
    Glyph,
    ScrollUp,
    ScrollDown,
    FilterContent,
    FilterWord,
}

fn proof_paint_role(id: &str) -> Option<ProofPaintRole> {
    use ProofCard::{Click, Filter, Hover, Key, Scroll};
    use ProofPaintPart::{Dot, FilterContent, FilterWord, Glyph, Icon, Root, ScrollDown, ScrollUp};

    let (card, part) = match id {
        "card_hover" => (Hover, Root),
        "card_hover_icon" => (Hover, Icon),
        "card_hover_dot" => (Hover, Dot),
        "card_hover_glyph" => (Hover, Glyph),
        "card_click" => (Click, Root),
        "card_click_icon" => (Click, Icon),
        "card_click_dot" => (Click, Dot),
        "card_click_glyph" => (Click, Glyph),
        "card_scroll" => (Scroll, Root),
        "card_scroll_icon" => (Scroll, Icon),
        "card_scroll_dot" => (Scroll, Dot),
        "card_scroll_up" => (Scroll, ScrollUp),
        "card_scroll_down" => (Scroll, ScrollDown),
        "card_key" => (Key, Root),
        "card_key_icon" => (Key, Icon),
        "card_key_dot" => (Key, Dot),
        "card_key_glyph" => (Key, Glyph),
        "filter_panel" => (Filter, Root),
        "filter_content" => (Filter, FilterContent),
        "filter_text_input" => (Filter, FilterWord),
        "filter_list" => (Filter, FilterContent),
        "filter_word" => (Filter, FilterWord),
        // Filter word asset IDs point to pre-rendered text
        id if id.starts_with("filter_") => (Filter, FilterWord),
        _ => return None,
    };
    Some(ProofPaintRole { card, part })
}

fn blend_asset_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    width: u32,
    height: u32,
    source: &[u8],
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(height) {
        return Ok(());
    }
    let source_y = y - top;
    let src_row = source_y as usize * width as usize * 4;
    let source = source
        .get(src_row..src_row + width as usize * 4)
        .ok_or(WindowdError::BufferLengthMismatch)?;
    blend_overlay_row(row, x as usize, source)
}

fn blend_asset_row_clipped(
    y: u32,
    row: &mut [u8],
    x: u32,
    top: u32,
    width: u32,
    height: u32,
    source: &[u8],
    clip_x: u32,
    clip_width: u32,
) -> Result<(), WindowdError> {
    if y < top || y >= top.saturating_add(height) || clip_width == 0 {
        return Ok(());
    }
    let visible_x = x.max(clip_x);
    let visible_end = x
        .saturating_add(width)
        .min(clip_x.saturating_add(clip_width))
        .min((row.len() / 4) as u32);
    if visible_end <= visible_x {
        return Ok(());
    }
    let source_y = y - top;
    let src_row = source_y as usize * width as usize * 4;
    let src_offset = visible_x.saturating_sub(x) as usize * 4;
    let src_len = visible_end.saturating_sub(visible_x) as usize * 4;
    let source = source
        .get(src_row + src_offset..src_row + src_offset + src_len)
        .ok_or(WindowdError::BufferLengthMismatch)?;
    blend_overlay_row(row, visible_x as usize, source)
}

fn fill_row_rect(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if y < rect_y || y >= rect_y.saturating_add(height) {
        return Ok(());
    }
    let row_pixels = row.len() / 4;
    let start = x.min(row_pixels as u32) as usize;
    let end = x.saturating_add(width).min(row_pixels as u32) as usize;
    for px in start..end {
        let idx = px.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        let alpha = u32::from(bgra[3]);
        if alpha == 255 {
            row[idx..idx + 4].copy_from_slice(&[bgra[0], bgra[1], bgra[2], 0xff]);
            continue;
        }
        if alpha == 0 {
            continue;
        }
        let inv = 255u32.saturating_sub(alpha);
        row[idx] = ((u32::from(bgra[0]) * alpha + u32::from(row[idx]) * inv) / 255) as u8;
        row[idx + 1] = ((u32::from(bgra[1]) * alpha + u32::from(row[idx + 1]) * inv) / 255) as u8;
        row[idx + 2] = ((u32::from(bgra[2]) * alpha + u32::from(row[idx + 2]) * inv) / 255) as u8;
        row[idx + 3] = 0xff;
    }
    Ok(())
}

fn fill_triangle_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    up: bool,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 || y < rect_y || y >= rect_y.saturating_add(height) {
        return Ok(());
    }
    let local_y = y - rect_y;
    let progress = if up {
        height.saturating_sub(local_y + 1)
    } else {
        local_y
    };
    let span = ((progress + 1) * width).max(height) / height.max(1);
    let span = span.max(1).min(width);
    let start = x + (width.saturating_sub(span)) / 2;
    fill_row_rect(y, row, start, y, span, 1, bgra)
}

fn draw_path_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    path: &nexus_layout_types::PathShape,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0
        || height == 0
        || path.points.len() < 2
        || y < rect_y
        || y >= rect_y.saturating_add(height)
    {
        return Ok(());
    }
    for segment in path.points.windows(2) {
        draw_line_segment_row(
            y, row, x, rect_y, width, height, segment[0], segment[1], bgra,
        )?;
    }
    if path.closed {
        draw_line_segment_row(
            y,
            row,
            x,
            rect_y,
            width,
            height,
            *path
                .points
                .last()
                .unwrap_or(&nexus_layout_types::PathPoint::new(0, 0)),
            path.points[0],
            bgra,
        )?;
    }
    Ok(())
}

fn draw_line_segment_row(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    start: nexus_layout_types::PathPoint,
    end: nexus_layout_types::PathPoint,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    let x0 = x + (u32::from(start.x_milli) * width) / 1000;
    let y0 = rect_y + (u32::from(start.y_milli) * height) / 1000;
    let x1 = x + (u32::from(end.x_milli) * width) / 1000;
    let y1 = rect_y + (u32::from(end.y_milli) * height) / 1000;
    let min_y = y0.min(y1);
    let max_y = y0.max(y1);
    if y < min_y || y > max_y {
        return Ok(());
    }
    if y0 == y1 {
        let start_x = x0.min(x1);
        let span = x0.max(x1).saturating_sub(start_x).saturating_add(1);
        return fill_row_rect(y, row, start_x, y, span, 1, bgra);
    }
    let dy = y1 as i64 - y0 as i64;
    let dx = x1 as i64 - x0 as i64;
    let relative = y as i64 - y0 as i64;
    let px = x0 as i64 + dx * relative / dy;
    let px = px.max(0) as u32;
    fill_row_rect(y, row, px.saturating_sub(1), y, 3, 1, bgra)
}

fn stroke_row_rect_width(
    y: u32,
    row: &mut [u8],
    x: u32,
    rect_y: u32,
    width: u32,
    height: u32,
    stroke: u32,
    bgra: [u8; 4],
) -> Result<(), WindowdError> {
    if width == 0 || height == 0 || stroke == 0 {
        return Ok(());
    }
    let stroke = stroke.min(width).min(height);
    fill_row_rect(y, row, x, rect_y, width, stroke, bgra)?;
    fill_row_rect(
        y,
        row,
        x,
        rect_y + height.saturating_sub(stroke),
        width,
        stroke,
        bgra,
    )?;
    fill_row_rect(y, row, x, rect_y, stroke, height, bgra)?;
    fill_row_rect(
        y,
        row,
        x + width.saturating_sub(stroke),
        rect_y,
        stroke,
        height,
        bgra,
    )
}

fn rgba_to_bgra(color: nexus_layout_types::Rgba8) -> [u8; 4] {
    [color.b, color.g, color.r, color.a]
}

fn blend_overlay_row(row: &mut [u8], x: usize, source: &[u8]) -> Result<(), WindowdError> {
    let row_pixels = row.len() / 4;
    for (col, pixel) in source.chunks_exact(4).enumerate() {
        let dst_col = x.saturating_add(col);
        if dst_col >= row_pixels {
            break;
        }
        let alpha = pixel[3];
        if alpha == 0 {
            continue;
        }
        let dst = dst_col
            .checked_mul(4)
            .ok_or(WindowdError::ArithmeticOverflow)?;
        if alpha == 255 {
            row[dst..dst + 4].copy_from_slice(pixel);
            continue;
        }
        let alpha = u32::from(alpha);
        let inv = 255u32.saturating_sub(alpha);
        for channel in 0..3 {
            row[dst + channel] = ((u32::from(pixel[channel]) * alpha
                + u32::from(row[dst + channel]) * inv)
                / 255) as u8;
        }
        row[dst + 3] = 255;
    }
    Ok(())
}

fn blend_cursor_row(
    row: &mut [u8],
    row_y: u32,
    cursor_bitmap: &[u8],
    cursor_width: u32,
    cursor_height: u32,
    cursor_x: i32,
    cursor_y: i32,
) {
    let cursor_row = row_y as i32 - cursor_y;
    if cursor_row < 0 || cursor_row >= cursor_height as i32 {
        return;
    }
    for col in 0..(row.len() / 4) {
        let cursor_col = col as i32 - cursor_x;
        if cursor_col < 0 || cursor_col >= cursor_width as i32 {
            continue;
        }
        let src_idx = ((cursor_row as u32 * cursor_width + cursor_col as u32) * 4) as usize;
        let dst_idx = col * 4;
        if src_idx + 4 > cursor_bitmap.len() {
            continue;
        }
        let alpha = cursor_bitmap[src_idx + 3];
        if alpha == 0 {
            continue;
        }
        if alpha == 255 {
            row[dst_idx..dst_idx + 4].copy_from_slice(&cursor_bitmap[src_idx..src_idx + 4]);
            continue;
        }
        let inv_alpha = 255u32.saturating_sub(u32::from(alpha));
        let alpha = u32::from(alpha);
        for channel in 0..3 {
            row[dst_idx + channel] = ((u32::from(cursor_bitmap[src_idx + channel]) * alpha
                + u32::from(row[dst_idx + channel]) * inv_alpha)
                / 255) as u8;
        }
        row[dst_idx + 3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_backdrop_cache_row, backdrop_cache_slot, build_scale_lut, cursor_damage_rect,
        layer_cache_key, path_cache_slot, path_id_hash, record_layer_cache_row, shadow_cache_key,
        shadow_cache_scale, BackdropCacheEntry, LayerCache, ProofBoxRect, TileMap, TILES_X,
        TILES_Y, TILE_SIZE,
    };
    use crate::live_runtime::{DamageRect, GlassQuality};
    use nexus_layout_types::Rgba8;

    #[test]
    fn scale_lut_is_monotonic_and_clamped() {
        let lut = build_scale_lut(8, 3).expect("lut");
        assert_eq!(lut, vec![0, 0, 0, 1, 1, 1, 2, 2]);
        assert!(lut.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(*lut.last().unwrap_or(&u32::MAX), 2);
    }

    #[test]
    fn backdrop_cache_slot_is_stable_for_same_segment() {
        let a = backdrop_cache_slot(440, 56, 610, GlassQuality::High, 16);
        let b = backdrop_cache_slot(440, 56, 610, GlassQuality::High, 16);
        let c = backdrop_cache_slot(441, 56, 610, GlassQuality::High, 16);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn path_cache_slot_is_stable_for_same_key() {
        let id_hash = path_id_hash("card_hover_glyph");
        let a = path_cache_slot(id_hash, 16, 16, [1, 2, 3, 255], 8);
        let b = path_cache_slot(id_hash, 16, 16, [1, 2, 3, 255], 8);
        let c = path_cache_slot(id_hash, 24, 16, [1, 2, 3, 255], 8);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn shadow_cache_scale_keeps_large_panel_inside_fixed_budget() {
        let scale = shadow_cache_scale(920, 360, 16 * 1024).expect("scaled cache");
        let cache_w = 920u32.div_ceil(u32::from(scale));
        let cache_h = 360u32.div_ceil(u32::from(scale));
        assert!(cache_w as usize * cache_h as usize * 4 <= 16 * 1024);
        assert!(scale > 1);
    }

    #[test]
    fn damage_premerge_merges_only_bounded_area_growth() {
        let mut rects = [
            DamageRect {
                x: 10,
                y: 10,
                width: 20,
                height: 20,
            },
            DamageRect {
                x: 25,
                y: 10,
                width: 20,
                height: 20,
            },
            DamageRect {
                x: 400,
                y: 400,
                width: 20,
                height: 20,
            },
        ];
        let count = super::premerge_damage_rects(&mut rects, 3);
        assert_eq!(count, 2);
        assert!(rects[..count]
            .iter()
            .any(|rect| rect.width == 35 && rect.height == 20));
        assert!(rects[..count]
            .iter()
            .any(|rect| rect.x == 400 && rect.y == 400));
    }

    #[test]
    fn backdrop_cache_preserves_same_segment_key() {
        let mut row = vec![
            0u8, 0, 0, 255, 20, 20, 20, 255, 40, 40, 40, 255, 60, 60, 60, 255,
        ];
        let mut cache = [BackdropCacheEntry::new(); 4];
        let mut scratch = vec![0u8; row.len()];
        apply_backdrop_cache_row(
            &mut row,
            440,
            0,
            4,
            GlassQuality::High,
            &mut cache,
            &mut scratch,
        )
        .expect("first cache fill");
        let cached = row.clone();

        row.fill(255);
        apply_backdrop_cache_row(
            &mut row,
            440,
            0,
            4,
            GlassQuality::High,
            &mut cache,
            &mut scratch,
        )
        .expect("cache hit");
        assert_eq!(row, cached);
    }

    #[test]
    fn tile_map_has_dirty_in_row_range_detects_marked_rows() {
        let mut tm = TileMap::new();
        // Mark a rect covering rows 128..192 (tiles ty=2..=2)
        tm.mark_rect(DamageRect {
            x: 0,
            y: 128,
            width: 1280,
            height: 64,
        });
        assert!(tm.has_dirty_in_row_range(128, 192));
        // Row range outside the marked area should be clean
        assert!(!tm.has_dirty_in_row_range(0, 64));
        assert!(!tm.has_dirty_in_row_range(256, 320));
    }

    #[test]
    fn tile_map_has_dirty_in_row_range_partial_overlap() {
        let mut tm = TileMap::new();
        // Mark tile rows 2..=3 (y=128..256)
        tm.mark_rect(DamageRect {
            x: 0,
            y: 140,
            width: 1280,
            height: 100,
        });
        // Row range that only partially overlaps should still be dirty
        assert!(tm.has_dirty_in_row_range(120, 180));
        assert!(tm.has_dirty_in_row_range(200, 300));
    }

    #[test]
    fn tile_map_clear_resets_all_dirty() {
        let mut tm = TileMap::new();
        tm.mark_rect(DamageRect {
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
        });
        assert!(tm.has_dirty());
        tm.clear();
        assert!(!tm.has_dirty());
        assert!(!tm.has_dirty_in_row_range(0, 800));
    }

    #[test]
    fn cursor_damage_rect_clips_hotspot_and_edges() {
        let rect = cursor_damage_rect(1, 1, 32, 32, 1280, 800).expect("visible cursor");
        assert_eq!(
            rect,
            DamageRect {
                x: 0,
                y: 0,
                width: 31,
                height: 31
            }
        );

        let offscreen = cursor_damage_rect(-80, -80, 32, 32, 1280, 800);
        assert!(offscreen.is_none());
    }

    #[test]
    fn cursor_damage_merge_covers_old_and_new_bounds_once() {
        let old_rect = cursor_damage_rect(100, 100, 32, 32, 1280, 800).expect("old cursor");
        let new_rect = cursor_damage_rect(116, 112, 32, 32, 1280, 800).expect("new cursor");
        assert_eq!(
            old_rect.merge(new_rect),
            DamageRect {
                x: 98,
                y: 98,
                width: 48,
                height: 44
            }
        );
    }

    #[test]
    fn shadow_cache_key_includes_shape_and_effect_params() {
        let color = Rgba8::new(1, 2, 3, 180);
        let base = shadow_cache_key(7, 64, 32, 6, 2, color);
        assert_ne!(base, shadow_cache_key(7, 65, 32, 6, 2, color));
        assert_ne!(base, shadow_cache_key(7, 64, 33, 6, 2, color));
        assert_ne!(base, shadow_cache_key(7, 64, 32, 7, 2, color));
        assert_ne!(base, shadow_cache_key(7, 64, 32, 6, 3, color));
        assert_ne!(
            base,
            shadow_cache_key(7, 64, 32, 6, 2, Rgba8::new(1, 2, 3, 181))
        );
    }

    #[test]
    fn layer_cache_populates_rows_and_serves_clean_layer() {
        let mut cache = LayerCache::default();
        let key = layer_cache_key("proof_panel");
        let rect = ProofBoxRect {
            x: 4,
            y: 10,
            width: 2,
            height: 2,
        };
        let mut row0 = vec![0u8; 8 * 4];
        let mut row1 = vec![0u8; 8 * 4];
        row0[16..24].copy_from_slice(&[1, 2, 3, 255, 4, 5, 6, 255]);
        row1[16..24].copy_from_slice(&[7, 8, 9, 255, 10, 11, 12, 255]);

        record_layer_cache_row(&mut cache, key, rect, 10, &row0, 255, None).expect("row 0 cache");
        assert!(cache.get(key).expect("layer").dirty);

        record_layer_cache_row(&mut cache, key, rect, 11, &row1, 255, None).expect("row 1 cache");
        let layer = cache.get(key).expect("layer");
        assert!(!layer.dirty);
        assert_eq!(
            layer.pixels,
            [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]
        );
    }
}
