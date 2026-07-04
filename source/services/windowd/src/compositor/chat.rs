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

use super::primitives::fill_row_rect;
use crate::error::WindowdError;
use crate::interaction::{
    chat_message_height, chat_wrap_line_range, chat_wrap_lines, CHAT_CLOSE_ZONE_W, CHAT_LINE_H,
    CHAT_MSG_PAD, CHAT_PAD, CHAT_PANEL_H, CHAT_PANEL_W, CHAT_SCROLLBAR_W, CHAT_TEXT_INSET,
    CHAT_TITLE_BAR_H,
};
use crate::text::{draw_text_row, FontSize};
use alloc::vec::Vec;
use chat_app::ChatMessageProvider;
use nexus_virtual_list::ItemProvider;

// Colours (BGRA — the framebuffer is BGRA8888).
// The body tint, bubble fill, and text now come from the active theme snapshot
// (TASK-0072 Phase 9); `PANEL_BG[3]` remains the SSOT for the frosted-glass body
// alpha (the theme swaps only the RGB per mode). Only our own messages get an
// accent bubble; incoming messages render directly on the glass body.
const PANEL_BG: [u8; 4] = [40, 34, 30, 150];
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
    /// Index of this message's first wrapped-line char range in the shared
    /// `ranges` buffer (its `lines` consecutive entries belong to it).
    pub(crate) range_start: usize,
}

/// Precompute the wrapped line count of every message ONCE (the wrap width is
/// fixed — the text column stays at wrap width regardless of window size).
/// `compute_visible_window` then costs O(1) per message instead of re-walking
/// every message's text on every scroll re-window.
pub(crate) fn build_lines_cache(provider: &ChatMessageProvider, out: &mut Vec<u32>) {
    out.clear();
    let len = provider.len();
    for opt in provider.get(0..len) {
        out.push(opt.map(|m| chat_wrap_lines(m.text)).unwrap_or(1));
    }
}

/// Rebuild the visible-window buffer for a given `scroll_y` and return the
/// total content height. Message heights come from the precomputed
/// `lines_cache` (O(1) per message); the wrap walk runs only for the ~dozen
/// VISIBLE messages, storing each wrapped line's char range in the shared,
/// REUSED `ranges` buffer — the renderer then slices lines by index instead of
/// re-walking the text per pixel row. Zero steady-state alloc: both Vecs are
/// cleared, not reallocated (windowd's bump allocator never frees).
pub(crate) fn compute_visible_window(
    provider: &ChatMessageProvider,
    scroll_y: u32,
    out: &mut Vec<ChatVisibleMsg>,
    vp_h: u32,
    lines_cache: &[u32],
    ranges: &mut Vec<(u32, u32)>,
) -> u32 {
    out.clear();
    ranges.clear();
    let (_, vp_y, _, _) = viewport();
    let view_bottom = scroll_y.saturating_add(vp_h);
    let mut block_top = 0u32;
    let len = provider.len();
    for (i, opt) in provider.get(0..len).iter().enumerate() {
        let Some(msg) = opt else {
            continue;
        };
        let lines = lines_cache.get(i).copied().unwrap_or_else(|| chat_wrap_lines(msg.text));
        let height = chat_message_height(lines);
        let block_bottom = block_top.saturating_add(height);
        if block_bottom > scroll_y && block_top < view_bottom {
            let top = vp_y as i32 + block_top as i32 - scroll_y as i32;
            let range_start = ranges.len();
            for idx in 0..lines {
                let (s, e) = chat_wrap_line_range(msg.text, idx).unwrap_or((0, 0));
                ranges.push((s as u32, e as u32));
            }
            out.push(ChatVisibleMsg {
                text: msg.text,
                from_me: msg.from_me,
                top,
                lines,
                range_start,
            });
        }
        block_top = block_bottom;
    }
    block_top
}

/// Surface-local viewport rect (x, y, w, h). The chat surface top-left is (0,0);
/// the on-screen position is applied only at composite time. Content starts
/// below the title bar.
#[inline]
fn viewport() -> (u32, u32, u32, u32) {
    let vp_x = CHAT_PAD;
    let vp_y = CHAT_TITLE_BAR_H.saturating_add(CHAT_PAD);
    let vp_w =
        CHAT_PANEL_W.saturating_sub(CHAT_PAD.saturating_mul(2)).saturating_sub(CHAT_SCROLLBAR_W);
    let vp_h =
        CHAT_PANEL_H.saturating_sub(CHAT_TITLE_BAR_H).saturating_sub(CHAT_PAD.saturating_mul(2));
    (vp_x, vp_y, vp_w, vp_h)
}

