// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Single source of truth for windowd's interactive geometry and the
//! pure hit-test logic over it. This is the compositor-owned hit-testing model
//! (OHOS HWC / Fuchsia Flatland / Apple): the window server resolves
//! hover/click/scroll against the *same* rects its live renderer paints, so a
//! control's hit area is always identical to its rendered rect. inputd no longer
//! hit-tests — it ships a display-space pointer plus raw button/wheel/key facts,
//! and windowd resolves intent here.
//!
//! Every rect is in display pixels. The rect builders below are called by both
//! the live renderer (`draw_animation_proof_overlay_row`) and the hit-tester,
//! which is what makes "hit area == rendered rect" structural rather than a
//! coincidence kept in sync by hand. This module is intentionally free of any
//! OS/`no_std` dependencies so it is fully host-testable.
//!
//! OWNERS: @ui
//! STATUS: TASK-0063 — windowd-owned hit-testing
//! API_STABILITY: Unstable

use crate::smoke::VisibleBootstrapMode;

// ── Glass button (top-right hamburger): hover highlights, click toggles sidebar ──
pub(crate) const GLASS_BUTTON_W: u32 = 156;
pub(crate) const GLASS_BUTTON_H: u32 = 56;
pub(crate) const GLASS_BUTTON_TOP: u32 = 24;
pub(crate) const GLASS_BUTTON_RIGHT: u32 = 24;
pub(crate) const GLASS_BUTTON_RADIUS: u32 = 18;

// ── Sidebar (right-edge glass panel) ──
pub(crate) const SIDEBAR_WIDTH: u32 = 320;
pub(crate) const SIDEBAR_MARGIN_TOP: u32 = 18;
pub(crate) const SIDEBAR_MARGIN_BOTTOM: u32 = 18;
pub(crate) const SIDEBAR_RADIUS: u32 = 24;

// ── Close (X) icon inside the sidebar ──
pub(crate) const LUCIDE_ICON_SIZE: u32 = 24;
pub(crate) const VISIBLE_ROUTE_WIDTH: u32 = 64;
pub(crate) const VISIBLE_ROUTE_HEIGHT: u32 = 48;
pub(crate) const CLOSE_TARGET_ROUTE_X: u32 = 52;
pub(crate) const CLOSE_TARGET_ROUTE_Y: u32 = 18;
/// Padding added around the close icon so the clickable area is comfortable.
const CLOSE_HIT_PAD: u32 = 12;

// ── Left proof/test panel ──
pub(crate) const PROOF_PANEL_X: u32 = 56;
pub(crate) const PROOF_PANEL_Y: u32 = 440;

// ── Chat panel (right side, clear of the 826px-wide combined left panel that
// spans x=56..882). Right edge aligns with the glass button (x..1256). ──
pub(crate) const CHAT_PANEL_X: u32 = 890;
pub(crate) const CHAT_PANEL_Y: u32 = 96;
pub(crate) const CHAT_PANEL_W: u32 = 366;
pub(crate) const CHAT_PANEL_H: u32 = 600;
/// Inner padding of the chat panel and width reserved for its scrollbar.
pub(crate) const CHAT_PAD: u32 = 14;
pub(crate) const CHAT_SCROLLBAR_W: u32 = 8;
// 5×7 bitmap font at 2× — one glyph cell is 6×7 → advance 12px, line 20px.
pub(crate) const CHAT_FONT_W: u32 = 5;
pub(crate) const CHAT_FONT_H: u32 = 7;
pub(crate) const CHAT_FONT_SCALE: u32 = 2;
pub(crate) const CHAT_FONT_ADVANCE: u32 = (CHAT_FONT_W + 1) * CHAT_FONT_SCALE;
pub(crate) const CHAT_LINE_H: u32 = CHAT_FONT_H * CHAT_FONT_SCALE + 6;
/// Vertical padding inside a message bubble (top and bottom).
pub(crate) const CHAT_MSG_PAD: u32 = 8;

