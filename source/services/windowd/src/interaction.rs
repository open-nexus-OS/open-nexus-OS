// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Single source of truth for windowd's interactive geometry and the
//! pure hit-test logic over it. This is the compositor-owned hit-testing model
//! (the compositor window-server): the window server resolves
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
/// Chat body line height = the baked 16px face's line height (TASK-0070
/// Phase 6 — runtime glyphs replaced the 5×7 bitmap; the baked value is 20px,
/// identical to the old bitmap line, so the scroll/height math is unchanged).
pub(crate) const CHAT_LINE_H: u32 = crate::text::line_height(crate::text::FontSize::Body);
/// Vertical padding inside a message bubble (top and bottom).
pub(crate) const CHAT_MSG_PAD: u32 = 8;

/// Width in pixels available for chat text (panel minus padding and scrollbar).
pub(crate) fn chat_text_width() -> u32 {
    CHAT_PANEL_W.saturating_sub(CHAT_PAD.saturating_mul(2)).saturating_sub(CHAT_SCROLLBAR_W)
}

/// Rough characters-per-line estimate (average advance) — only feeds the chat
/// provider's internal height hint; the real wrap below is measured.
pub(crate) fn chat_chars_per_line() -> usize {
    (chat_text_width() / crate::text::avg_advance(crate::text::FontSize::Body)).max(1) as usize
}

/// Left inset of the text column inside the chat viewport (bubble inset + text
/// padding) — the renderer's `text_x` offset, subtracted from the wrap width.
pub(crate) const CHAT_TEXT_INSET: u32 = 10;

/// Pixel width the wrap walker fills — the wrap SSOT width (TASK-0070 Phase 7:
/// MEASURED word wrap at the 16px face replaces the char-count estimate).
pub(crate) fn chat_wrap_width() -> u32 {
    chat_text_width().saturating_sub(CHAT_TEXT_INSET)
}

/// One wrapped line starting at char index `start`: returns `(end, next_start)`
/// — the line is chars `[start, end)`, the next line begins at `next_start`
/// (the break space itself is consumed, not rendered). Greedy WORD wrap at the
/// 16px face's measured advances; a single word wider than `width` breaks
/// mid-word. Always consumes at least one char. Walking by chars keeps
/// multi-byte UTF-8 (em-dashes) boundary-safe.
fn chat_wrap_line_end(text: &str, width: u32, start: usize) -> (usize, usize) {
    let w = width.max(1) as i32;
    let mut pen = 0i32;
    let mut last_space: Option<usize> = None;
    let mut idx = start;
    for ch in text.chars().skip(start) {
        let adv = crate::text::advance(ch, crate::text::FontSize::Body) as i32;
        if pen + adv > w && idx > start {
            return match last_space {
                Some(s) if s > start => (s, s + 1),
                _ => (idx, idx),
            };
        }
        if ch == ' ' {
            last_space = Some(idx);
        }
        pen += adv;
        idx += 1;
    }
    (idx, idx)
}

/// Number of wrapped lines `text` occupies at the chat wrap width.
pub(crate) fn chat_wrap_lines(text: &str) -> u32 {
    let width = chat_wrap_width();
    let len = text.chars().count();
    let mut start = 0usize;
    let mut lines = 0u32;
    while start < len {
        let (_, next) = chat_wrap_line_end(text, width, start);
        start = next.max(start + 1);
        lines += 1;
    }
    lines.max(1)
}

/// Char range `[start, end)` of wrapped line `idx` of `text`, or `None` past
/// the last line. Walks the same `chat_wrap_line_end` as `chat_wrap_lines`, so
/// layout heights and painted slices can never drift.
pub(crate) fn chat_wrap_line_range(text: &str, idx: u32) -> Option<(usize, usize)> {
    let width = chat_wrap_width();
    let len = text.chars().count();
    let mut start = 0usize;
    let mut line = 0u32;
    while start < len {
        let (end, next) = chat_wrap_line_end(text, width, start);
        if line == idx {
            return Some((start, end));
        }
        start = next.max(start + 1);
        line += 1;
    }
    // An empty message still occupies one (blank) line.
    (idx == 0 && len == 0).then_some((0, 0))
}

/// Total block height of a message (its wrapped lines plus top/bottom padding).
pub(crate) fn chat_message_height(lines: u32) -> u32 {
    lines.saturating_mul(CHAT_LINE_H).saturating_add(CHAT_MSG_PAD.saturating_mul(2))
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
    /// Cursor is over the chat button → toggle the chat window open/closed.
    ToggleChat,
    /// Cursor is over the left proof/test panel → focus it (filter input).
    FocusPanel,
    /// Cursor is over the greeter's user avatar → log that user in
    /// (TASK-0065B; only reachable while the greeter owns the display).
    GreeterUser,
    /// Click landed on nothing interactive.
    None,
}