/// Render one *surface-local* row `ly` (0..CHAT_PANEL_H) of the chat panel into
/// `row` (pixels written at local x 0..CHAT_PANEL_W). The caller blits the
/// finished surface to its on-screen position. No-op for `ly` outside the panel.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_chat_panel_row(
    ly: u32,
    row: &mut [u8],
    w: u32,
    scroll_y: u32,
    content_h: u32,
    visible: &[ChatVisibleMsg],
    ranges: &[(u32, u32)],
    surface_h: u32,
    title_hover: Option<super::shell_window::TitleButton>,
    corner_radius: u32,
    tk: &crate::theme::ThemeTokens,
) -> Result<(), WindowdError> {
    // The scrollbar is dropped on the GPU scroll-offset path (the surface is an
    // overscan window scrolled by composite offset, not by re-render).
    let _ = (scroll_y, content_h);
    if ly >= surface_h {
        return Ok(());
    }
    // Title bar rows render the SHARED window chrome (same title bar + close
    // "x" as the Search window), composited fixed on top of the scrolling body.
    if ly < CHAT_TITLE_BAR_H {
        return super::shell_window::draw_title_bar_row(
            ly,
            row,
            w,
            "Chat",
            CHAT_TITLE_BAR_H,
            CHAT_CLOSE_ZONE_W,
            title_hover,
            corner_radius,
            tk,
        );
    }
    // Panel background (full panel width, every row). STRAIGHT-ALPHA COPY (not a
    // blend): `band_scratch` is reused across bands without clearing, and a
    // translucent `fill_row_rect` would BLEND `PANEL_BG` over the stale rows left by
    // the previous band → ghost copies of the list ("three stacked layers"). A
    // straight copy overwrites the stale pixels AND keeps alpha 150 so the body
    // composites as real frosted glass (matching the Search window's `write_tint_span`).
    // Background spans the LIVE window width (resizable since TASK-0070
    // Phase 3); the text column below stays at its wrap width — a wider
    // window reads like a max-content-width column (re-wrap lands with the
    // Phase-7 list/layout unification).
    let _ = surface_h;
    // Theme color at the shared frosted-glass alpha (`PANEL_BG[3]` is the SSOT
    // for that translucency; the theme swaps only the RGB per mode).
    fill_row_straight(row, 0, w, crate::theme::with_alpha(tk.glass_tint, PANEL_BG[3]));

    let (vp_x, vp_y, vp_w, _vp_h) = viewport();
    // Content fills from below the title to the bottom of the (overscan) surface.
    let vp_bottom = surface_h;
    if ly < vp_y || ly >= vp_bottom {
        return Ok(());
    }
    let yi = ly as i32;

    for m in visible {
        let height = chat_message_height(m.lines);
        // Bubble background ONLY for our own (from_me) messages — incoming messages
        // read directly on the glass window body (the colored bubble is fine, the
        // rest must not overlay the window background). Clipped to the viewport.
        if m.from_me {
            let bub_top = m.top.max(vp_y as i32);
            let bub_bottom = (m.top + height as i32).min(vp_bottom as i32);
            if bub_bottom > bub_top && yi >= bub_top && yi < bub_bottom {
                fill_row_rect(
                    ly,
                    row,
                    vp_x.saturating_add(BUBBLE_INSET),
                    bub_top as u32,
                    vp_w.saturating_sub(BUBBLE_INSET.saturating_mul(2)),
                    (bub_bottom - bub_top) as u32,
                    tk.accent,
                )?;
            }
        }
        // Text lines: runtime glyphs from the baked 16px atlas. Each wrapped
        // line is a band of CHAT_LINE_H rows; the char range was walked ONCE
        // per visible message by `compute_visible_window` (the measured
        // word-wrap SSOT) — here it is a plain indexed lookup per row.
        let text_top = m.top + CHAT_MSG_PAD as i32;
        let text_bottom = text_top + (m.lines.saturating_mul(CHAT_LINE_H)) as i32;
        if yi >= text_top && yi < text_bottom {
            let line_idx = ((yi - text_top) as u32) / CHAT_LINE_H;
            if let Some(&(cs, ce)) = ranges.get(m.range_start + line_idx as usize) {
                let (cs, ce) = (cs as usize, ce as usize);
                let text_x = vp_x.saturating_add(CHAT_TEXT_INSET);
                let clip_end = vp_x.saturating_add(vp_w);
                let line_top = text_top + (line_idx * CHAT_LINE_H) as i32;
                // Our own messages sit on an accent bubble → accent-foreground for
                // contrast; incoming messages read on the glass body → fg.
                let text_color = if m.from_me { tk.accent_fg } else { tk.fg };
                draw_text_row(
                    row,
                    ly,
                    line_top,
                    text_x,
                    clip_end,
                    m.text.chars().skip(cs).take(ce.saturating_sub(cs)),
                    FontSize::Body,
                    text_color,
                );
            }
        }
    }

    Ok(())
}

/// Write one straight-alpha BGRA span (a raw copy, NOT an alpha blend) into `row`
/// over `[x0, x1)`. Used for the glass body tint so it (a) overwrites the stale
/// `band_scratch` rows reused across bands — no ghosting — and (b) keeps its real
/// alpha for the composite's glass blend (gpud blends it over the blurred backdrop).
fn fill_row_straight(row: &mut [u8], x0: u32, x1: u32, c: [u8; 4]) {
    let rp = (row.len() / 4) as u32;
    for px in x0.min(rp)..x1.min(rp) {
        let idx = px as usize * 4;
        row[idx..idx + 4].copy_from_slice(&c);
    }
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
    let track_x = CHAT_PANEL_W.saturating_sub(CHAT_PAD).saturating_sub(CHAT_SCROLLBAR_W);
    if ly >= vp_y && ly < vp_y.saturating_add(vp_h) {
        fill_row_rect(ly, row, track_x, vp_y, CHAT_SCROLLBAR_W, vp_h, SCROLL_TRACK)?;
    }
    let (thumb_y, thumb_h) = scrollbar_thumb_span(scroll_y, content_h);
    if ly >= thumb_y && ly < thumb_y.saturating_add(thumb_h) {
        fill_row_rect(ly, row, track_x, thumb_y, CHAT_SCROLLBAR_W, thumb_h, SCROLL_THUMB)?;
    }
    Ok(())
}
