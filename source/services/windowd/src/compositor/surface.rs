// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Proof-surface and layout-box rendering: row-level compositing with paint roles,
//! layer cache, backdrop integration, and border/fill logic.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use super::backdrop::{
    apply_backdrop_cache_row, blur_backdrop_segment, draw_combined_panel_glass_row,
    saturate_bgra_segment,
};
use super::cache::{BackdropCacheEntry, GlassLayerCache, Layer, LayerCache, PathCacheEntry};
use super::filter::{draw_filter_input_text_row, draw_filter_word_list_row};
use super::path_cache::blend_cached_path_row;
use super::primitives::{
    blend_asset_row, draw_path_row, fill_row_rect, fill_triangle_row, rgba_to_bgra,
    stroke_row_rect_width,
};
use super::sdf::{
    fill_sdf_circle_row, fill_sdf_rounded_rect_row, stroke_sdf_circle_row,
    stroke_sdf_rounded_rect_row,
};
use super::types::{
    ProofBoxRect, ProofCard, ProofPaintPart, ProofPaintRole, RenderClip, SourceFrame,
};
use super::{
    BACKDROP_CACHE_MAX_WIDTH, DARK_GLASS_BLUR_RADIUS, DARK_GLASS_BORDER, DARK_GLASS_RADIUS,
    DARK_GLASS_SATURATION_PERCENT, DARK_GLASS_TINT, FILTER_INPUT_FONT_ADVANCE, FILTER_INPUT_FONT_H,
    FILTER_INPUT_FONT_SCALE, FILTER_INPUT_FONT_W, FILTER_INPUT_PADDING_X, FILTER_LIST_PADDING_X,
    FILTER_LIST_PADDING_Y, FILTER_LIST_ROW_GAP, GLASS_LAYER_MAX_BYTES, GLASS_LAYER_MAX_HEIGHT,
    GLASS_LAYER_MAX_WIDTH, GLASS_LAYER_SCALE, LAYER_CACHE_MAX_BYTES, LAYER_CACHE_MAX_LAYER_BYTES,
    PROOF_PANEL_X, PROOF_PANEL_Y, SOFT_PANEL_SHADOW_BLUR_RADIUS, SOFT_PANEL_SHADOW_OFFSET_Y,
};
use crate::assets;
use crate::error::WindowdError;
use crate::fixed_sdf;
use crate::live_runtime::{DamageRect, GlassQuality, LayoutHotPathIndex};
use crate::smoke::VisibleBootstrapMode;
use alloc::vec::Vec;
use input_live_protocol::VisibleState;
use nexus_layout::LayoutResult;
use nexus_layout_types::{FxPx, Rgba8};

pub(crate) fn draw_proof_surface_row(
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
    let cache_static_layer = cache_key.is_some()
        && paint_role.is_none()
        && opacity_alpha >= 255
        && static_layer_has_cacheable_paint(layout_box)
        && layout_box.visual.shadow.is_none()
        && layout_box.id.is_some_and(static_layer_cacheable_id);
    // Phase A1: CPU blur removed — GPU BlurBackdrop handles all glass effects.

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

pub(crate) fn layer_cache_key(id: &str) -> u64 {
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
        id if id.starts_with("filter_") => (Filter, FilterWord),
        _ => return None,
    };
    Some(ProofPaintRole { card, part })
}

pub(crate) fn record_layer_cache_row(
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

pub(crate) fn proof_box_rect(layout_box: &nexus_layout::LayoutBox) -> Option<ProofBoxRect> {
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
