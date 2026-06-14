// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Scrollable chat panel rendering for the CPU retained-plane path.
//! Mirrors the filter word-list pattern (`filter.rs`): a per-scanline,
//! zero-allocation renderer that paints the chat message list into Plane 1 at
//! the chat region. The message data comes from `nexus_virtual_list`'s
//! `ChatMessageProvider`; the visible window + per-message screen positions are
//! precomputed by the runtime on scroll into a reusable buffer, so this row
//! function never allocates and never re-walks the full 500-message collection.
//!
//! Wrapping is hard-wrap (the ASCII message pool has no embedded newlines), and
//! the wrap math lives in `crate::interaction` (host-tested) so layout and paint
//! can never drift. This panel is purely additive at the right-hand chat region
//! and does not touch the left proof panel, button, or sidebar.
//!
//! OWNERS: @ui
//! STATUS: TASK-0063 — UI v5b chat
//! API_STABILITY: Unstable

use super::font::bitmap_font_5x7;
use super::primitives::fill_row_rect;
use crate::error::WindowdError;
use crate::interaction::{
    chat_chars_per_line, chat_line_char_range, chat_message_height, chat_message_lines,
    CHAT_FONT_ADVANCE, CHAT_FONT_H, CHAT_FONT_SCALE, CHAT_FONT_W, CHAT_LINE_H, CHAT_MSG_PAD,
    CHAT_CLOSE_ZONE_W, CHAT_PAD, CHAT_PANEL_H, CHAT_PANEL_W, CHAT_SCROLLBAR_W,
    CHAT_TITLE_BAR_H,
};
use alloc::vec::Vec;
use nexus_virtual_list::{ChatMessageProvider, ItemProvider};

// Colours (BGRA — the framebuffer is BGRA8888).
// PANEL_BG is translucent so the compositor's backdrop blur shows through as
// glass; message bubbles stay opaque for readability (iMessage/Telegram look).
const PANEL_BG: [u8; 4] = [44, 38, 38, 168];
const BUBBLE_INCOMING: [u8; 4] = [70, 64, 60, 255];
const BUBBLE_FROM_ME: [u8; 4] = [180, 96, 44, 255];
const TEXT_COLOR: [u8; 4] = [245, 245, 240, 255];
const SCROLL_TRACK: [u8; 4] = [34, 30, 30, 200];
const SCROLL_THUMB: [u8; 4] = [130, 122, 120, 230];
/// Gap between a message bubble and the next, plus the bubble inset from the
/// viewport edge so consecutive bubbles read as separate.
const BUBBLE_INSET: u32 = 4;

/// A message in the current scroll window, positioned in *surface-local* pixels
/// (top-left of the chat surface is 0,0). The surface is composited to its
/// on-screen position by a single BlitAbsolute, so the content is independent of
/// where the window currently sits — moving the window costs no re-render.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChatVisibleMsg {
    pub(crate) text: &'static str,
    pub(crate) from_me: bool,
    /// Top of the message block in surface-local pixels (may be negative when the
    /// message is partially scrolled above the viewport).
    pub(crate) top: i32,
    pub(crate) lines: u32,
    /// Character count (NOT byte length) — the message pool contains multi-byte
    /// UTF-8 (em-dashes), so wrapping is by char and slicing is by char boundary.
    pub(crate) char_len: usize,
}

/// Rebuild the visible-window buffer for a given `scroll_y` and return the total
/// content height. Walks the full collection once (cheap; called only on scroll),
/// pushing only the messages that intersect the viewport. Zero alloc after the
/// `Vec` reaches steady capacity (it is cleared, not reallocated).
pub(crate) fn compute_visible(
    provider: &ChatMessageProvider,
    scroll_y: u32,
    out: &mut Vec<ChatVisibleMsg>,
) -> u32 {
    out.clear();
    let (_, vp_y, _, vp_h) = viewport();
    let cpl = chat_chars_per_line();
    let view_bottom = scroll_y.saturating_add(vp_h);
    let mut block_top = 0u32;
    let len = provider.len();
    for opt in provider.get(0..len) {
        let Some(msg) = opt else {
            continue;
        };
        let char_len = msg.text.chars().count();
        let lines = chat_message_lines(char_len, cpl);
        let height = chat_message_height(lines);
        let block_bottom = block_top.saturating_add(height);
        if block_bottom > scroll_y && block_top < view_bottom {
            let top = vp_y as i32 + block_top as i32 - scroll_y as i32;
            out.push(ChatVisibleMsg {
                text: msg.text,
                from_me: msg.from_me,
                top,
                lines,
                char_len,
            });
        }
        block_top = block_bottom;
    }
    block_top
}

// Title bar colours (BGRA): subtle vertical gradient, separator, close X.
const TITLE_BAR_TOP: [u8; 4] = [72, 64, 62, 255];
const TITLE_BAR_BOTTOM: [u8; 4] = [52, 46, 45, 255];
const TITLE_SEPARATOR: [u8; 4] = [30, 27, 27, 255];
const TITLE_TEXT: [u8; 4] = [240, 238, 235, 255];

