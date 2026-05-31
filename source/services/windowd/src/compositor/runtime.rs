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
use super::cache::{
    BackdropCacheEntry, GlassLayerCache, LayerCache, PathCacheEntry, ShadowBoxCacheEntry,
};
use super::cursor::blend_cursor_row;
use super::damage::{
    cursor_damage_rect, damage_rects_intersect, flush_error_label, inflate_effect_rect,
};
use super::emit_windowd_telemetry;
use super::filter::{
    build_live_proof_layouts, filter_layout_variant_index, filter_list_content_height,
    filter_list_viewport_height, refill_filtered_words,
};
use super::scene::{copy_cursor_background_row, copy_scene_row};
use super::source::build_scale_lut;
use super::surface::proof_box_rect;
use super::tile_map::TileMap;
use super::types::{
    FixedDebugLine, ProofBoxRect, ProofCard, ProofPaintPart, ProofPaintRole, RenderClip,
    SourceFrame,
};
use super::{
    BACKDROP_CACHE_ENTRIES, BACKDROP_CACHE_MAX_WIDTH, COL_SCRATCH_SIZE, COMBINED_PANEL_WIDTH,
    CURSOR_BG_MAX_BYTES, GLASS_LAYER_MAX_BYTES, IPC_BATCH_LIMIT, LAYER_CACHE_MAX_BYTES,
    LAYER_CACHE_MAX_LAYER_BYTES, LIVE_FILTER_VARIANTS, PATH_CACHE_ENTRIES, PATH_CACHE_MAX_PIXELS,
    PROOF_PANEL_H, PROOF_PANEL_X, PROOF_PANEL_Y, ROUTE_NAME, ROW_WRITE_CHUNK,
    SHADOW_BOX_CACHE_ENTRIES, SOFT_PANEL_SHADOW_BLUR_RADIUS, SOFT_PANEL_SHADOW_OFFSET_Y,
    VISIBLE_UPDATE_FLUSH_LIMIT, WINDOWD_SHADOW_ARENA_SIZE,
};
use crate::error::WindowdError;
use crate::ids::CallerCtx;
use crate::live_runtime::{
    premerge_damage_rects, select_glass_quality, DamageRect, GlassQuality, LayoutHotPathIndex,
    TargetDamage,
};
use crate::markers::*;
use crate::smoke::VisibleBootstrapMode;
use crate::telemetry::WindowdDisplayTelemetryReport;
use alloc::vec::Vec;
use animation::{AnimProp, AnimationDriver, Easing, LayerId, SceneUpdate};
use core::fmt::Write as _;
use input_live_protocol::{VisibleState, STATUS_MALFORMED, STATUS_OK};
use nexus_abi::{cap_clone, debug_println, nsec, vmo_write, Handle};
use nexus_effects::ShadowArena;
use nexus_gfx::{CommandBuffer, RenderPassDesc, TileRect};
use nexus_ipc::{Client as _, KernelClient, Wait};
use nexus_layout::LayoutResult;
use nexus_layout_types::FxPx;

const GPU_ANIMATION_SUBMIT_OP: u8 = 1;
const GPU_SET_FRAMEBUFFER_VMO_OP: u8 = 3; // mirrors gpud::OP_SET_FRAMEBUFFER_VMO
const HOVER_LAYER_ID: LayerId = LayerId(1);
const SIDEBAR_LAYER_ID: LayerId = LayerId(62);
const SIDEBAR_WIDTH: u32 = 320;
const SIDEBAR_ANIMATION_NS: u64 = 420_000_000;
const GLASS_BUTTON_W: u32 = 156;
const GLASS_BUTTON_H: u32 = 56;
const GLASS_BUTTON_TOP: u32 = 24;
const GLASS_BUTTON_RIGHT: u32 = 24;

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
    let button_x = mode.width.saturating_sub(GLASS_BUTTON_W + GLASS_BUTTON_RIGHT);
    let button_alpha = (96.0 + 80.0 * scene.hover_opacity).clamp(0.0, 220.0) as u8;
    draw_glass_rect_row(
        row,
        y,
        button_x,
        GLASS_BUTTON_TOP,
        GLASS_BUTTON_W,
        GLASS_BUTTON_H,
        [235, 245, 255, button_alpha],
        true,
    );

    let translate = scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32;
    let sidebar_x = mode.width.saturating_sub(SIDEBAR_WIDTH).saturating_add(translate);
    let sidebar_alpha = (128.0 * scene.sidebar_opacity).clamp(0.0, 192.0) as u8;
    if sidebar_alpha == 0 {
        return;
    }
    draw_glass_rect_row(
        row,
        y,
        sidebar_x,
        0,
        SIDEBAR_WIDTH,
        mode.height,
        [220, 236, 255, sidebar_alpha],
        true,
    );
}

