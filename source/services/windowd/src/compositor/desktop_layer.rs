// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shell-P2b — the desktop **glass topbar** (a TOS-style menu bar with
//! blur + rounded corners + drop shadow + hover), rendered into an atlas surface
//! and composited onto the scanout as one GPU layer (the path that reaches the
//! virgl scanout; the retained Plane 1 does not — see
//! [[black-screen-is-2d-3d-dual-not-host]]). Same mechanism as the chat window:
//! `try_composite_layer` with `backdrop_blur` + `corner_radius` + shadow does
//! the glass; the atlas carries a translucent tint + opaque text, blended with
//! the shared straight-alpha layer blend.
//! OWNERS: @ui
//! STATUS: In progress (P2b)

use super::font::bitmap_font_5x7;
use super::primitives::fill_row_rect;
use crate::error::WindowdError;

/// Topbar geometry (display-space). The bar floats with a margin like a macOS
/// menu bar; rounding/shadow/blur come from the layer composite.
pub(crate) const TOPBAR_MARGIN_X: u32 = 16;
pub(crate) const TOPBAR_TOP: u32 = 12;
pub(crate) const TOPBAR_H: u32 = 44;
pub(crate) const TOPBAR_RADIUS: u32 = 14;

/// Menu items, left to right. TOS-flavored but shell-oriented.
pub(crate) const TOPBAR_ITEMS: [&str; 5] = ["Apps", "Files", "Edit", "View", "Help"];

// Glyph metrics for the topbar labels (5×7 bitmap font, scaled up).
const FONT_W: u32 = 5;
const FONT_H: u32 = 7;
const FONT_SCALE: u32 = 2;
const GLYPH_W: u32 = FONT_W * FONT_SCALE; // 10
const GLYPH_ADVANCE: u32 = GLYPH_W + 2; // 12
const ITEM_PAD_X: u32 = 14; // horizontal padding inside each hover cell
const ITEM_GAP: u32 = 6; // gap between cells
const TEXT_TOP: u32 = (TOPBAR_H - FONT_H * FONT_SCALE) / 2; // vertically centered

/// Menu (hamburger) icon at the right of the bar — opens the side panel.
const MENU_ICON_SIZE: u32 = 26;
const MENU_ICON_PAD_R: u32 = 12;
const MENU_BAR_W: u32 = 16;
const MENU_BAR_H: u32 = 2;
const MENU_BAR_GAP: u32 = 4;

/// Bar-local x of the menu icon's left edge for a bar `bar_w` wide.
fn menu_icon_x(bar_w: u32) -> u32 {
    bar_w.saturating_sub(MENU_ICON_SIZE + MENU_ICON_PAD_R)
}

/// Menu-icon hit-test in **bar-local** coordinates (the caller offsets the
/// cursor by the bar's on-screen origin). A generous square around the glyph.
pub(crate) fn topbar_menu_icon_hit(local_x: u32, local_y: u32, bar_w: u32) -> bool {
    let x0 = menu_icon_x(bar_w);
    let y0 = (TOPBAR_H.saturating_sub(MENU_ICON_SIZE)) / 2;
    local_x >= x0 && local_x < x0 + MENU_ICON_SIZE && local_y >= y0 && local_y < y0 + MENU_ICON_SIZE
}

/// Glass tint (cool dark, translucent) + brighter hover tint. Straight alpha —
/// gpud's layer blend (H_BLEND_ALPHA) composites these over the blurred backdrop.
const TINT: [u8; 4] = [40, 34, 30, 150]; // BGRA, ~59% — reads as frosted glass
const HOVER_TINT: [u8; 4] = [120, 110, 100, 96]; // additive-ish lighter cell
const TEXT_COLOR: [u8; 4] = [255, 255, 255, 255];

/// Pixel width of a label at the topbar font.
fn label_width(s: &str) -> u32 {
    let n = s.chars().count() as u32;
    if n == 0 {
        0
    } else {
        n * GLYPH_ADVANCE - (GLYPH_ADVANCE - GLYPH_W)
    }
}

/// `[start_x, end_x)` of each item's hover cell within the bar (local x).
fn item_cell(index: usize) -> Option<(u32, u32)> {
    let mut x = TOPBAR_MARGIN_X;
    for (i, item) in TOPBAR_ITEMS.iter().enumerate() {
        let cell_w = label_width(item) + 2 * ITEM_PAD_X;
        if i == index {
            return Some((x, x + cell_w));
        }
        x += cell_w + ITEM_GAP;
    }
    None
}

