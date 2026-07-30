// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pure pager decisions for `.scroll(paged)` viewports (the
//! launcher pager, design_handoff_launcher §11): which page a wheel notch
//! turns to, and where a page index sits in pixels. `probe/scroll.rs` owns
//! the physics + lock TIMING; the DECISION lives here so host tests can
//! reach it (`probe/` is RISC-V-only — the `hover_wash` pattern).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (turn/lock/clamp/polarity).

/// A wheel notch's page-turn decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageTurn {
    /// The new glide target in px (a whole-page multiple, clamped to max_x).
    pub target_px: i32,
    /// +1 = next page, −1 = previous (the trigger the caller fires).
    pub dir: i32,
}

/// The page index a pixel target sits on (nearest page).
pub(crate) fn page_of(target_px: i32, view_w: i32) -> i32 {
    if view_w <= 0 {
        return 0;
    }
    (target_px + view_w / 2) / view_w
}

/// The pixel offset of page `i`, clamped to the scrollable extent.
pub(crate) fn page_target_px(page: i32, view_w: i32, max_x: i32) -> i32 {
    (page.max(0)).saturating_mul(view_w.max(0)).min(max_x.max(0))
}

/// One wheel notch over a paged viewport: ±1 page from the CURRENT glide
/// target (never from the eased position — a mid-glide notch turns from
/// where the glide is heading), clamped to the page range. `None` = no turn
/// (locked, at an edge, zero delta, or nothing to page).
///
/// Polarity matches vertical scrolling: wheel DOWN (toward the user,
/// negative notch) = NEXT page.
pub(crate) fn page_turn(
    target_px_now: i32,
    view_w: i32,
    max_x: i32,
    delta_notches: i32,
    locked: bool,
) -> Option<PageTurn> {
    if locked || delta_notches == 0 || view_w <= 0 || max_x <= 0 {
        return None;
    }
    let dir: i32 = if delta_notches < 0 { 1 } else { -1 };
    let cur = page_of(target_px_now, view_w);
    let last = (max_x + view_w - 1) / view_w;
    let next = (cur + dir).clamp(0, last);
    if next == cur {
        return None;
    }
    Some(PageTurn { target_px: page_target_px(next, view_w, max_x), dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW_W: i32 = 1280;
    const MAX_X: i32 = 3 * VIEW_W; // four pages: 0..=3

    #[test]
    fn wheel_down_turns_one_page_forward() {
        let t = page_turn(0, VIEW_W, MAX_X, -1, false).expect("turns");
        assert_eq!(t, PageTurn { target_px: VIEW_W, dir: 1 });
    }

    #[test]
    fn wheel_up_turns_one_page_back() {
        let t = page_turn(2 * VIEW_W, VIEW_W, MAX_X, 1, false).expect("turns");
        assert_eq!(t, PageTurn { target_px: VIEW_W, dir: -1 });
    }

    #[test]
    fn lock_swallows_the_notch() {
        assert_eq!(page_turn(0, VIEW_W, MAX_X, -1, true), None);
    }

    #[test]
    fn edges_clamp_no_turn() {
        assert_eq!(page_turn(0, VIEW_W, MAX_X, 1, false), None, "first page, wheel up");
        assert_eq!(page_turn(MAX_X, VIEW_W, MAX_X, -1, false), None, "last page, wheel down");
    }

    #[test]
    fn single_page_never_turns() {
        assert_eq!(page_turn(0, VIEW_W, 0, -1, false), None);
    }

    #[test]
    fn mid_glide_notch_turns_from_the_target_not_the_position() {
        // Glide heading to page 1 (target = VIEW_W); another notch (after the
        // lock expired) goes to page 2 — regardless of the eased position.
        let t = page_turn(VIEW_W, VIEW_W, MAX_X, -1, false).expect("turns");
        assert_eq!(t.target_px, 2 * VIEW_W);
    }

    #[test]
    fn last_page_target_clamps_to_max_x() {
        // A ragged extent (content not an exact page multiple) still lands
        // inside the scrollable range.
        let max = 2 * VIEW_W + 300;
        let t = page_turn(2 * VIEW_W, VIEW_W, max, -1, false).expect("turns");
        assert_eq!(t.target_px, max, "clamped to max_x");
        assert_eq!(t.dir, 1);
    }

    #[test]
    fn page_of_rounds_to_nearest() {
        assert_eq!(page_of(0, VIEW_W), 0);
        assert_eq!(page_of(VIEW_W - 10, VIEW_W), 1, "just short of page 1 rounds up");
        assert_eq!(page_of(VIEW_W + 10, VIEW_W), 1);
    }
}
