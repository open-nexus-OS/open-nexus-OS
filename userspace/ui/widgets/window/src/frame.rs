// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pure window-frame geometry shared by every window instance — the
//! host-testable SSOT for hit-testing, drag clamping, and damage rects. Window
//! *state* (glass/blur/atlas, present) is a compositor concern and lives in the
//! display server; this is only the geometry, so a window manager and N windows
//! reuse one implementation (RFC-0067 P3: window geometry is a widget concern,
//! not a compositor one).
//!
//! Pure logic, no OS or rendering deps → fully host-testable.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests

/// What a primary press landed on inside a window (window-local resolution).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowPress {
    /// The minimize "–" in the title bar (window hides into the dock).
    Minimize,
    /// The maximize "□" in the title bar (toggle fullscreen).
    Maximize,
    /// The close "x" in the title bar.
    Close,
    /// The title bar (outside the buttons) — begins a drag.
    TitleDrag,
    /// The window body (below the title bar).
    Body,
    /// Outside the window.
    Miss,
}

/// The three title-bar buttons, right-aligned in the order `[– □ ×]` (each
/// `close_w` wide). Shared by the hit-tester and the renderer so the hover
/// highlight and the press resolution can never disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitleButton {
    Minimize,
    Maximize,
    Close,
}

impl TitleButton {
    /// Zone index from the RIGHT edge (close = 0, maximize = 1, minimize = 2).
    pub fn zone_from_right(self) -> u32 {
        match self {
            TitleButton::Close => 0,
            TitleButton::Maximize => 1,
            TitleButton::Minimize => 2,
        }
    }
}

/// Width of the edge/corner resize hit band, in px INSIDE the window border
/// (TASK-0070 Phase 3). Corners are the intersection of two bands.
pub const RESIZE_BORDER: u32 = 7;

/// Which window edge/corner a resize drag grabs. Determines both the resize
/// math and the pointer shape (`ew`/`ns`/`nesw`/`nwse`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl ResizeEdge {
    /// Whether this edge moves the LEFT border (x + w change together).
    pub fn affects_left(self) -> bool {
        matches!(self, ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft)
    }
    /// Whether this edge moves the RIGHT border (w changes).
    pub fn affects_right(self) -> bool {
        matches!(self, ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight)
    }
    /// Whether this edge moves the TOP border (y + h change together).
    pub fn affects_top(self) -> bool {
        matches!(self, ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight)
    }
    /// Whether this edge moves the BOTTOM border (h changes).
    pub fn affects_bottom(self) -> bool {
        matches!(self, ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight)
    }
}

/// A window's display-space rectangle plus its chrome geometry. Signed origin so a
/// window dragged partly off-screen is representable before clamping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// Title-bar height (drag handle) and close-button zone width.
    pub title_h: u32,
    pub close_w: u32,
}

impl Frame {
    /// True if `(cx, cy)` is anywhere inside the window.
    pub fn contains(&self, cx: i32, cy: i32) -> bool {
        cx >= self.x && cx < self.x + self.w as i32 && cy >= self.y && cy < self.y + self.h as i32
    }

    /// True if `cy` falls within the title-bar band.
    pub fn in_title_bar(&self, cy: i32) -> bool {
        cy >= self.y && cy < self.y + self.title_h as i32
    }

    /// Window-local x of a title button's zone (its LEFT edge). Buttons are
    /// right-aligned `[– □ ×]`, each `close_w` wide — one geometry for the
    /// hit-test AND the renderer.
    pub fn button_local_x(&self, button: TitleButton) -> u32 {
        self.w.saturating_sub(self.close_w.saturating_mul(button.zone_from_right() + 1))
    }

    /// The title button under `(cx, cy)`, if any.
    pub fn title_button_at(&self, cx: i32, cy: i32) -> Option<TitleButton> {
        if !self.in_title_bar(cy) || !self.contains(cx, cy) {
            return None;
        }
        for button in [TitleButton::Minimize, TitleButton::Maximize, TitleButton::Close] {
            let bx = self.x + self.button_local_x(button) as i32;
            if cx >= bx && cx < bx + self.close_w as i32 {
                return Some(button);
            }
        }
        None
    }