/// Which item (if any) the bar-local point `local_x` falls in.
pub(crate) fn topbar_item_at(local_x: u32) -> Option<usize> {
    (0..TOPBAR_ITEMS.len()).find(|&i| item_cell(i).is_some_and(|(s, e)| local_x >= s && local_x < e))
}

/// Side panel that slides in from the right when the topbar menu is tapped.
pub(crate) const SIDEPANEL_W: u32 = 300;
pub(crate) const SIDEPANEL_MARGIN: u32 = 16;
pub(crate) const SIDEPANEL_RADIUS: u32 = 18;
/// Top of the panel (below the topbar).
pub(crate) const SIDEPANEL_TOP: u32 = TOPBAR_TOP + TOPBAR_H + 10;
// ── Topbar "Apps" dropdown — a small reusable glass menu, animated open. ──
//
// A self-contained dropdown "component": its items + geometry + per-row
// rendering live here so the scene graph can later own/optimize it. For now
// windowd rasterizes it into an atlas and composites it as one animated layer.

/// A clickable row in the Apps dropdown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DropdownItem {
    Chat,
    Search,
}

const DROPDOWN_ITEMS: [(DropdownItem, &str); 2] =
    [(DropdownItem::Chat, "Chat"), (DropdownItem::Search, "Search")];

pub(crate) const DROPDOWN_W: u32 = 156;
pub(crate) const DROPDOWN_PAD: u32 = 8;
pub(crate) const DROPDOWN_ROW_H: u32 = 30;
pub(crate) const DROPDOWN_RADIUS: u32 = 12;

/// Full (open) height of the dropdown.
pub(crate) const fn dropdown_full_h() -> u32 {
    DROPDOWN_PAD * 2 + DROPDOWN_ROW_H * DROPDOWN_ITEMS.len() as u32
}

/// Bar-local x of the "Apps" topbar item (the dropdown anchors under it).
pub(crate) fn apps_item_x() -> u32 {
    item_cell(0).map(|(s, _)| s).unwrap_or(TOPBAR_MARGIN_X)
}

/// Which dropdown item a dropdown-local point falls in.
pub(crate) fn dropdown_item_at(local_y: u32) -> Option<DropdownItem> {
    for (i, (item, _)) in DROPDOWN_ITEMS.iter().enumerate() {
        let top = DROPDOWN_PAD + i as u32 * DROPDOWN_ROW_H;
        if local_y >= top && local_y < top + DROPDOWN_ROW_H {
            return Some(*item);
        }
    }
    None
}

/// Draw one dropdown-local row: glass tint, hover cell, item label.
pub(crate) fn draw_dropdown_row(
    local_y: u32,
    row: &mut [u8],
    w: u32,
    hover: Option<DropdownItem>,
) -> Result<(), WindowdError> {
    write_tint_span(row, 0, w, TINT);
    for (i, (item, label)) in DROPDOWN_ITEMS.iter().enumerate() {
        let top = DROPDOWN_PAD + i as u32 * DROPDOWN_ROW_H;
        if hover == Some(*item) && local_y >= top && local_y < top + DROPDOWN_ROW_H {
            write_tint_span(row, 4, w.saturating_sub(4), HOVER_TINT);
        }
        let text_top = top + (DROPDOWN_ROW_H - FONT_H * FONT_SCALE) / 2;
        draw_label(local_y, row, label, DROPDOWN_PAD + 6, text_top, TEXT_COLOR)?;
    }
    Ok(())
}

/// Draw a label at `(x0, top)` (bar/panel-local) in `color`, only on rows that
/// intersect the glyph band.
fn draw_label(local_y: u32, row: &mut [u8], text: &str, x0: u32, top: u32, color: [u8; 4]) -> Result<(), WindowdError> {
    if local_y < top || local_y >= top + FONT_H * FONT_SCALE {
        return Ok(());
    }
    let glyph_row = ((local_y - top) / FONT_SCALE).min(FONT_H - 1) as usize;
    let mut pen_x = x0;
    for ch in text.chars() {
        let bits = bitmap_font_5x7(ch)[glyph_row];
        for col in 0..FONT_W {
            if bits & (1 << (FONT_W - 1 - col)) != 0 {
                fill_row_rect(local_y, row, pen_x + col * FONT_SCALE, local_y, FONT_SCALE, 1, color)?;
            }
        }
        pen_x += GLYPH_ADVANCE;
    }
    Ok(())
}