/// Surface-local viewport rect (x, y, w, h). The chat surface top-left is (0,0);
/// the on-screen position is applied only at composite time. Content starts
/// below the title bar.
#[inline]
fn viewport() -> (u32, u32, u32, u32) {
    let vp_x = CHAT_PAD;
    let vp_y = CHAT_TITLE_BAR_H.saturating_add(CHAT_PAD);
    let vp_w = CHAT_PANEL_W
        .saturating_sub(CHAT_PAD.saturating_mul(2))
        .saturating_sub(CHAT_SCROLLBAR_W);
    let vp_h = CHAT_PANEL_H
        .saturating_sub(CHAT_TITLE_BAR_H)
        .saturating_sub(CHAT_PAD.saturating_mul(2));
    (vp_x, vp_y, vp_w, vp_h)
}

/// Render one row of the title bar: gradient background, bottom separator,
/// "CHAT" label on the left, and an X glyph centered in the close zone.
fn draw_title_bar_row(ly: u32, row: &mut [u8]) -> Result<(), WindowdError> {
    // Vertical gradient (per-row lerp top → bottom).
    let denom = (CHAT_TITLE_BAR_H - 1).max(1);
    let mut bg = [0u8; 4];
    for c in 0..4 {
        let t = TITLE_BAR_TOP[c] as u32;
        let b = TITLE_BAR_BOTTOM[c] as u32;
        bg[c] = ((t * (denom - ly) + b * ly) / denom) as u8;
    }
    if ly == CHAT_TITLE_BAR_H - 1 {
        bg = TITLE_SEPARATOR;
    }
    fill_row_rect(ly, row, 0, 0, CHAT_PANEL_W, CHAT_TITLE_BAR_H, bg)?;

    // Title label "CHAT" (5x7 bitmap font, 2x scale) vertically centered.
    const TITLE: &str = "CHAT";
    const TITLE_SCALE: u32 = 2;
    let glyph_h = 7 * TITLE_SCALE;
    let title_top = (CHAT_TITLE_BAR_H - glyph_h) / 2;
    if ly >= title_top && ly < title_top + glyph_h {
        let glyph_row = ((ly - title_top) / TITLE_SCALE) as usize;
        let mut pen_x = CHAT_PAD;
        for ch in TITLE.chars() {
            let glyph = bitmap_font_5x7(ch);
            let bits = glyph[glyph_row];
            for col in 0..5u32 {
                if bits & (1 << (4 - col)) != 0 {
                    let px = pen_x + col * TITLE_SCALE;
                    fill_row_rect(ly, row, px, ly, TITLE_SCALE, 1, TITLE_TEXT)?;
                }
            }
            pen_x += (5 + 1) * TITLE_SCALE;
        }
    }

    // Close X: two 2px diagonals in a 14x14 box centered in the close zone.
    const X_SIZE: u32 = 14;
    let zone_x = CHAT_PANEL_W - CHAT_CLOSE_ZONE_W;
    let x0 = zone_x + (CHAT_CLOSE_ZONE_W - X_SIZE) / 2;
    let y0 = (CHAT_TITLE_BAR_H - X_SIZE) / 2;
    if ly >= y0 && ly < y0 + X_SIZE {
        let dy = ly - y0;
        // Down-right diagonal and down-left diagonal, 2px thick.
        for (dx, w) in [(dy, 2u32), (X_SIZE - 1 - dy, 2u32)] {
            let px = x0 + dx.min(X_SIZE - 2);
            fill_row_rect(ly, row, px, ly, w, 1, TITLE_TEXT)?;
        }
    }
    Ok(())
}