// (WheelTarget / resolve_wheel_target retired in TASK-0070 Phase 1: wheel
//  routing is now the z/focus stack's front-to-back hit order in
//  `window_scene::WindowStack::hit_order` — the same SSOT as presses — so the
//  TOPMOST window under the cursor scrolls, not a hardcoded chat-first rule.)

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

/// Chat window title bar height (drawn by `compositor::chat`, hit-tested by
/// `wm`) — single source so the drag/close zones always match the pixels.
pub(crate) const CHAT_TITLE_BAR_H: u32 = 40;
/// Close hit-zone width at the right end of the title bar (matches the X).
pub(crate) const CHAT_CLOSE_ZONE_W: u32 = 48;

/// Chat toggle button — a square glass button in the bottom-right corner
/// (clear of the chat window's default bounds and the hamburger button).
pub(crate) const CHAT_BUTTON_SIZE: u32 = 56;
pub(crate) const CHAT_BUTTON_MARGIN: u32 = 24;
pub(crate) const CHAT_BUTTON_RADIUS: u32 = 16;

pub(crate) fn chat_button_rect(width: u32, height: u32) -> HitRect {
    HitRect {
        x: width.saturating_sub(CHAT_BUTTON_SIZE.saturating_add(CHAT_BUTTON_MARGIN)),
        y: height.saturating_sub(CHAT_BUTTON_SIZE.saturating_add(CHAT_BUTTON_MARGIN)),
        width: CHAT_BUTTON_SIZE,
        height: CHAT_BUTTON_SIZE,
    }
}

/// Sidebar rect for a given horizontal `translate` (0 = fully open, SIDEBAR_WIDTH
/// = fully closed/offscreen). The renderer uses the identical expression.
pub(crate) fn sidebar_rect(mode: VisibleBootstrapMode, translate_x: f32) -> HitRect {
    let translate = translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32;
    let x = mode.width.saturating_sub(SIDEBAR_WIDTH).saturating_add(translate);
    let height =
        mode.height.saturating_sub(SIDEBAR_MARGIN_TOP.saturating_add(SIDEBAR_MARGIN_BOTTOM)).max(1);
    HitRect { x, y: SIDEBAR_MARGIN_TOP, width: SIDEBAR_WIDTH, height }
}

