// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a client surface is on the screen — the ONE place both the scene (where it
//! draws the surface's pixels) and a present's damage (where the changed pixels are) take it
//! from. A present's rectangles were put at the window's shadow-halo rectangle (the frame plus
//! 75 px each way) while the scene drew the surface at the frame's corner: every rectangle landed
//! up to 75 px up and to the left of what changed, a calculator digit's damage shared no pixel
//! with the digit, and the display's planes showed it only by accident (board--2026-11-19T09-
//! 18-34; TASK-0351 P18's flickering window bottoms). Host-tested here.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

/// A rectangle `(x, y, width, height)` in pixels.
pub(crate) type Rect = (u32, u32, u32, u32);

/// Where a surface's pixel (0, 0) is drawn and the extent of the frame it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SurfacePlace {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl SurfacePlace {
    /// A window's surface: drawn at its frame's corner, cut to the display's left and top edge
    /// (the scene draws a window moved past them from the edge).
    pub(crate) fn of(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x: x.max(0) as u32, y: y.max(0) as u32, w, h }
    }

    /// The desktop's surface: the whole display.
    pub(crate) const fn display((w, h): (u32, u32)) -> Self {
        Self { x: 0, y: 0, w, h }
    }

    /// Where the surface's pixel (0, 0) is on the screen.
    pub(crate) const fn origin(&self) -> (u32, u32) {
        (self.x, self.y)
    }

    /// Surface rectangle `r` on a display `(dw, dh)`: moved by the origin and cut at the frame
    /// and at the display — never moved anywhere else, never past the display. `None` when
    /// nothing of it is left.
    pub(crate) fn on_screen(&self, r: Rect, (dw, dh): (u32, u32)) -> Option<Rect> {
        let (x0, y0) = (self.x.saturating_add(r.0), self.y.saturating_add(r.1));
        let x1 = x0.saturating_add(r.2).min(self.x.saturating_add(self.w)).min(dw);
        let y1 = y0.saturating_add(r.3).min(self.y.saturating_add(self.h)).min(dh);
        (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: (u32, u32) = (1920, 1080);

    fn inside(p: (u32, u32), r: Rect) -> bool {
        p.0 >= r.0 && p.1 >= r.1 && p.0 < r.0 + r.2 && p.1 < r.1 + r.3
    }

    /// For every window place and every rectangle the app may name, the damage covers exactly
    /// the pixels the scene draws that rectangle at (on the display and in the frame).
    #[test]
    fn a_presents_damage_lands_where_the_scene_draws_it() {
        let windows = [(-20, -10), (0, 0), (10, 10), (74, 74), (75, 75), (598, 62), (1800, 1000)];
        let rects = [(0, 0, 1, 1), (12, 30, 41, 72), (300, 2, 40, 40), (0, 590, 340, 10)];
        for &(wx, wy) in &windows {
            let place = SurfacePlace::of(wx, wy, 340, 600);
            let (ox, oy) = place.origin();
            assert_eq!((ox, oy), (wx.max(0) as u32, wy.max(0) as u32), "the scene's origin");
            for &r in &rects {
                let on = place.on_screen(r, DISPLAY);
                for y in r.1..r.1 + r.3 {
                    for x in (r.0..r.0 + r.2).step_by(7) {
                        let p = (ox + x, oy + y);
                        let shown = p.0 < DISPLAY.0 && p.1 < DISPLAY.1 && x < 340 && y < 600;
                        assert_eq!(
                            on.is_some_and(|o| inside(p, o)),
                            shown,
                            "{wx},{wy} {r:?} {p:?}"
                        );
                    }
                }
            }
        }
    }

    /// The board's digit tap: the window at (598, 62), the digit's 41x72 rectangle. Put at the
    /// shadow halo (the old origin) it shared no pixel with the digit; put at the surface it is
    /// the digit.
    #[test]
    fn the_boards_digit_tap_is_named_where_it_is() {
        let (digit, halo) = ((258u32, 46u32, 41u32, 72u32), 75u32);
        let place = SurfacePlace::of(598, 62, 340, 600);
        let drawn = (598 + digit.0, 62 + digit.1, digit.2, digit.3);
        let old = (598 - halo + digit.0, 62u32.saturating_sub(halo) + digit.1, digit.2, digit.3);
        let overlap = |a: Rect, b: Rect| {
            let w = (a.0 + a.2).min(b.0 + b.2).saturating_sub(a.0.max(b.0));
            let h = (a.1 + a.3).min(b.1 + b.3).saturating_sub(a.1.max(b.1));
            u64::from(w) * u64::from(h)
        };
        assert_eq!(overlap(old, drawn), 0, "the halo origin missed the digit whole");
        assert_eq!(place.on_screen(digit, DISPLAY), Some(drawn));
        assert_eq!(overlap(drawn, drawn), 41 * 72);
    }

    /// A rectangle past the frame or the display names nothing there; one across the display's
    /// edge is cut to it; edges near the integer's end saturate instead of wrapping.
    #[test]
    fn test_reject_damage_past_the_frame_and_the_display() {
        let place = SurfacePlace::of(100, 100, 340, 600);
        assert_eq!(place.on_screen((340, 0, 10, 10), DISPLAY), None, "right of the frame");
        assert_eq!(place.on_screen((0, 600, 10, 10), DISPLAY), None, "below the frame");
        let edge = SurfacePlace::of(1900, 1070, 340, 600);
        assert_eq!(edge.on_screen((0, 0, 100, 100), DISPLAY), Some((1900, 1070, 20, 10)));
        assert_eq!(edge.on_screen((u32::MAX - 1, 0, 5, 5), DISPLAY), None, "saturates");
        assert_eq!(
            SurfacePlace::display(DISPLAY).on_screen((0, 0, 4000, 4000), DISPLAY),
            Some((0, 0, 1920, 1080))
        );
    }
}
