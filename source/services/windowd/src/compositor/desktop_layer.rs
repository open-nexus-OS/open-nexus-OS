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

use crate::error::WindowdError;
use crate::text::{draw_text_row, line_height, measure, FontSize};

/// Topbar geometry (display-space). The bar floats with a margin like a macOS
/// menu bar; rounding/shadow/blur come from the layer composite.
pub(crate) const TOPBAR_MARGIN_X: u32 = 16;
pub(crate) const TOPBAR_TOP: u32 = 12;
pub(crate) const TOPBAR_H: u32 = 44;
pub(crate) const TOPBAR_RADIUS: u32 = 14;

/// Menu items, left to right. TOS-flavored but shell-oriented.
pub(crate) const TOPBAR_ITEMS: [&str; 5] = ["Apps", "Files", "Edit", "View", "Help"];

// Chrome labels render with the 13px runtime face (TASK-0070 Phase 6);
// layout AND hit-testing share `label_width` (a real measure), so cells and
// clicks can never disagree.
const LABEL_FONT: FontSize = FontSize::Small;
const ITEM_PAD_X: u32 = 14; // horizontal padding inside each hover cell
const ITEM_GAP: u32 = 6; // gap between cells
const TEXT_TOP: u32 = (TOPBAR_H - line_height(LABEL_FONT)) / 2; // vertically centered

/// Menu icon at the right of the bar (the real Lucide `menu` icon) — opens the
/// side panel.
const MENU_ICON_SIZE: u32 = 26;
const MENU_ICON_PAD_R: u32 = 12;

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
    measure(s.chars(), LABEL_FONT)
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

// The dropdown's *contents* are no longer a hardcoded const — they come from the
// bundle registry via [`crate::app_menu::AppMenu`] (RFC-0065). This module keeps
// only the visual geometry + per-row rendering; rows are driven by the menu the
// runtime fetched (`bundlemgrd` OP_LIST_APPS), with a seed fallback.

pub(crate) const DROPDOWN_W: u32 = 156;
/// Dropdown row geometry SSOT lives in `app_menu` (host-tested hit-testing must
/// agree with rendering). Re-export so this module's renderer uses the same values.
pub(crate) use crate::app_menu::{DROPDOWN_PAD, DROPDOWN_ROW_H};
pub(crate) const DROPDOWN_RADIUS: u32 = 12;

/// Atlas band height reserved for the dropdown — sized for the maximum registry
/// list (`app_menu::MAX_MENU_APPS`) so any fetched menu fits without re-reserving.
/// The *open* (animated) height is dynamic: `AppMenu::dropdown_full_h()`.
pub(crate) const fn dropdown_band_h() -> u32 {
    DROPDOWN_PAD * 2 + DROPDOWN_ROW_H * crate::app_menu::MAX_MENU_APPS as u32
}

/// Bar-local x of the "Apps" topbar item (the dropdown anchors under it).
/// Bar-local x of topbar item `index`'s cell (a dropdown anchors under it).
pub(crate) fn menu_item_x(index: usize) -> u32 {
    item_cell(index).map(|(s, _)| s).unwrap_or(TOPBAR_MARGIN_X)
}

/// Whether topbar item `index` owns a dropdown menu (Apps=0, Edit=2 today).
pub(crate) fn topbar_item_has_menu(index: usize) -> bool {
    matches!(index, 0 | 2)
}

/// Draw one dropdown-local row from the dynamic [`crate::app_menu::AppMenu`]:
/// glass tint, hover cell, and the registry-sourced label per row.
pub(crate) fn draw_dropdown_row(
    menu: &crate::app_menu::AppMenu,
    local_y: u32,
    row: &mut [u8],
    w: u32,
    hover: Option<usize>,
) -> Result<(), WindowdError> {
    write_tint_span(row, 0, w, TINT);
    for (i, entry) in menu.entries().iter().enumerate() {
        let top = DROPDOWN_PAD + i as u32 * DROPDOWN_ROW_H;
        if hover == Some(i) && local_y >= top && local_y < top + DROPDOWN_ROW_H {
            write_tint_span(row, 4, w.saturating_sub(4), HOVER_TINT);
        }
        let text_top = top + (DROPDOWN_ROW_H - line_height(LABEL_FONT)) / 2;
        draw_label(local_y, row, &entry.label, DROPDOWN_PAD + 6, text_top, TEXT_COLOR)?;
    }
    Ok(())
}