/// Width in pixels available for chat text (panel minus padding and scrollbar).
pub(crate) fn chat_text_width() -> u32 {
    CHAT_PANEL_W
        .saturating_sub(CHAT_PAD.saturating_mul(2))
        .saturating_sub(CHAT_SCROLLBAR_W)
}

/// Characters per wrapped line for the chat (hard-wrap; the renderer is the
/// single source of truth for wrapping, so layout and paint can never drift).
/// Wrapping is by char count and the renderer slices by char boundary, so
/// multi-byte UTF-8 in the message pool (e.g. em-dashes) is handled correctly.
pub(crate) fn chat_chars_per_line() -> usize {
    (chat_text_width() / CHAT_FONT_ADVANCE).max(1) as usize
}

/// Number of wrapped lines a message of `char_count` characters occupies.
pub(crate) fn chat_message_lines(char_count: usize, cpl: usize) -> u32 {
    let cpl = cpl.max(1);
    (char_count.div_ceil(cpl)).max(1) as u32
}

/// Total block height of a message (its wrapped lines plus top/bottom padding).
pub(crate) fn chat_message_height(lines: u32) -> u32 {
    lines
        .saturating_mul(CHAT_LINE_H)
        .saturating_add(CHAT_MSG_PAD.saturating_mul(2))
}

/// Character range `[start, end)` of wrapped line `idx` within a message of
/// `char_count` characters. Returns `None` past the last line. These are
/// *character* offsets (not byte offsets); the renderer walks `chars()` so it
/// never splits a multi-byte codepoint.
pub(crate) fn chat_line_char_range(
    char_count: usize,
    cpl: usize,
    idx: u32,
) -> Option<(usize, usize)> {
    let cpl = cpl.max(1);
    let lines = chat_message_lines(char_count, cpl);
    if idx >= lines {
        return None;
    }
    let start = (idx as usize).saturating_mul(cpl).min(char_count);
    let end = start.saturating_add(cpl).min(char_count);
    Some((start, end))
}

/// A display-space rectangle. Self-contained (no dependency on the OS-only
/// compositor types) so the geometry is host-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HitRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The action a primary-button press resolves to, given the cursor position and
/// current sidebar state. Pure and host-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickAction {
    /// Cursor is over the glass button → toggle the sidebar open/closed.
    ToggleSidebar,
    /// Cursor is over the close icon, or outside an open sidebar → close it.
    CloseSidebar,
    /// Cursor is over the left proof/test panel → focus it (filter input).
    FocusPanel,
    /// Click landed on nothing interactive.
    None,
}

/// Which scrollable a wheel event over the cursor should drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WheelTarget {
    Chat,
    Filter,
    None,
}