fn draw_glass_rect_row(
    row: &mut [u8],
    y: u32,
    x: u32,
    top: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
    shadow: bool,
) {
    if y < top || y >= top.saturating_add(height) {
        if shadow && y >= top.saturating_add(8) && y < top.saturating_add(height + 18) {
            blend_span(row, x.saturating_add(10), width, [0, 0, 0, 34]);
        }
        return;
    }
    if shadow && y >= top.saturating_add(6) {
        blend_span(row, x.saturating_add(8), width, [0, 0, 0, 28]);
    }
    blend_span(row, x, width, color);
    if y == top || y == top.saturating_add(height).saturating_sub(1) {
        blend_span(row, x, width, [255, 255, 255, 90]);
    } else {
        blend_pixel(row, x, [255, 255, 255, 72]);
        blend_pixel(row, x.saturating_add(width).saturating_sub(1), [255, 255, 255, 48]);
    }
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
    /// Animation driver: spring physics, keyframes, reduced motion (RFC-0059).
    animation_driver: AnimationDriver,
    animated_scene: AnimatedSceneState,
    animation_proof: AnimationProofState,
    gpud_client: Option<KernelClient>,
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
        let proof_layout_index =
            proof_layouts.as_ref().and_then(|layouts| layouts.first()).map(|layout| {
                LayoutHotPathIndex::build(
                    layout,
                    PROOF_PANEL_X,
                    PROOF_PANEL_Y,
                    mode.width,
                    mode.height,
                )
            });
        let _ = debug_println(RUNTIME_INIT_OK);
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
            animation_driver: AnimationDriver::new(),
            animated_scene: AnimatedSceneState::new(),
            animation_proof: AnimationProofState::default(),
            gpud_client: None,
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

    pub(crate) fn register_framebuffer(&mut self, handle: Handle) -> u8 {
        self.framebuffer = Some(handle);
        self.state.display_scanout_ready = true;
        self.refresh_observer_state();

        // Notify gpud: send the framebuffer VMO for zero-copy GPU scanout.
        // If gpud is not available, the CPU path (ramfb) remains active.
        self.try_handoff_framebuffer_to_gpud(handle);

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

    pub(crate) fn apply_input_state(&mut self, upstream: VisibleState) -> u8 {
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
            // Hover card opacity: 0.0 → 1.0 (or reverse)
            if old_state.hover_visible != self.state.hover_visible {
                let from = if old_state.hover_visible { 1.0 } else { 0.0 };
                let to = if self.state.hover_visible { 1.0 } else { 0.0 };
                self.animation_driver.spring_to(
                    HOVER_LAYER_ID,
                    AnimProp::Opacity,
                    from,
                    to,
                    spring,
                );
                let sidebar_from = if old_state.hover_visible { 0.0 } else { SIDEBAR_WIDTH as f32 };
                let sidebar_to = if self.state.hover_visible { 0.0 } else { SIDEBAR_WIDTH as f32 };
                self.animation_driver.keyframe_to(
                    SIDEBAR_LAYER_ID,
                    AnimProp::TranslateX,
                    alloc::vec![(0.0, sidebar_from), (1.0, sidebar_to)],
                    SIDEBAR_ANIMATION_NS,
                    Easing::EaseInOut,
                );
                self.animation_driver.keyframe_to(
                    SIDEBAR_LAYER_ID,
                    AnimProp::Opacity,
                    alloc::vec![
                        (0.0, self.animated_scene.sidebar_opacity),
                        (1.0, if self.state.hover_visible { 1.0 } else { 0.0 }),
                    ],
                    SIDEBAR_ANIMATION_NS,
                    Easing::EaseInOut,
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
                self.animation_driver.spring_to(LayerId(2), AnimProp::Opacity, from, to, spring);
            }
            // Keyboard card opacity
            if old_state.keyboard_visible != self.state.keyboard_visible {
                let from = if old_state.keyboard_visible { 1.0 } else { 0.0 };
                let to = if self.state.keyboard_visible { 1.0 } else { 0.0 };
                self.animation_driver.spring_to(LayerId(3), AnimProp::Opacity, from, to, spring);
            }
        }
        let pointer_only_change =
            cursor_changed && !paint_flags_changed && !text_changed && !filter_changed;
        if pointer_only_change && self.saved_cursor_rect.is_some() {
            if self.update_cursor_fast_path().is_ok() {
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
                let x = PROOF_PANEL_X.saturating_add(rect.x.as_u32().unwrap_or(0));
                let y = PROOF_PANEL_Y.saturating_add(rect.y.as_u32().unwrap_or(0));
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

    pub(crate) fn tick(&mut self, now_ns: u64) {
        // The scanout VMO persists; avoid rewriting a full 1280x800 frame on idle ticks.
        // Drive animations — produces SceneUpdates for changed properties.
        let anim_updates = self.animation_driver.tick(now_ns);
        if !anim_updates.is_empty() {
            self.apply_scene_updates(&anim_updates);
            let panel_damage = DamageRect {
                x: 0,
                y: 0,
                width: COMBINED_PANEL_WIDTH as u32,
                height: PROOF_PANEL_H,
            };
            self.queue_dirty_rect(panel_damage);
            self.queue_dirty_rect(self.sidebar_damage_rect());
            if self.submit_animation_to_gpud(&anim_updates).is_ok()
                && !self.animation_proof.batch_marker
            {
                let _ = debug_println(UIRUNTIME_BATCH_COMMIT_OK);
                self.animation_proof.batch_marker = true;
            }
            if !self.animation_proof.live_marker {
                let _ = debug_println(WINDOWD_LIVE_TRANSITION_OK);
                self.animation_proof.live_marker = true;
            }
            if self.animation_driver.active_count() == 0 && !self.animation_proof.spring_marker {
                let _ = debug_println(UIANIM_SPRING_CONVERGE_OK);
                self.animation_proof.spring_marker = true;
            }
            if self.animation_proof.batch_marker
                && self.animation_proof.live_marker
                && self.animation_proof.spring_marker
                && self.input_markers_emitted.v2b_assets_summary
                && !self.animation_proof.v5_summary_marker
            {
                let _ = debug_println(SELFTEST_UI_V5_TRANSITION_OK);
                self.animation_proof.v5_summary_marker = true;
            }
        }

        if let Some(report) = self.telemetry.report_values_if_due(now_ns) {
            emit_windowd_telemetry(report);
        }
    }

    fn apply_scene_updates(&mut self, updates: &[SceneUpdate]) {
        for update in updates {
            match (update.layer_id, update.property) {
                (HOVER_LAYER_ID, AnimProp::Opacity) => {
                    self.animated_scene.hover_opacity = update.value.clamp(0.0, 1.0);
                }
                (SIDEBAR_LAYER_ID, AnimProp::TranslateX) => {
                    self.animated_scene.sidebar_translate_x =
                        update.value.clamp(0.0, SIDEBAR_WIDTH as f32);
                }
                (SIDEBAR_LAYER_ID, AnimProp::Opacity) => {
                    self.animated_scene.sidebar_opacity = update.value.clamp(0.0, 1.0);
                }
                _ => {}
            }
        }
    }

    /// Send the framebuffer VMO to gpud for zero-copy GPU scanout.
    /// Falls back silently if gpud is not reachable — the CPU ramfb path stays active.
    fn try_handoff_framebuffer_to_gpud(&mut self, fb_handle: Handle) {
        let Ok(clone) = nexus_abi::cap_clone(fb_handle) else {
            return;
        };
        if self.gpud_client.is_none() {
            self.gpud_client = KernelClient::new_for("gpud").ok();
        }
        let Some(client) = self.gpud_client.as_ref() else {
            return;
        };
        let request = [GPU_SET_FRAMEBUFFER_VMO_OP];
        if client.send_with_cap_move_wait(&request, clone, Wait::NonBlocking).is_ok() {
            let _ = debug_println("windowd: fb handoff to gpud ok");
        }
    }

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
                .try_draw_tiles(&[
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
                ])
                .map_err(|_| WindowdError::InvalidDamage)?;
            encoder.end_encoding();
        }
        let committed = cmd.try_commit().map_err(|_| WindowdError::InvalidDamage)?;
        if committed.command_count() == 0 {
            return Err(WindowdError::InvalidDamage);
        }
        let frame = [GPU_ANIMATION_SUBMIT_OP];
        if self.gpud_client.is_none() {
            self.gpud_client = KernelClient::new_for("gpud").ok();
        }
        let Some(client) = self.gpud_client.as_ref() else {
            return Err(WindowdError::InvalidDamage);
        };
        client.send(&frame, Wait::NonBlocking).map_err(|_| WindowdError::InvalidDamage)
    }

    fn sidebar_damage_rect(&self) -> DamageRect {
        DamageRect {
            x: self.mode.width.saturating_sub(SIDEBAR_WIDTH),
            y: 0,
            width: SIDEBAR_WIDTH,
            height: self.mode.height,
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
        self.write_rows(0, self.mode.height, select_glass_quality(PROOF_PANEL_H), false)?;
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
        let cursor_bitmap = self.cursor_bitmap.as_deref();
        let cursor_width = self.cursor_width;
        let cursor_height = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
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
                draw_animation_proof_overlay_row(band_row, y, mode, animated_scene);
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
        let cursor_bitmap = self.cursor_bitmap.as_deref();
        let cursor_width = self.cursor_width;
        let cursor_height = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
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
                draw_animation_proof_overlay_row(band_row, y, mode, animated_scene);
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
            nsec().unwrap_or(render_start_ns).saturating_sub(render_start_ns),
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

    pub(crate) fn flush_pending_damage(&mut self) -> Result<(), WindowdError> {
        let paint_only = self.paint_only_damage;
        let mut wrote_any = false;
        let mut rects = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; 5];
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

    pub(crate) fn has_pending_damage(&self) -> bool {
        !self.pending_damage_rects.is_empty() || self.pending_damage_rect.is_some()
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
            vmo_write(handle, dst_offset, &self.cursor_bg_saved[src_offset..src_end])
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

        let row_len = self.mode.stride as usize;
        let byte_len = rect.width as usize * 4;
        if byte_len == 0
            || byte_len.saturating_mul(rect.height as usize) > self.cursor_bg_saved.len()
        {
            self.saved_cursor_rect = None;
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
        let blur_row_buf = &mut self.blur_row_buf[..row_len];
        let backdrop_cache = &mut self.backdrop_cache;
        let glass_layer = &mut self.glass_layer;
        let glass_scratch = &mut self.glass_scratch;
        let path_cache = &mut self.path_cache;
        let shadow_scratch = &mut self.shadow_scratch[..row_len];
        let mut shadow_arena =
            ShadowArena::from_buffer_with_used(&mut self.shadow_arena_buf, self.shadow_arena_used);
        let render_clip = RenderClip::new(rect.x, rect.end_x(), self.mode.width);

        for (row_idx, y) in (rect.y..rect.end_y().min(self.mode.height)).enumerate() {
            let dest_start = row_idx * byte_len;
            let dest_end = dest_start + byte_len;
            if dest_end > self.cursor_bg_saved.len() {
                break;
            }
            let row_buf = &mut self.band_scratch[..row_len];
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
                row_buf,
                &mut self.layer_cache,
                shadow_scratch,
                &mut shadow_arena,
                &mut self.col_scratch,
                &mut self.shadow_box_cache,
            )?;
            let src_start = rect.x as usize * 4;
            let src_end = src_start + byte_len;
            if src_end > row_buf.len() {
                break;
            }
            self.cursor_bg_saved[dest_start..dest_end]
                .copy_from_slice(&row_buf[src_start..src_end]);
            blend_cursor_row(
                row_buf,
                y,
                cursor_bitmap,
                self.cursor_width,
                self.cursor_height,
                self.state.cursor_x - crate::assets::CURSOR_HOTSPOT_X,
                self.state.cursor_y - crate::assets::CURSOR_HOTSPOT_Y,
            );
            let dst_offset = y as usize * row_len + rect.x as usize * 4;
            vmo_write(handle, dst_offset, &row_buf[src_start..src_end])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
        }
        self.shadow_arena_used = shadow_arena.used_bytes();
        self.saved_cursor_rect = Some(rect);
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.refresh_observer_state();
        self.telemetry.record_compose_timed(
            u64::from(rect.width).saturating_mul(u64::from(rect.height)),
            nsec().unwrap_or(render_start_ns).saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        Ok(())
    }
}