    /// True if `(cx, cy)` is over the close "x" at the title bar's right edge.
    pub fn close_hit(&self, cx: i32, cy: i32) -> bool {
        self.title_button_at(cx, cy) == Some(TitleButton::Close)
    }

    /// Resolve a primary press to a window region. The title buttons win over
    /// the title bar; the title bar begins a drag; the rest is the body;
    /// outside is a miss.
    pub fn press(&self, cx: i32, cy: i32) -> WindowPress {
        if !self.contains(cx, cy) {
            return WindowPress::Miss;
        }
        if self.in_title_bar(cy) {
            match self.title_button_at(cx, cy) {
                Some(TitleButton::Minimize) => WindowPress::Minimize,
                Some(TitleButton::Maximize) => WindowPress::Maximize,
                Some(TitleButton::Close) => WindowPress::Close,
                None => WindowPress::TitleDrag,
            }
        } else {
            WindowPress::Body
        }
    }

    /// The resize edge/corner under `(cx, cy)` — a `RESIZE_BORDER`-wide band
    /// just inside each window border; corners where two bands meet. Resolved
    /// BEFORE title-bar presses (the top band overlaps the title bar's first
    /// pixels), so grabbing the very edge resizes instead of dragging.
    pub fn resize_edge_at(&self, cx: i32, cy: i32) -> Option<ResizeEdge> {
        if !self.contains(cx, cy) {
            return None;
        }
        let b = RESIZE_BORDER as i32;
        let left = cx < self.x + b;
        let right = cx >= self.x + self.w as i32 - b;
        let top = cy < self.y + b;
        let bottom = cy >= self.y + self.h as i32 - b;
        match (left, right, top, bottom) {
            (true, _, true, _) => Some(ResizeEdge::TopLeft),
            (_, true, true, _) => Some(ResizeEdge::TopRight),
            (true, _, _, true) => Some(ResizeEdge::BottomLeft),
            (_, true, _, true) => Some(ResizeEdge::BottomRight),
            (true, _, _, _) => Some(ResizeEdge::Left),
            (_, true, _, _) => Some(ResizeEdge::Right),
            (_, _, true, _) => Some(ResizeEdge::Top),
            (_, _, _, true) => Some(ResizeEdge::Bottom),
            _ => None,
        }
    }

    /// Pure resize math: the frame that results from dragging `edge` by
    /// `(dx, dy)` from `start`, clamped to `min_w`/`min_h` and the display.
    /// Deterministic in the drag START frame (not incremental), so a fast
    /// pointer never accumulates rounding drift.
    pub fn resized(
        start: Frame,
        edge: ResizeEdge,
        dx: i32,
        dy: i32,
        min_w: u32,
        min_h: u32,
        mode_w: u32,
        mode_h: u32,
    ) -> Frame {
        let mut x0 = start.x;
        let mut y0 = start.y;
        let mut x1 = start.x + start.w as i32;
        let mut y1 = start.y + start.h as i32;
        if edge.affects_left() {
            x0 = (start.x + dx).clamp(0, x1 - min_w as i32);
        }
        if edge.affects_right() {
            x1 = (x1 + dx).clamp(x0 + min_w as i32, mode_w as i32);
        }
        if edge.affects_top() {
            y0 = (start.y + dy).clamp(0, y1 - min_h as i32);
        }
        if edge.affects_bottom() {
            y1 = (y1 + dy).clamp(y0 + min_h as i32, mode_h as i32);
        }
        Frame {
            x: x0,
            y: y0,
            w: (x1 - x0).max(min_w as i32) as u32,
            h: (y1 - y0).max(min_h as i32) as u32,
            title_h: start.title_h,
            close_w: start.close_w,
        }
    }

    /// Clamp a dragged top-left (`nx, ny`) so the window stays fully on the
    /// display. Returns the clamped origin.
    pub fn clamp_pos(&self, nx: i32, ny: i32, mode_w: u32, mode_h: u32) -> (i32, i32) {
        let max_x = mode_w.saturating_sub(self.w) as i32;
        let max_y = mode_h.saturating_sub(self.h) as i32;
        (nx.clamp(0, max_x.max(0)), ny.clamp(0, max_y.max(0)))
    }