#[inline]
pub(crate) fn rect_contains(rect: HitRect, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 {
        return false;
    }
    let (x, y) = (x as u32, y as u32);
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

/// Midpoint of a route cell mapped into display extent. Mirrors the renderer's
/// close-icon placement so the close hit rect tracks the rendered icon exactly.
pub(crate) fn route_cell_midpoint(route_coord: u32, route_extent: u32, display_extent: u32) -> u32 {
    let start = route_coord.saturating_mul(display_extent) / route_extent.max(1);
    let end = (route_coord.saturating_add(1))
        .saturating_mul(display_extent)
        .saturating_add(route_extent.saturating_sub(1))
        / route_extent.max(1);
    let end = end.max(start.saturating_add(1));
    (start.saturating_add(end).saturating_sub(1)) / 2
}

/// Glass button rect — top-right. `width` is the display width.
pub(crate) fn button_rect(width: u32) -> HitRect {
    HitRect {
        x: width.saturating_sub(GLASS_BUTTON_W.saturating_add(GLASS_BUTTON_RIGHT)),
        y: GLASS_BUTTON_TOP,
        width: GLASS_BUTTON_W,
        height: GLASS_BUTTON_H,
    }
}

/// Sidebar rect for a given horizontal `translate` (0 = fully open, SIDEBAR_WIDTH
/// = fully closed/offscreen). The renderer uses the identical expression.
pub(crate) fn sidebar_rect(mode: VisibleBootstrapMode, translate_x: f32) -> HitRect {
    let translate = translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32;
    let x = mode
        .width
        .saturating_sub(SIDEBAR_WIDTH)
        .saturating_add(translate);
    let height = mode
        .height
        .saturating_sub(SIDEBAR_MARGIN_TOP.saturating_add(SIDEBAR_MARGIN_BOTTOM))
        .max(1);
    HitRect {
        x,
        y: SIDEBAR_MARGIN_TOP,
        width: SIDEBAR_WIDTH,
        height,
    }
}

/// The rendered close (X) icon rect inside `sidebar`. Mirrors the renderer's
/// clamp so the icon never escapes the panel.
pub(crate) fn sidebar_close_icon_rect(mode: VisibleBootstrapMode, sidebar: HitRect) -> HitRect {
    let close_mid_x = route_cell_midpoint(CLOSE_TARGET_ROUTE_X, VISIBLE_ROUTE_WIDTH, mode.width);
    let close_mid_y = route_cell_midpoint(CLOSE_TARGET_ROUTE_Y, VISIBLE_ROUTE_HEIGHT, mode.height);
    let sidebar_end_x = sidebar.x.saturating_add(sidebar.width);
    let sidebar_end_y = sidebar.y.saturating_add(sidebar.height);
    let x = close_mid_x.saturating_sub(LUCIDE_ICON_SIZE / 2).clamp(
        sidebar.x.saturating_add(14),
        sidebar_end_x.saturating_sub(LUCIDE_ICON_SIZE + 14),
    );
    let y = close_mid_y.saturating_sub(LUCIDE_ICON_SIZE / 2).clamp(
        sidebar.y.saturating_add(14),
        sidebar_end_y.saturating_sub(LUCIDE_ICON_SIZE + 14),
    );
    HitRect {
        x,
        y,
        width: LUCIDE_ICON_SIZE,
        height: LUCIDE_ICON_SIZE,
    }
}

/// Comfortable click target around the close icon.
fn sidebar_close_hit_rect(mode: VisibleBootstrapMode, sidebar: HitRect) -> HitRect {
    let icon = sidebar_close_icon_rect(mode, sidebar);
    HitRect {
        x: icon.x.saturating_sub(CLOSE_HIT_PAD),
        y: icon.y.saturating_sub(CLOSE_HIT_PAD),
        width: icon.width.saturating_add(2 * CLOSE_HIT_PAD),
        height: icon.height.saturating_add(2 * CLOSE_HIT_PAD),
    }
}

/// Left proof/test panel (target tests + filter list). Wheel here scrolls the
/// filter list; a click here focuses the filter input.
pub(crate) fn proof_panel_rect() -> HitRect {
    HitRect {
        x: PROOF_PANEL_X,
        y: PROOF_PANEL_Y,
        width: crate::proof_panel_spec::PANEL_WIDTH as u32
            + crate::proof_panel_spec::PANEL_GAP as u32
            + crate::proof_panel_spec::FILTER_PANEL_WIDTH as u32,
        height: crate::proof_panel_spec::PANEL_HEIGHT as u32,
    }
}

/// Right-hand chat panel viewport (the scrollable message list). Kept distinct
/// from the proof panel so wheel events route to the control under the cursor.
pub(crate) fn chat_viewport_rect() -> HitRect {
    HitRect {
        x: CHAT_PANEL_X,
        y: CHAT_PANEL_Y,
        width: CHAT_PANEL_W,
        height: CHAT_PANEL_H,
    }
}

/// True when the cursor is over the glass button (hover highlight; never opens
/// the sidebar — only a click does).
pub(crate) fn hover_over_button(mode: VisibleBootstrapMode, x: i32, y: i32) -> bool {
    rect_contains(button_rect(mode.width), x, y)
}

/// Resolve a primary-button press to an action. `sidebar_open` is windowd's
/// current sidebar state. Order matters: the close icon and the button win over
/// the panel; a click outside an open sidebar dismisses it.
pub(crate) fn resolve_click(
    mode: VisibleBootstrapMode,
    sidebar_open: bool,
    x: i32,
    y: i32,
) -> ClickAction {
    if rect_contains(button_rect(mode.width), x, y) {
        return ClickAction::ToggleSidebar;
    }
    if sidebar_open {
        // Use the resting-open rect (translate 0) for the click hitbox.
        let open = sidebar_rect(mode, 0.0);
        if rect_contains(sidebar_close_hit_rect(mode, open), x, y) {
            return ClickAction::CloseSidebar;
        }
        if !rect_contains(open, x, y) {
            return ClickAction::CloseSidebar;
        }
        // Inside the open sidebar but not on the close icon: no-op.
        return ClickAction::None;
    }
    if rect_contains(proof_panel_rect(), x, y) {
        return ClickAction::FocusPanel;
    }
    ClickAction::None
}

/// Resolve which scrollable a wheel event belongs to, by the cursor position.
pub(crate) fn resolve_wheel_target(mode: VisibleBootstrapMode, x: i32, y: i32) -> WheelTarget {
    let _ = mode;
    if rect_contains(chat_viewport_rect(), x, y) {
        WheelTarget::Chat
    } else if rect_contains(proof_panel_rect(), x, y) {
        WheelTarget::Filter
    } else {
        WheelTarget::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode() -> VisibleBootstrapMode {
        VisibleBootstrapMode::fixed().expect("fixed mode")
    }

    #[test]
    fn button_rect_is_top_right_and_matches_render_consts() {
        let m = mode();
        let r = button_rect(m.width);
        // 1280 - (156 + 24) = 1100.
        assert_eq!(r.x, 1100);
        assert_eq!(r.y, GLASS_BUTTON_TOP);
        assert_eq!(r.width, GLASS_BUTTON_W);
        assert_eq!(r.height, GLASS_BUTTON_H);
    }

    #[test]
    fn hover_hit_area_equals_button_rect_edges() {
        let r = button_rect(mode().width);
        // Inclusive top-left corner is inside.
        assert!(hover_over_button(mode(), r.x as i32, r.y as i32));
        // Last inside pixel.
        assert!(hover_over_button(
            mode(),
            (r.x + r.width - 1) as i32,
            (r.y + r.height - 1) as i32
        ));
        // Exclusive far edges are outside (hit area == rect, no off-by-one).
        assert!(!hover_over_button(mode(), (r.x + r.width) as i32, r.y as i32));
        assert!(!hover_over_button(mode(), r.x as i32, (r.y + r.height) as i32));
        // One pixel left/above the rect is outside.
        assert!(!hover_over_button(mode(), r.x as i32 - 1, r.y as i32));
        assert!(!hover_over_button(mode(), r.x as i32, r.y as i32 - 1));
    }

    #[test]
    fn click_on_button_toggles_sidebar() {
        let r = button_rect(mode().width);
        let cx = (r.x + r.width / 2) as i32;
        let cy = (r.y + r.height / 2) as i32;
        assert_eq!(
            resolve_click(mode(), false, cx, cy),
            ClickAction::ToggleSidebar
        );
        assert_eq!(
            resolve_click(mode(), true, cx, cy),
            ClickAction::ToggleSidebar
        );
    }

    #[test]
    fn click_close_icon_closes_when_open() {
        let open = sidebar_rect(mode(), 0.0);
        let icon = sidebar_close_icon_rect(mode(), open);
        let cx = (icon.x + icon.width / 2) as i32;
        let cy = (icon.y + icon.height / 2) as i32;
        assert_eq!(
            resolve_click(mode(), true, cx, cy),
            ClickAction::CloseSidebar
        );
    }

    #[test]
    fn click_outside_open_sidebar_dismisses() {
        // Far top-left, away from button/sidebar/panel.
        assert_eq!(resolve_click(mode(), true, 10, 10), ClickAction::CloseSidebar);
    }

    #[test]
    fn click_proof_panel_focuses_when_closed() {
        let p = proof_panel_rect();
        let cx = (p.x + p.width / 2) as i32;
        let cy = (p.y + p.height / 2) as i32;
        assert_eq!(
            resolve_click(mode(), false, cx, cy),
            ClickAction::FocusPanel
        );
    }

    #[test]
    fn wheel_routes_to_control_under_cursor() {
        let chat = chat_viewport_rect();
        let p = proof_panel_rect();
        assert_eq!(
            resolve_wheel_target(mode(), (chat.x + 5) as i32, (chat.y + 5) as i32),
            WheelTarget::Chat
        );
        assert_eq!(
            resolve_wheel_target(mode(), (p.x + 5) as i32, (p.y + 5) as i32),
            WheelTarget::Filter
        );
        assert_eq!(resolve_wheel_target(mode(), 5, 5), WheelTarget::None);
    }

    #[test]
    fn chat_line_ranges_cover_the_whole_message_without_gaps() {
        let cpl = chat_chars_per_line();
        assert!(cpl >= 10, "cpl should be reasonable, got {cpl}");
        let len = cpl * 3 + 5; // 4 lines (3 full + remainder)
        assert_eq!(chat_message_lines(len, cpl), 4);
        // Line ranges tile [0, len) exactly, in order, no gaps/overlap.
        let mut expected_start = 0usize;
        for idx in 0..4u32 {
            let (s, e) = chat_line_char_range(len, cpl, idx).expect("line in range");
            assert_eq!(s, expected_start);
            assert!(e <= len && e >= s);
            assert!(e - s <= cpl);
            expected_start = e;
        }
        assert_eq!(expected_start, len, "lines must cover the whole message");
        assert_eq!(chat_line_char_range(len, cpl, 4), None);
    }

    #[test]
    fn chat_line_ranges_are_char_boundary_safe_for_multibyte() {
        // The message pool contains em-dashes (3-byte UTF-8). Char-based ranges
        // must let the renderer walk chars() without ever splitting a codepoint.
        let text = "Let me check that — need to reproduce it first, then I will report back here.";
        let cpl = chat_chars_per_line();
        let char_len = text.chars().count();
        let lines = chat_message_lines(char_len, cpl);
        // Walk exactly as the renderer does — must not panic on a multi-byte
        // boundary, and the per-line char ranges must tile the whole message.
        let mut expected = text.chars();
        for idx in 0..lines {
            let (cs, ce) = chat_line_char_range(char_len, cpl, idx).expect("line");
            for ch in text.chars().skip(cs).take(ce - cs) {
                assert_eq!(Some(ch), expected.next());
            }
        }
        assert_eq!(expected.next(), None, "char ranges must tile the message exactly");
    }

    #[test]
    fn chat_message_height_grows_with_lines() {
        let one = chat_message_height(1);
        let three = chat_message_height(3);
        assert!(three > one);
        assert_eq!(three, one + 2 * CHAT_LINE_H);
        // An empty/one-char message is still one line tall.
        assert_eq!(chat_message_lines(0, chat_chars_per_line()), 1);
        assert_eq!(chat_message_lines(1, chat_chars_per_line()), 1);
    }

    #[test]
    fn chat_and_proof_panels_do_not_overlap() {
        let chat = chat_viewport_rect();
        let p = proof_panel_rect();
        let p_right = p.x + p.width;
        assert!(
            chat.x >= p_right,
            "chat {} should start past panel right {}",
            chat.x,
            p_right
        );
    }
}