/// Draw a label at `(x0, top)` (bar/panel-local) in `color`, only on rows that
/// intersect the text band (13px runtime face).
fn draw_label(
    local_y: u32,
    row: &mut [u8],
    text: &str,
    x0: u32,
    top: u32,
    color: [u8; 4],
) -> Result<(), WindowdError> {
    let clip = (row.len() / 4) as u32;
    draw_text_row(row, local_y, top as i32, x0, clip, text.chars(), LABEL_FONT, color);
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

/// The search word list — owned by the real `search-app` crate (RFC-0065 /
/// ADR-0037), not duplicated here. windowd renders it; the app owns the data.
pub(crate) use search_app::model::SEARCH_WORDS;

/// Words from [`SEARCH_WORDS`] whose name starts with `prefix` (case-insensitive).
///
/// Mirrors the app's `search_app::model::filter` predicate, but fills a caller-
/// reused buffer (alloc-free per keystroke) instead of returning a fresh `Vec` —
/// windowd's hot path must not allocate per frame. The data is the app's.
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

/// Visible list rows for a Search window of height `h` (TASK-0070 Phase 3:
/// the window is resizable, so the row count derives from the live height —
/// the boot default equals `SEARCH_VISIBLE_ROWS`). At least one row.
pub(crate) fn search_visible_rows(h: u32) -> u32 {
    (h.saturating_sub(SEARCH_TITLE_H + SEARCH_FILTER_H + 2 * SEARCH_PAD) / SEARCH_ROW_H).max(1)
}

/// Max scroll offset (in rows) for `count` words in a window showing
/// `visible_rows` rows.

/// Height in px of one list row + the visible list viewport — so the shared
/// `ScrollMomentum` engine (the same one backing the chat window) can run in
/// pixels and the runtime maps its offset to a whole-row slice for the render.
pub(crate) const SEARCH_LIST_ROW_H: u32 = SEARCH_ROW_H;
pub(crate) const SEARCH_LIST_VIEWPORT_H: u32 = SEARCH_ROW_H * SEARCH_VISIBLE_ROWS;

/// Draw one window-local row of the Search window: glass body, title bar with a
/// close "x", the filter text, and the visible slice of the filtered list
/// (already scrolled by the caller). A scrollbar marks position when scrollable.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_search_window_row(
    local_y: u32,
    row: &mut [u8],
    w: u32,
    visible_rows: u32,
    filter_text: &str,
    visible_words: &[&'static str],
    scroll: u32,
    total: usize,
    title_hover: Option<super::shell_window::TitleButton>,
    corner_radius: u32,
) -> Result<(), WindowdError> {
    write_tint_span(row, 0, w, TINT);
    if local_y < SEARCH_TITLE_H {
        // Shared window chrome (same title bar + `[– □ ×]` as the chat window).
        return super::shell_window::draw_title_bar_row(
            local_y,
            row,
            w,
            "Search",
            SEARCH_TITLE_H,
            SEARCH_CLOSE_W,
            title_hover,
            corner_radius,
        );
    }
    let filter_top = SEARCH_TITLE_H + (SEARCH_FILTER_H - line_height(LABEL_FONT)) / 2;
    if local_y >= SEARCH_TITLE_H && local_y < SEARCH_TITLE_H + SEARCH_FILTER_H {
        write_tint_span(row, SEARCH_PAD, w.saturating_sub(SEARCH_PAD), [30, 28, 26, 150]);
        if filter_text.is_empty() {
            draw_label(local_y, row, "type to filter...", SEARCH_PAD + 6, filter_top, [150, 150, 150, 200])?;
        } else {
            draw_label(local_y, row, filter_text, SEARCH_PAD + 6, filter_top, TEXT_COLOR)?;
        }
        return Ok(());
    }
    // PIXEL-scrolled list (TASK-0070 list architecture, step 1): `scroll` is the
    // list's pixel offset (the momentum engine's native unit) — rows glide
    // smoothly instead of snapping whole 24px rows. Each surface row maps to a
    // content row + a within-row remainder; partially visible rows at the top
    // and bottom render naturally.
    let list_top = SEARCH_TITLE_H + SEARCH_FILTER_H + SEARCH_PAD;
    if local_y >= list_top && local_y < list_top + SEARCH_ROW_H * visible_rows {
        let content_y = (local_y - list_top) + scroll;
        let row_idx = (content_y / SEARCH_ROW_H) as usize;
        if let Some(word) = visible_words.get(row_idx) {
            let row_screen_top = local_y - (content_y % SEARCH_ROW_H);
            let text_top = row_screen_top + (SEARCH_ROW_H - line_height(LABEL_FONT)) / 2;
            draw_label(local_y, row, word, SEARCH_PAD + 6, text_top, TEXT_COLOR)?;
        }
    }
    // Scrollbar thumb on the right when the list overflows (pixel-space math).
    let max_scroll_px =
        (total as u32 * SEARCH_ROW_H).saturating_sub(SEARCH_ROW_H * visible_rows);
    if max_scroll_px > 0 {
        let track_top = list_top;
        let track_h = SEARCH_ROW_H * visible_rows;
        let thumb_h = (track_h * visible_rows / total.max(1) as u32).clamp(16, track_h);
        let thumb_y =
            track_top + (track_h - thumb_h) * scroll.min(max_scroll_px) / max_scroll_px;
        if local_y >= thumb_y && local_y < thumb_y + thumb_h {
            write_tint_span(row, w.saturating_sub(8), w.saturating_sub(4), [200, 200, 200, 180]);
        }
    }
    Ok(())
}

// ── Settings window — a static glass panel opened from Edit → Settings. ──────
// TASK-0072 Phase 10 seed: a real 3rd ShellWindow (z/focus/minimize/snap come
// free from the shared frame). Classic flat-framed sections (sharp 1px lines) —
// the vocabulary the settings panel grows into; the Appearance rows read the
// current values (theme/font), the live toggle + settingsd wiring land next.
pub(crate) const SETTINGS_W: u32 = 380;
pub(crate) const SETTINGS_TITLE_H: u32 = 36;
pub(crate) const SETTINGS_CLOSE_W: u32 = 40;
pub(crate) const SETTINGS_RADIUS: u32 = 16;
const SETTINGS_PAD: u32 = 16;
const SETTINGS_SECTION_GAP: u32 = 14;
const SETTINGS_ROW_H: u32 = 34;
const SETTINGS_LABEL_FONT: FontSize = FontSize::Body;
const SETTINGS_HEADER_FONT: FontSize = FontSize::Small;

/// Flat section frame line + the section-row separator (sharp 1px, classic).
const SETTINGS_FRAME: [u8; 4] = [92, 84, 78, 220];
const SETTINGS_VALUE: [u8; 4] = [190, 210, 245, 255];

/// One "Appearance" section with two rows (Theme, Font). Fixed layout → the
/// window height is a const; no scroll.
const SETTINGS_ROWS: u32 = 2;

/// Full settings window height: title + Appearance header + framed rows + pad.
pub(crate) const fn settings_full_h() -> u32 {
    SETTINGS_TITLE_H
        + SETTINGS_PAD
        + line_height(SETTINGS_HEADER_FONT)
        + 6
        + SETTINGS_ROW_H * SETTINGS_ROWS
        + SETTINGS_SECTION_GAP
        + SETTINGS_PAD
}

/// Render one window-local row `local_y` of the settings panel into `row`.
/// `theme` / `font` are the current values shown on the right of each row.
pub(crate) fn draw_settings_window_row(
    local_y: u32,
    row: &mut [u8],
    w: u32,
    theme: &str,
    font: &str,
    title_hover: Option<super::shell_window::TitleButton>,
    corner_radius: u32,
) -> Result<(), WindowdError> {
    // Title bar (shared chrome: title + [– □ ×]).
    if local_y < SETTINGS_TITLE_H {
        return super::shell_window::draw_title_bar_row(
            local_y,
            row,
            w,
            "Settings",
            SETTINGS_TITLE_H,
            SETTINGS_CLOSE_W,
            title_hover,
            corner_radius,
        );
    }
    // Glass body.
    write_tint_span(row, 0, w, TINT);

    let body_x0 = SETTINGS_PAD;
    let body_x1 = w.saturating_sub(SETTINGS_PAD);
    // "Appearance" section header.
    let header_top = SETTINGS_TITLE_H + SETTINGS_PAD;
    if local_y >= header_top && local_y < header_top + line_height(SETTINGS_HEADER_FONT) {
        draw_text_row(
            row,
            local_y,
            header_top as i32,
            body_x0,
            body_x1,
            "APPEARANCE".chars(),
            SETTINGS_HEADER_FONT,
            [150, 150, 160, 255],
        );
        return Ok(());
    }
    // Framed rows block.
    let rows_top = header_top + line_height(SETTINGS_HEADER_FONT) + 6;
    let rows_bottom = rows_top + SETTINGS_ROW_H * SETTINGS_ROWS;
    if local_y >= rows_top && local_y < rows_bottom {
        // Outer + inter-row frame lines (sharp 1px).
        let on_frame = local_y == rows_top
            || local_y == rows_bottom - 1
            || (local_y >= rows_top && (local_y - rows_top) % SETTINGS_ROW_H == 0);
        if on_frame {
            write_tint_span(row, body_x0, body_x1, SETTINGS_FRAME);
        }
        // Row content: label left, value right.
        let idx = (local_y - rows_top) / SETTINGS_ROW_H;
        let (label, value) = match idx {
            0 => ("Theme", theme),
            _ => ("Font", font),
        };
        let row_top = rows_top + idx * SETTINGS_ROW_H;
        let text_top = row_top + (SETTINGS_ROW_H - line_height(SETTINGS_LABEL_FONT)) / 2;
        // Left/right frame verticals.
        if local_y > row_top && local_y < row_top + SETTINGS_ROW_H {
            write_tint_span(row, body_x0, body_x0 + 1, SETTINGS_FRAME);
            write_tint_span(row, body_x1.saturating_sub(1), body_x1, SETTINGS_FRAME);
        }
        draw_text_row(
            row,
            local_y,
            text_top as i32,
            body_x0 + 12,
            body_x1,
            label.chars(),
            SETTINGS_LABEL_FONT,
            [230, 230, 235, 255],
        );
        let vw = measure(value.chars(), SETTINGS_LABEL_FONT);
        let vx = body_x1.saturating_sub(12 + vw);
        draw_text_row(
            row,
            local_y,
            text_top as i32,
            vx,
            body_x1.saturating_sub(12),
            value.chars(),
            SETTINGS_LABEL_FONT,
            SETTINGS_VALUE,
        );
    }
    Ok(())
}

/// Alpha-blend one row of a straight-alpha BGRA icon sprite into `row` at column
/// `dst_x`. `icon_row` is the sprite's row index; `alpha_mul` (0..=255) scales the
/// sprite alpha (hover / brightness). src-over in straight alpha; the destination
/// keeps its own alpha (the surface composites over the backdrop later).
pub(crate) fn blend_icon_row(row: &mut [u8], dst_x: u32, icon: &[u8], dim: u32, icon_row: u32, alpha_mul: u8) {
    if icon_row >= dim {
        return;
    }
    let rp = (row.len() / 4) as u32;
    let src_off = (icon_row * dim) as usize * 4;
    for ix in 0..dim {
        let px = dst_x + ix;
        if px >= rp {
            break;
        }
        let s = src_off + ix as usize * 4;
        if s + 4 > icon.len() {
            break;
        }
        let a = u32::from(icon[s + 3]) * u32::from(alpha_mul) / 255;
        if a == 0 {
            continue;
        }
        let inv = 255 - a;
        let d = px as usize * 4;
        for ch in 0..3 {
            row[d + ch] = ((u32::from(icon[s + ch]) * a + u32::from(row[d + ch]) * inv) / 255) as u8;
        }
    }
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
    // Menu icon at the right — the REAL Lucide `menu` icon (rasterized white,
    // straight-alpha) blended in, over a hover-highlight cell.
    {
        let icon_x = menu_icon_x(bar_w);
        let icon_y0 = (TOPBAR_H.saturating_sub(MENU_ICON_SIZE)) / 2;
        if menu_hover && local_y >= icon_y0 && local_y < icon_y0 + MENU_ICON_SIZE {
            write_tint_span(row, icon_x, (icon_x + MENU_ICON_SIZE).min(bar_w), HOVER_TINT);
        }
        if local_y >= icon_y0 && local_y < icon_y0 + crate::assets::MENU_ICON_DIM {
            blend_icon_row(
                row,
                icon_x,
                crate::assets::MENU_ICON_BGRA,
                crate::assets::MENU_ICON_DIM,
                local_y - icon_y0,
                255,
            );
        }
    }

    // Labels — `draw_label` gates on the text band internally.
    for (i, item) in TOPBAR_ITEMS.iter().enumerate() {
        let Some((cell_x, _)) = item_cell(i) else { continue };
        draw_label(local_y, row, item, cell_x + ITEM_PAD_X, TEXT_TOP, TEXT_COLOR)?;
    }
    Ok(())
}
