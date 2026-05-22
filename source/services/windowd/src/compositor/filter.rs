// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Filter word-list rendering: text input glyphs, scrollable word list,
//! scrollbar, layout builders.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use alloc::vec::Vec;
use crate::assets;
use crate::error::WindowdError;
use crate::layout_panel;
use input_live_protocol::VisibleState;
use nexus_layout::LayoutResult;
use nexus_layout_types::FxPx;
use super::types::ProofBoxRect;
use super::font::bitmap_font_5x7;
use super::primitives::{blend_asset_row_clipped, fill_row_rect, rgba_to_bgra};
use super::{FILTER_INPUT_FONT_W, FILTER_INPUT_FONT_H, FILTER_INPUT_FONT_SCALE, FILTER_INPUT_FONT_ADVANCE, FILTER_INPUT_PADDING_X, FILTER_LIST_PADDING_X, FILTER_LIST_PADDING_Y, FILTER_LIST_ROW_GAP, LIVE_FILTER_VARIANTS};

pub(crate) fn refill_filtered_words(out: &mut Vec<&'static str>, filter_text: &str) {
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
pub(crate) fn filter_list_content_height(filtered_words: &[&'static str]) -> u32 {
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

pub(crate) fn filter_list_viewport_height(list_height: u32) -> u32 {
    list_height.saturating_sub(FILTER_LIST_PADDING_Y.saturating_mul(2))
}

fn filter_list_viewport_width(list_width: u32) -> u32 {
    list_width
        .saturating_sub(FILTER_LIST_PADDING_X.saturating_mul(2))
        .saturating_sub(layout_panel::FILTER_SCROLLBAR_GUTTER)
}

pub(crate) fn draw_filter_word_list_row(
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

pub(crate) fn draw_filter_input_text_row(
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

pub(crate) fn filter_layout_variant_index(filter_text: &str) -> usize {
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

pub(crate) fn build_live_proof_layouts(state: VisibleState) -> Option<Vec<LayoutResult>> {
    let mut layouts = Vec::with_capacity(LIVE_FILTER_VARIANTS.len());
    for filter_text in LIVE_FILTER_VARIANTS {
        layouts.push(layout_panel::compute_proof_layout(state, filter_text).ok()?);
    }
    Some(layouts)
}