/// Draw one panel-local row of the glass side panel: translucent body, a title,
/// and a vertical list of items. Corners/shadow/blur applied by the composite.
pub(crate) fn draw_sidepanel_row(local_y: u32, row: &mut [u8], panel_w: u32) -> Result<(), WindowdError> {
    let _ = local_y;
    // Empty glass body for now (content TBD); the composite rounds + shadows it.
    write_tint_span(row, 0, panel_w, TINT);
    Ok(())
}

// ── Search window — a movable/closable glass window with a filterable list. ──
pub(crate) const SEARCH_W: u32 = 360;
pub(crate) const SEARCH_TITLE_H: u32 = 36;
pub(crate) const SEARCH_CLOSE_W: u32 = 40;
pub(crate) const SEARCH_RADIUS: u32 = 16;
const SEARCH_PAD: u32 = 14;
const SEARCH_ROW_H: u32 = 28;
const SEARCH_FILTER_H: u32 = 30;
/// Visible word rows in the window (the filtered list scrolls within these).
pub(crate) const SEARCH_VISIBLE_ROWS: u32 = 10;

/// A longer demo word list so the filtered result actually scrolls.
pub(crate) const SEARCH_WORDS: &[&str] = &[
    "apple", "application", "apt", "arrow", "asset", "atom", "audio", "batch", "binary", "block",
    "buffer", "build", "cache", "canvas", "channel", "clock", "cluster", "codec", "compile",
    "component", "config", "context", "cursor", "daemon", "device", "display", "driver", "engine",
    "event", "filter", "fragment", "frame", "gradient", "handle", "kernel", "layer", "module",
    "neuron", "packet", "pipeline", "pointer", "process", "render", "scanout", "scene", "shader",
    "shell", "socket", "surface", "texture", "thread", "vector", "vertex", "widget", "window",
];

/// Words from [`SEARCH_WORDS`] whose name starts with `prefix` (case-insensitive).
pub(crate) fn search_filter<'a>(prefix: &str, out: &mut alloc::vec::Vec<&'static str>) {
    out.clear();
    for w in SEARCH_WORDS {
        let hit = prefix.is_empty()
            || (w.len() >= prefix.len()
                && w.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()));
        if hit {
            out.push(w);
        }
    }
}

/// Full window height (title + filter field + SEARCH_VISIBLE_ROWS rows).
pub(crate) const fn search_full_h() -> u32 {
    SEARCH_TITLE_H + SEARCH_FILTER_H + SEARCH_PAD + SEARCH_ROW_H * SEARCH_VISIBLE_ROWS + SEARCH_PAD
}

/// Max scroll offset (in rows) for a filtered list of `count` words.
pub(crate) fn search_max_scroll(count: usize) -> u32 {
    (count as u32).saturating_sub(SEARCH_VISIBLE_ROWS)
}

