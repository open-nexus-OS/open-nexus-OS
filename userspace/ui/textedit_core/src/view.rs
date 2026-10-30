// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a single-line field shows its text (TASK-0346 P2): the horizontal offset that
//! keeps the caret inside the field's box — `margin` px from either edge while the text allows —
//! moved only as far as the caret needs (typing at the end never jumps back to the start, a
//! deletion pulls the text back so no empty space trails it). Pure pixels: the host measures with
//! the painter's metrics and paints, places the caret and hit-tests at the same offset.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below

/// The offset (px, ≥ 0) a field of `box_w` shows its text of `text_w` at, its caret at `caret_x`
/// (both measured from the text's start), starting from the `offset` it showed before.
#[must_use]
pub fn follow(offset: i32, caret_x: i32, box_w: i32, text_w: i32, margin: i32) -> i32 {
    let box_w = box_w.max(1);
    let margin = margin.clamp(0, box_w / 4);
    let mut off = offset;
    if caret_x - off > box_w - margin {
        off = caret_x - (box_w - margin);
    }
    if caret_x - off < margin {
        off = caret_x - margin;
    }
    // Never past the end: the text's end stays at most `margin` inside the right edge.
    off.clamp(0, (text_w + margin - box_w).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_never_scrolls() {
        assert_eq!(follow(0, 40, 200, 60, 8), 0);
        assert_eq!(follow(30, 40, 200, 60, 8), 0, "a stale offset is pulled back");
    }

    #[test]
    fn typing_past_the_edge_scrolls_just_enough_and_home_comes_back() {
        // 300 px of text in a 200 px box, the caret at the end: the end sits 8 px inside.
        let at_end = follow(0, 300, 200, 300, 8);
        assert_eq!(at_end, 108);
        // One more char: the offset follows by its width, no jump.
        assert_eq!(follow(at_end, 310, 200, 310, 8), 118);
        // The caret moved left inside the view: nothing moves.
        assert_eq!(follow(118, 200, 200, 310, 8), 118);
        // Home: the start comes back.
        assert_eq!(follow(118, 0, 200, 310, 8), 0);
    }

    #[test]
    fn a_deletion_pulls_the_text_back() {
        // The text shrank under a scrolled view: no empty space trails the end.
        assert_eq!(follow(118, 150, 200, 150, 8), 0);
        assert_eq!(follow(118, 250, 200, 250, 8), 58);
    }
}
