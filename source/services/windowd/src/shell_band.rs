// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: which pointer positions belong to the SHELL above every app window
//! (TASK-0067). The scene composites these of the desktop surface over the
//! windows: the top-bar strip, the shell's PANEL-level glass (the Control
//! Center, notifications, calendar and the search drop-downs — scene pass 2b)
//! and the layers it declares in front (its toast — pass 2c, RFC-0109 §motion
//! layers); while the shell holds a modal, or a front layer's backdrop spans
//! the screen, the whole surface is the input owner. A front layer that is
//! leaving (TASK-0354 P9) owns nothing — neither its rect nor its scrim: the
//! pointer is free the moment it closes.
//! Input must agree with that paint: before this rule a press on a Control
//! Center panel lying over a window reached the WINDOW beneath, and a search
//! panel over a window could be seen but not used. One pure rule, used by the
//! press, hover and wheel routing alike.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

use nexus_display_proto::surface_layers::MotionLayerDesc;

/// A rect in display space: `(x, y, width, height)`.
pub type Rect = (i32, i32, u32, u32);

/// The rects of the layers the shell declared in front that own the pointer, written to the
/// start of `out` (as many as fit), and whether one of them spans its backdrop (a scrim owns
/// every point). A leaving layer owns nothing: its rect and its scrim free the pointer at once.
#[must_use]
pub fn front_claims(motions: &[MotionLayerDesc], out: &mut [Rect]) -> (usize, bool) {
    let mut n = 0;
    let mut scrim = false;
    for m in motions.iter().filter(|m| m.front && !m.exit) {
        if let Some(slot) = out.get_mut(n) {
            *slot = (i32::from(m.x), i32::from(m.y), u32::from(m.w), u32::from(m.h));
            n += 1;
        }
        scrim |= m.backdrop().is_some();
    }
    (n, scrim)
}

/// Whether a pointer at `(x, y)` belongs to the shell above every app window:
/// anywhere while the shell holds a modal (a system modal captures input — its
/// backdrop is where an outside press dismisses it), the top-bar strip
/// (`y < topbar_h`), or inside one of the panel rects the scene composites
/// above the windows.
#[must_use]
pub fn shell_owns_point(x: i32, y: i32, topbar_h: u32, panels: &[Rect], shell_modal: bool) -> bool {
    if shell_modal || y < i32::try_from(topbar_h).unwrap_or(i32::MAX) {
        return true;
    }
    panels.iter().any(|&(px, py, w, h)| {
        let (w, h) = (i32::try_from(w).unwrap_or(i32::MAX), i32::try_from(h).unwrap_or(i32::MAX));
        x >= px && y >= py && x < px.saturating_add(w) && y < py.saturating_add(h)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: u32 = 36;

    #[test]
    fn the_top_bar_strip_is_the_shells() {
        assert!(shell_owns_point(600, 0, BAR, &[], false));
        assert!(shell_owns_point(600, 35, BAR, &[], false));
        assert!(!shell_owns_point(600, 36, BAR, &[], false), "below the bar: the window's");
    }

    #[test]
    fn a_panel_above_the_windows_takes_its_own_rect_only() {
        let cc = [(900, 44, 368, 420)];
        assert!(shell_owns_point(900, 44, BAR, &cc, false), "top-left corner inside");
        assert!(shell_owns_point(1267, 463, BAR, &cc, false), "bottom-right corner inside");
        assert!(!shell_owns_point(1268, 300, BAR, &cc, false), "one past the right edge");
        assert!(!shell_owns_point(899, 300, BAR, &cc, false), "one before the left edge");
        assert!(!shell_owns_point(400, 400, BAR, &cc, false), "elsewhere: the window's");
    }

    #[test]
    fn a_shell_modal_captures_every_point() {
        assert!(shell_owns_point(400, 400, BAR, &[], true));
        assert!(shell_owns_point(0, 799, BAR, &[], true));
    }

    /// TASK-0354 P9: a front layer that is leaving — a toast, a modal over its scrim — owns no
    /// pointer: the window beneath takes the press while the layer plays its exit.
    #[test]
    fn test_reject_a_pointer_for_a_leaving_front_layer() {
        let toast =
            MotionLayerDesc { x: 16, y: 700, w: 320, h: 64, front: true, ..MotionLayerDesc::EMPTY };
        let modal = MotionLayerDesc {
            x: 480,
            y: 300,
            w: 320,
            h: 160,
            front: true,
            backdrop: 1,
            backdrop_opacity: 102,
            ..MotionLayerDesc::EMPTY
        };
        let mut out = [(0, 0, 0, 0); 4];
        let (n, scrim) = front_claims(&[toast, modal], &mut out);
        assert_eq!((n, scrim), (2, true), "shown: both rects, the scrim owns every point");
        assert!(shell_owns_point(20, 710, BAR, &out[..n], scrim));
        let leaving =
            [MotionLayerDesc { exit: true, ..toast }, MotionLayerDesc { exit: true, ..modal }];
        let (n, scrim) = front_claims(&leaving, &mut out);
        assert_eq!((n, scrim), (0, false), "leaving: no rect, no scrim");
        assert!(
            !shell_owns_point(20, 710, BAR, &out[..n], scrim),
            "the toast's rect: the window's"
        );
        assert!(
            !shell_owns_point(100, 100, BAR, &out[..n], scrim),
            "under the scrim: the window's"
        );
        let (n, scrim) = front_claims(&[leaving[0], modal], &mut out);
        assert_eq!((n, scrim), (1, true), "one leaving, one shown: the shown one's alone");
        let behind = MotionLayerDesc { front: false, ..toast };
        assert_eq!(front_claims(&[behind], &mut out), (0, false), "not in front: the surface's");
        assert_eq!(
            front_claims(&[toast, modal], &mut []),
            (0, true),
            "no room: the scrim still owns"
        );
    }

    #[test]
    fn test_reject_degenerate_panels() {
        assert!(!shell_owns_point(10, 100, BAR, &[(10, 100, 0, 50)], false), "zero width");
        assert!(!shell_owns_point(10, 100, BAR, &[(10, 100, 50, 0)], false), "zero height");
        // A huge rect saturates instead of wrapping into a negative range.
        assert!(shell_owns_point(
            i32::MAX - 1,
            100,
            BAR,
            &[(i32::MAX - 2, 0, u32::MAX, 200)],
            false
        ));
    }
}