/// The rendered close (X) icon rect inside `sidebar`. Mirrors the renderer's
/// clamp so the icon never escapes the panel.
pub(crate) fn sidebar_close_icon_rect(mode: VisibleBootstrapMode, sidebar: HitRect) -> HitRect {
    let close_mid_x = route_cell_midpoint(CLOSE_TARGET_ROUTE_X, VISIBLE_ROUTE_WIDTH, mode.width);
    let close_mid_y = route_cell_midpoint(CLOSE_TARGET_ROUTE_Y, VISIBLE_ROUTE_HEIGHT, mode.height);
    let sidebar_end_x = sidebar.x.saturating_add(sidebar.width);
    let sidebar_end_y = sidebar.y.saturating_add(sidebar.height);
    let x = close_mid_x
        .saturating_sub(LUCIDE_ICON_SIZE / 2)
        .clamp(sidebar.x.saturating_add(14), sidebar_end_x.saturating_sub(LUCIDE_ICON_SIZE + 14));
    let y = close_mid_y
        .saturating_sub(LUCIDE_ICON_SIZE / 2)
        .clamp(sidebar.y.saturating_add(14), sidebar_end_y.saturating_sub(LUCIDE_ICON_SIZE + 14));
    HitRect { x, y, width: LUCIDE_ICON_SIZE, height: LUCIDE_ICON_SIZE }
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


/// Right-hand chat panel viewport (the scrollable message list). Kept distinct
/// from the proof panel so wheel events route to the control under the cursor.
pub(crate) fn chat_viewport_rect() -> HitRect {
    HitRect { x: CHAT_PANEL_X, y: CHAT_PANEL_Y, width: CHAT_PANEL_W, height: CHAT_PANEL_H }
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
    if rect_contains(chat_button_rect(mode.width, mode.height), x, y) {
        return ClickAction::ToggleChat;
    }
    // C1: the proof/target-test panel was deleted — no panel to focus.
    ClickAction::None
}

/// The greeter's clickable avatar card (circle + name label), centered on the
/// display. Geometry SSOT shared by the renderer and hit-testing; sized from
/// the SystemUI greeter config's avatar diameter (padding for label + hover).
pub(crate) fn greeter_avatar_rect(mode: VisibleBootstrapMode, avatar_diameter: u32) -> HitRect {
    // Card = circle + label band below; generous horizontal padding so the
    // name stays inside the hover/redraw region.
    let card_w = avatar_diameter.max(160) + 48;
    let card_h = avatar_diameter + 64;
    HitRect {
        x: mode.width.saturating_sub(card_w) / 2,
        y: mode.height.saturating_sub(card_h) / 2,
        width: card_w.min(mode.width),
        height: card_h.min(mode.height),
    }
}

/// Session-aware click resolution (TASK-0065B): while the greeter owns the
/// display ONLY the avatar is interactive — every shell affordance (sidebar,
/// chat, hotspot) is unreachable. This is windowd's pre-session launch gating,
/// host-tested. When no greeter is active it defers to [`resolve_click`].
pub(crate) fn resolve_click_session(
    mode: VisibleBootstrapMode,
    sidebar_open: bool,
    greeter_avatar: Option<HitRect>,
    x: i32,
    y: i32,
) -> ClickAction {
    match greeter_avatar {
        Some(rect) => {
            if rect_contains(rect, x, y) {
                ClickAction::GreeterUser
            } else {
                ClickAction::None
            }
        }
        None => resolve_click(mode, sidebar_open, x, y),
    }
}

/// Greeter hover feedback: true while the cursor is over the avatar card.
pub(crate) fn hover_over_greeter(rect: HitRect, x: i32, y: i32) -> bool {
    rect_contains(rect, x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode() -> VisibleBootstrapMode {
        VisibleBootstrapMode::fixed().expect("fixed mode")
    }

    #[test]
    fn greeter_click_hits_avatar_and_nothing_else() {
        let m = mode();
        let rect = greeter_avatar_rect(m, 96);
        let (cx, cy) = (
            (rect.x + rect.width / 2) as i32,
            (rect.y + rect.height / 2) as i32,
        );
        // Center of the card logs in.
        assert_eq!(
            resolve_click_session(m, false, Some(rect), cx, cy),
            ClickAction::GreeterUser
        );
        // The card is centered on the display.
        assert_eq!(rect.x + rect.width / 2, m.width / 2);
        // Outside the card: nothing (even on the sidebar button — the shell
        // affordances are unreachable while the greeter owns the display).
        let btn = button_rect(m.width);
        assert_eq!(
            resolve_click_session(
                m,
                false,
                Some(rect),
                (btn.x + btn.width / 2) as i32,
                (btn.y + btn.height / 2) as i32
            ),
            ClickAction::None
        );
        // Corner hotspot region is dead too.
        assert_eq!(
            resolve_click_session(m, false, Some(rect), 4, (m.height - 4) as i32),
            ClickAction::None
        );
    }

    #[test]
    fn greeter_gate_off_defers_to_shell_clicks() {
        let m = mode();
        let btn = button_rect(m.width);
        assert_eq!(
            resolve_click_session(
                m,
                false,
                None,
                (btn.x + btn.width / 2) as i32,
                (btn.y + btn.height / 2) as i32
            ),
            ClickAction::ToggleSidebar
        );
    }

    #[test]
    fn greeter_hover_tracks_avatar() {
        let m = mode();
        let rect = greeter_avatar_rect(m, 96);
        assert!(hover_over_greeter(
            rect,
            (rect.x + 1) as i32,
            (rect.y + 1) as i32
        ));
        assert!(!hover_over_greeter(rect, 0, 0));
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
        assert!(hover_over_button(mode(), (r.x + r.width - 1) as i32, (r.y + r.height - 1) as i32));
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
        assert_eq!(resolve_click(mode(), false, cx, cy), ClickAction::ToggleSidebar);
        assert_eq!(resolve_click(mode(), true, cx, cy), ClickAction::ToggleSidebar);
    }

    #[test]
    fn click_chat_button_toggles_chat_when_sidebar_closed() {
        let m = mode();
        let r = chat_button_rect(m.width, m.height);
        let cx = (r.x + r.width / 2) as i32;
        let cy = (r.y + r.height / 2) as i32;
        assert_eq!(resolve_click(mode(), false, cx, cy), ClickAction::ToggleChat);
        // One pixel outside misses.
        assert_eq!(resolve_click(mode(), false, (r.x + r.width) as i32, cy), ClickAction::None);
    }

    #[test]
    fn chat_button_sits_bottom_right_clear_of_chat_window() {
        let m = mode();
        let cb = chat_button_rect(m.width, m.height);
        // Bottom-right corner with margin.
        assert_eq!(cb.x + cb.width, m.width - CHAT_BUTTON_MARGIN);
        assert_eq!(cb.y + cb.height, m.height - CHAT_BUTTON_MARGIN);
        // Clear of the chat window's default bounds (no z-fight with the
        // title bar / close X).
        assert!(cb.y >= CHAT_PANEL_Y + CHAT_PANEL_H);
    }

    #[test]
    fn click_close_icon_closes_when_open() {
        let open = sidebar_rect(mode(), 0.0);
        let icon = sidebar_close_icon_rect(mode(), open);
        let cx = (icon.x + icon.width / 2) as i32;
        let cy = (icon.y + icon.height / 2) as i32;
        assert_eq!(resolve_click(mode(), true, cx, cy), ClickAction::CloseSidebar);
    }

    #[test]
    fn click_outside_open_sidebar_dismisses() {
        // Far top-left, away from button/sidebar/panel.
        assert_eq!(resolve_click(mode(), true, 10, 10), ClickAction::CloseSidebar);
    }

    // (wheel-routing tests moved to `window_scene` — the stack's hit_order is
    //  the wheel-target SSOT since TASK-0070 Phase 1.)

    #[test]
    fn chat_wrap_ranges_tile_the_message_and_break_at_words() {
        let text = "The quick brown fox jumps over the lazy dog while the compositor keeps every frame at a steady cadence without dropping input events.";
        let lines = chat_wrap_lines(text);
        assert!(lines >= 2, "long message wraps ({lines} lines)");
        // Ranges are in order; consecutive lines are contiguous up to ONE
        // consumed break space; every rendered line fits the wrap width.
        let width = chat_wrap_width() as i32;
        let chars: alloc::vec::Vec<char> = text.chars().collect();
        let mut prev_end = 0usize;
        for idx in 0..lines {
            let (s, e) = chat_wrap_line_range(text, idx).expect("line in range");
            assert!(s >= prev_end && s <= prev_end + 1, "≤1 skipped break space");
            assert!(e > s, "non-empty line");
            let w: i32 = chars[s..e]
                .iter()
                .map(|&c| crate::text::advance(c, crate::text::FontSize::Body) as i32)
                .sum();
            assert!(w <= width, "line {idx} measures {w} > wrap width {width}");
            // Word wrap: a line broken at a space never starts with a space.
            assert_ne!(chars[s], ' ', "line {idx} starts on the skipped space");
            prev_end = e;
        }
        assert!(prev_end == chars.len(), "lines cover the whole message");
        assert_eq!(chat_wrap_line_range(text, lines), None);
    }

    #[test]
    fn chat_wrap_is_char_boundary_safe_for_multibyte() {
        // The message pool contains em-dashes (3-byte UTF-8). Char-based ranges
        // must let the renderer walk chars() without ever splitting a codepoint.
        let text = "Let me check that — need to reproduce it first, then I will report back here — thanks a lot for the very detailed reproduction steps.";
        let lines = chat_wrap_lines(text);
        let mut expected = text.chars().filter(|&c| c != ' ');
        for idx in 0..lines {
            let (cs, ce) = chat_wrap_line_range(text, idx).expect("line");
            for ch in text.chars().skip(cs).take(ce - cs).filter(|&c| c != ' ') {
                assert_eq!(Some(ch), expected.next());
            }
        }
        assert_eq!(expected.next(), None, "all non-space chars appear exactly once");
    }

    #[test]
    fn chat_wrap_breaks_overlong_words_mid_word() {
        // A single "word" wider than the viewport must still wrap (no infinite
        // loop, no zero-length line).
        let text = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let lines = chat_wrap_lines(text);
        assert!(lines >= 2, "overlong word wraps mid-word ({lines})");
        let (s0, e0) = chat_wrap_line_range(text, 0).expect("first line");
        assert!(e0 > s0);
    }

    #[test]
    fn chat_message_height_grows_with_lines() {
        let one = chat_message_height(1);
        let three = chat_message_height(3);
        assert!(three > one);
        assert_eq!(three, one + 2 * CHAT_LINE_H);
        // An empty/short message is still one line tall.
        assert_eq!(chat_wrap_lines(""), 1);
        assert_eq!(chat_wrap_lines("a"), 1);
    }
}