/// Render one *surface-local* row `ly` (0..CHAT_PANEL_H) of the chat panel into
/// `row` (pixels written at local x 0..CHAT_PANEL_W). The caller blits the
/// finished surface to its on-screen position. No-op for `ly` outside the panel.
pub(crate) fn draw_chat_panel_row(
    ly: u32,
    row: &mut [u8],
    scroll_y: u32,
    content_h: u32,
    visible: &[ChatVisibleMsg],
) -> Result<(), WindowdError> {
    if ly >= CHAT_PANEL_H {
        return Ok(());
    }
    // Title bar rows render their own background + chrome.
    if ly < CHAT_TITLE_BAR_H {
        return draw_title_bar_row(ly, row);
    }
    // Panel background (full panel width, every row) — opaque, so no glass blur.
    fill_row_rect(ly, row, 0, 0, CHAT_PANEL_W, CHAT_PANEL_H, PANEL_BG)?;

    let (vp_x, vp_y, vp_w, vp_h) = viewport();
    let vp_bottom = vp_y.saturating_add(vp_h);
    if ly < vp_y || ly >= vp_bottom {
        return Ok(());
    }
    let yi = ly as i32;
    let cpl = chat_chars_per_line();

    for m in visible {
        let height = chat_message_height(m.lines);
        // Bubble background, clipped to the viewport vertically.
        let bub_top = m.top.max(vp_y as i32);
        let bub_bottom = (m.top + height as i32).min(vp_bottom as i32);
        if bub_bottom > bub_top && yi >= bub_top && yi < bub_bottom {
            let bubble_bg = if m.from_me { BUBBLE_FROM_ME } else { BUBBLE_INCOMING };
            fill_row_rect(
                ly,
                row,
                vp_x.saturating_add(BUBBLE_INSET),
                bub_top as u32,
                vp_w.saturating_sub(BUBBLE_INSET.saturating_mul(2)),
                (bub_bottom - bub_top) as u32,
                bubble_bg,
            )?;
        }
        // Text lines.
        let text_top = m.top + CHAT_MSG_PAD as i32;
        let text_bottom = text_top + (m.lines.saturating_mul(CHAT_LINE_H)) as i32;
        if yi >= text_top && yi < text_bottom {
            let rel = (yi - text_top) as u32;
            let line_idx = rel / CHAT_LINE_H;
            let glyph_row = (rel % CHAT_LINE_H) / CHAT_FONT_SCALE;
            if glyph_row < CHAT_FONT_H {
                if let Some((cs, ce)) = chat_line_char_range(m.char_len, cpl, line_idx) {
                    let text_x = vp_x.saturating_add(BUBBLE_INSET).saturating_add(6);
                    let clip_end = vp_x.saturating_add(vp_w);
                    draw_text_line_row(
                        ly,
                        row,
                        m.text,
                        cs,
                        ce,
                        text_x,
                        clip_end,
                        glyph_row as usize,
                    )?;
                }
            }
        }
    }

    if content_h > vp_h {
        draw_scrollbar_row(ly, row, vp_y, vp_h, scroll_y, content_h)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_text_line_row(
    y: u32,
    row: &mut [u8],
    text: &str,
    char_start: usize,
    char_end: usize,
    start_x: u32,
    clip_end_x: u32,
    glyph_row: usize,
) -> Result<(), WindowdError> {
    let mut pen_x = start_x;
    // Walk by characters (boundary-safe for multi-byte UTF-8 like em-dashes).
    for ch in text
        .chars()
        .skip(char_start)
        .take(char_end.saturating_sub(char_start))
    {
        if pen_x.saturating_add(CHAT_FONT_W * CHAT_FONT_SCALE) > clip_end_x {
            break;
        }
        let bits = bitmap_font_5x7(ch)[glyph_row.min(CHAT_FONT_H as usize - 1)];
        for col in 0..CHAT_FONT_W {
            if bits & (1 << (CHAT_FONT_W - 1 - col)) == 0 {
                continue;
            }
            fill_row_rect(
                y,
                row,
                pen_x + col * CHAT_FONT_SCALE,
                y,
                CHAT_FONT_SCALE,
                1,
                TEXT_COLOR,
            )?;
        }
        pen_x = pen_x.saturating_add(CHAT_FONT_ADVANCE);
    }
    Ok(())
}

/// Thumb geometry `(thumb_y, thumb_h)` in surface-local pixels for a scroll
/// offset.
fn scrollbar_thumb_span(scroll_y: u32, content_h: u32) -> (u32, u32) {
    let vp_y = CHAT_PAD;
    let vp_h = CHAT_PANEL_H.saturating_sub(CHAT_PAD.saturating_mul(2));
    let max_scroll = content_h.saturating_sub(vp_h).max(1);
    let thumb_h = (vp_h.saturating_mul(vp_h) / content_h.max(1)).clamp(24, vp_h);
    let travel = vp_h.saturating_sub(thumb_h);
    let thumb_y = vp_y.saturating_add(scroll_y.min(max_scroll).saturating_mul(travel) / max_scroll);
    (thumb_y, thumb_h)
}

fn draw_scrollbar_row(
    ly: u32,
    row: &mut [u8],
    vp_y: u32,
    vp_h: u32,
    scroll_y: u32,
    content_h: u32,
) -> Result<(), WindowdError> {
    // Surface-local scrollbar column at the right edge of the panel.
    let track_x = CHAT_PANEL_W
        .saturating_sub(CHAT_PAD)
        .saturating_sub(CHAT_SCROLLBAR_W);
    if ly >= vp_y && ly < vp_y.saturating_add(vp_h) {
        fill_row_rect(ly, row, track_x, vp_y, CHAT_SCROLLBAR_W, vp_h, SCROLL_TRACK)?;
    }
    let (thumb_y, thumb_h) = scrollbar_thumb_span(scroll_y, content_h);
    if ly >= thumb_y && ly < thumb_y.saturating_add(thumb_h) {
        fill_row_rect(ly, row, track_x, thumb_y, CHAT_SCROLLBAR_W, thumb_h, SCROLL_THUMB)?;
    }
    Ok(())
}