/// Draw one window-local row of the Search window: glass body, title bar with a
/// close "x", the filter text, and the visible slice of the filtered list
/// (already scrolled by the caller). A scrollbar marks position when scrollable.
pub(crate) fn draw_search_window_row(
    local_y: u32,
    row: &mut [u8],
    w: u32,
    filter_text: &str,
    visible_words: &[&'static str],
    scroll: u32,
    total: usize,
    close_hover: bool,
) -> Result<(), WindowdError> {
    write_tint_span(row, 0, w, TINT);
    if local_y < SEARCH_TITLE_H {
        // Shared window chrome (same title bar + close "x" as the chat window).
        return super::shell_window::draw_title_bar_row(
            local_y,
            row,
            w,
            "Search",
            SEARCH_TITLE_H,
            SEARCH_CLOSE_W,
            close_hover,
        );
    }
    let filter_top = SEARCH_TITLE_H + (SEARCH_FILTER_H - FONT_H * FONT_SCALE) / 2;
    if local_y >= SEARCH_TITLE_H && local_y < SEARCH_TITLE_H + SEARCH_FILTER_H {
        write_tint_span(row, SEARCH_PAD, w.saturating_sub(SEARCH_PAD), [30, 28, 26, 150]);
        if filter_text.is_empty() {
            draw_label(local_y, row, "type to filter...", SEARCH_PAD + 6, filter_top, [150, 150, 150, 200])?;
        } else {
            draw_label(local_y, row, filter_text, SEARCH_PAD + 6, filter_top, TEXT_COLOR)?;
        }
        return Ok(());
    }
    let list_top = SEARCH_TITLE_H + SEARCH_FILTER_H + SEARCH_PAD;
    for (i, word) in visible_words.iter().take(SEARCH_VISIBLE_ROWS as usize).enumerate() {
        let rt = list_top + i as u32 * SEARCH_ROW_H;
        let text_top = rt + (SEARCH_ROW_H - FONT_H * FONT_SCALE) / 2;
        draw_label(local_y, row, word, SEARCH_PAD + 6, text_top, TEXT_COLOR)?;
    }
    // Scrollbar thumb on the right when the list overflows.
    let max_scroll = search_max_scroll(total);
    if max_scroll > 0 {
        let track_top = list_top;
        let track_h = SEARCH_ROW_H * SEARCH_VISIBLE_ROWS;
        let thumb_h = (track_h * SEARCH_VISIBLE_ROWS / total.max(1) as u32).clamp(16, track_h);
        let thumb_y = track_top + (track_h - thumb_h) * scroll / max_scroll;
        if local_y >= thumb_y && local_y < thumb_y + thumb_h {
            write_tint_span(row, w.saturating_sub(8), w.saturating_sub(4), [200, 200, 200, 180]);
        }
    }
    Ok(())
}

/// Write one straight-alpha BGRA span (no premultiply); gpud's layer blend does
/// the SRC_ALPHA compositing over the (blurred) backdrop.
fn write_tint_span(row: &mut [u8], x0: u32, x1: u32, c: [u8; 4]) {
    let rp = (row.len() / 4) as u32;
    for px in x0.min(rp)..x1.min(rp) {
        let idx = px as usize * 4;
        row[idx..idx + 4].copy_from_slice(&c);
    }
}

/// Draw one atlas row (`local_y`, bar-local) of the glass topbar: translucent
/// tint across the bar, a brighter cell behind the hovered item, and the item
/// labels in opaque white. Corners/shadow/blur are applied by the composite.
pub(crate) fn draw_topbar_row(
    local_y: u32,
    row: &mut [u8],
    bar_w: u32,
    hover: Option<usize>,
    menu_hover: bool,
) -> Result<(), WindowdError> {
    // Base glass tint across the whole bar (corner mask applied at composite).
    write_tint_span(row, 0, bar_w, TINT);
    // Hover cell highlight.
    if let Some(h) = hover {
        if let Some((s, e)) = item_cell(h) {
            write_tint_span(row, s, e.min(bar_w), HOVER_TINT);
        }
    }
    // Menu (hamburger) icon at the right — three white bars.
    {
        let icon_x = menu_icon_x(bar_w);
        let icon_y0 = (TOPBAR_H.saturating_sub(MENU_ICON_SIZE)) / 2;
        // Hover highlight cell behind the icon.
        if menu_hover && local_y >= icon_y0 && local_y < icon_y0 + MENU_ICON_SIZE {
            write_tint_span(row, icon_x, (icon_x + MENU_ICON_SIZE).min(bar_w), HOVER_TINT);
        }
        let bars_total = 3 * MENU_BAR_H + 2 * MENU_BAR_GAP;
        let bars_top = icon_y0 + (MENU_ICON_SIZE.saturating_sub(bars_total)) / 2;
        let bar_x = icon_x + (MENU_ICON_SIZE.saturating_sub(MENU_BAR_W)) / 2;
        for i in 0..3u32 {
            let by = bars_top + i * (MENU_BAR_H + MENU_BAR_GAP);
            if local_y >= by && local_y < by + MENU_BAR_H {
                fill_row_rect(local_y, row, bar_x, local_y, MENU_BAR_W, 1, TEXT_COLOR)?;
            }
        }
    }

    // Labels — only on rows that intersect the text band.
    if local_y < TEXT_TOP || local_y >= TEXT_TOP + FONT_H * FONT_SCALE {
        return Ok(());
    }
    let glyph_row = ((local_y - TEXT_TOP) / FONT_SCALE).min(FONT_H - 1) as usize;
    for (i, item) in TOPBAR_ITEMS.iter().enumerate() {
        let Some((cell_x, _)) = item_cell(i) else { continue };
        let mut pen_x = cell_x + ITEM_PAD_X;
        for ch in item.chars() {
            let bits = bitmap_font_5x7(ch)[glyph_row];
            for col in 0..FONT_W {
                if bits & (1 << (FONT_W - 1 - col)) != 0 {
                    fill_row_rect(local_y, row, pen_x + col * FONT_SCALE, local_y, FONT_SCALE, 1, TEXT_COLOR)?;
                }
            }
            pen_x += GLYPH_ADVANCE;
        }
    }
    Ok(())
}