    /// Damage rect `(x, y, w, h)` of the window grown by `pad` on every side (the
    /// soft drop-shadow halo), clipped to the display.
    pub fn damage_bounds(&self, pad: u32, mode_w: u32, mode_h: u32) -> (u32, u32, u32, u32) {
        let x = (self.x.max(0) as u32).saturating_sub(pad);
        let y = (self.y.max(0) as u32).saturating_sub(pad);
        let w = (self.w + 2 * pad).min(mode_w.saturating_sub(x));
        let h = (self.h + 2 * pad).min(mode_h.saturating_sub(y));
        (x, y, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODE_W: u32 = 1280;
    const MODE_H: u32 = 800;
    fn frame() -> Frame {
        Frame { x: 890, y: 96, w: 366, h: 600, title_h: 40, close_w: 48 }
    }

    #[test]
    fn contains_matches_rect_bounds() {
        let f = frame();
        assert!(f.contains(f.x, f.y));
        assert!(f.contains(f.x + f.w as i32 - 1, f.y + f.h as i32 - 1));
        assert!(!f.contains(f.x - 1, f.y));
        assert!(!f.contains(f.x + f.w as i32, f.y));
        assert!(!f.contains(f.x, f.y + f.h as i32));
    }

    #[test]
    fn close_zone_is_right_of_title_bar_only() {
        let f = frame();
        let close_x = f.x + f.w as i32 - f.close_w as i32 + 1;
        assert!(f.close_hit(close_x, f.y + 1));
        // Just below the title bar → not the close zone.
        assert!(!f.close_hit(close_x, f.y + f.title_h as i32));
        // Left of the close zone but in the title bar → not close.
        assert!(!f.close_hit(f.x + 1, f.y + 1));
    }

    #[test]
    fn press_resolves_close_drag_body_miss() {
        let f = frame();
        assert_eq!(f.press(f.x - 5, f.y), WindowPress::Miss);
        assert_eq!(f.press(f.x + 10, f.y + 5), WindowPress::TitleDrag);
        let close_x = f.x + f.w as i32 - 10;
        assert_eq!(f.press(close_x, f.y + 5), WindowPress::Close);
        assert_eq!(f.press(f.x + 10, f.y + f.title_h as i32 + 10), WindowPress::Body);
    }

    #[test]
    fn title_buttons_are_right_aligned_minus_square_x() {
        let f = frame();
        let bw = f.close_w as i32;
        let right = f.x + f.w as i32;
        let cy = f.y + 5;
        // Sample the CENTER of each zone, right-to-left: × □ –
        assert_eq!(f.press(right - bw / 2, cy), WindowPress::Close);
        assert_eq!(f.press(right - bw - bw / 2, cy), WindowPress::Maximize);
        assert_eq!(f.press(right - 2 * bw - bw / 2, cy), WindowPress::Minimize);
        // Left of all three zones → drag.
        assert_eq!(f.press(right - 3 * bw - 5, cy), WindowPress::TitleDrag);
        // Below the title bar in the button column → body, not a button.
        assert_eq!(f.press(right - bw / 2, f.y + f.title_h as i32 + 2), WindowPress::Body);
    }

    #[test]
    fn title_button_at_matches_press_and_renderer_geometry() {
        let f = frame();
        let bw = f.close_w;
        // Renderer geometry: zone left edges from the right edge.
        assert_eq!(f.button_local_x(TitleButton::Close), f.w - bw);
        assert_eq!(f.button_local_x(TitleButton::Maximize), f.w - 2 * bw);
        assert_eq!(f.button_local_x(TitleButton::Minimize), f.w - 3 * bw);
        // Hover resolution agrees with press resolution.
        let cy = f.y + 5;
        let max_cx = f.x + (f.w - 2 * bw) as i32 + 3;
        assert_eq!(f.title_button_at(max_cx, cy), Some(TitleButton::Maximize));
        assert_eq!(f.title_button_at(f.x + 5, cy), None);
        // Outside the title bar → no button.
        assert_eq!(f.title_button_at(max_cx, f.y + f.title_h as i32), None);
    }

    #[test]
    fn clamp_keeps_window_on_display() {
        let f = frame();
        assert_eq!(f.clamp_pos(-500, -500, MODE_W, MODE_H), (0, 0));
        let (cx, cy) = f.clamp_pos(99999, 99999, MODE_W, MODE_H);
        assert_eq!(cx, (MODE_W - f.w) as i32);
        assert_eq!(cy, (MODE_H - f.h) as i32);
        assert!(cx + f.w as i32 <= MODE_W as i32);
        assert!(cy + f.h as i32 <= MODE_H as i32);
        // An in-bounds move is unchanged.
        assert_eq!(f.clamp_pos(100, 100, MODE_W, MODE_H), (100, 100));
    }

    #[test]
    fn resize_edges_resolve_bands_and_corners() {
        let f = frame();
        let b = RESIZE_BORDER as i32;
        // Band centers on each edge.
        assert_eq!(f.resize_edge_at(f.x + 2, f.y + f.h as i32 / 2), Some(ResizeEdge::Left));
        assert_eq!(
            f.resize_edge_at(f.x + f.w as i32 - 2, f.y + f.h as i32 / 2),
            Some(ResizeEdge::Right)
        );
        assert_eq!(f.resize_edge_at(f.x + f.w as i32 / 2, f.y + 2), Some(ResizeEdge::Top));
        assert_eq!(
            f.resize_edge_at(f.x + f.w as i32 / 2, f.y + f.h as i32 - 2),
            Some(ResizeEdge::Bottom)
        );
        // Corners where two bands intersect.
        assert_eq!(f.resize_edge_at(f.x + 2, f.y + 2), Some(ResizeEdge::TopLeft));
        assert_eq!(
            f.resize_edge_at(f.x + f.w as i32 - 2, f.y + f.h as i32 - 2),
            Some(ResizeEdge::BottomRight)
        );
        // Interior (past the band) is not a resize.
        assert_eq!(f.resize_edge_at(f.x + b + 5, f.y + f.h as i32 / 2), None);
        // Outside the window is not a resize.
        assert_eq!(f.resize_edge_at(f.x - 1, f.y), None);
        // The top band WINS over the title bar (edge grab resizes, not drags).
        assert_eq!(f.resize_edge_at(f.x + f.w as i32 / 2, f.y + 1), Some(ResizeEdge::Top));
    }

    #[test]
    fn resized_moves_only_the_grabbed_border_and_clamps() {
        let start = frame(); // x=890 y=96 w=366 h=600 (right edge at 1256)
                             // Drag the right edge +20: width grows, origin unchanged (1276 ≤ 1280).
        let r = Frame::resized(start, ResizeEdge::Right, 20, 0, 100, 100, MODE_W, MODE_H);
        assert_eq!((r.x, r.y, r.w, r.h), (start.x, start.y, start.w + 20, start.h));
        // Drag the left edge +50: x moves right, width shrinks.
        let l = Frame::resized(start, ResizeEdge::Left, 50, 0, 100, 100, MODE_W, MODE_H);
        assert_eq!((l.x, l.w), (start.x + 50, start.w - 50));
        // Corner drag changes both axes.
        let br = Frame::resized(start, ResizeEdge::BottomRight, 20, 30, 100, 100, MODE_W, MODE_H);
        assert_eq!((br.w, br.h), (start.w + 20, start.h + 30));
        // Min size clamps: shrinking below min stops at min.
        let tiny = Frame::resized(start, ResizeEdge::Right, -9000, 0, 120, 100, MODE_W, MODE_H);
        assert_eq!(tiny.w, 120);
        assert_eq!(tiny.x, start.x);
        // Display clamps: growing past the display stops at its border.
        let huge = Frame::resized(start, ResizeEdge::Right, 9000, 0, 100, 100, MODE_W, MODE_H);
        assert_eq!(huge.x + huge.w as i32, MODE_W as i32);
        // Deterministic in the start frame: same delta twice = same result.
        let again = Frame::resized(start, ResizeEdge::Right, 20, 0, 100, 100, MODE_W, MODE_H);
        assert_eq!(r, again);
    }

    #[test]
    fn damage_bounds_grows_by_pad_and_clips() {
        let f = Frame { x: 10, y: 10, w: 100, h: 100, title_h: 40, close_w: 48 };
        let (x, y, w, h) = f.damage_bounds(24, MODE_W, MODE_H);
        assert_eq!((x, y), (0, 0)); // 10 - 24 saturates to 0
        assert_eq!(w, 100 + 48); // grown by 2*pad, fits on screen
        assert_eq!(h, 100 + 48);
    }
}
