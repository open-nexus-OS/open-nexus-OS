// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Where the paint-time hover wash goes.
//!
//! The wash follows a box's `corner_radius`, and the box the hit-test hands
//! back is the HANDLER box — which in this DSL is frequently a bare wrapper.
//! `Stack { WinActionFace { … } } on Tap -> dispatch(NewFolder)` is the window
//! kit's documented shape (a dispatch target is a static case name and cannot
//! be passed as a prop), and a bare wrapper carries no radius. Washing it drew
//! a RECTANGLE around a pill — "every hovered circle wore a white square" — and
//! that defect is why the wash was switched off platform-wide rather than
//! aimed properly.
//!
//! This module aims it. It is deliberately a pure function over laid-out boxes
//! so it can be tested on the host: `probe/` only compiles for the RISC-V
//! target, so anything that lives there is boot-provable at best.

use nexus_layout::LayoutBox;

/// A hovered box and a candidate child are "the same control" when the child
/// covers essentially all of it. Handler wrappers hug their content, so a few
/// pixels of slack (padding, the hairline row under a list row) still counts.
const WASH_ANCHOR_SLACK: i32 = 8;

/// A control is SHORT: file rows 41, sidebar entries 44, chrome buttons 30,
/// action pills 52, menu rows 40. A container catch-all (overlay backdrop,
/// panel body) is hundreds of px tall.
const CONTROL_MAX_H: i32 = 72;

/// The GROW's ceiling (`anim::INTERACTION_MAX_DIM`), reused as the second arm
/// below so a small square control qualifies regardless of height.
const CONTROL_MAX_DIM: i32 = 160;

/// Whether a box may take the hover WASH.
///
/// Deliberately looser than the hover-GROW's rule: growing a full-width list
/// row would displace its own content, but washing one is exactly what the
/// design asks for. Sharing one predicate excluded a 680-px file row from
/// both — which is why list rows and sidebar entries had no hover at all.
#[must_use]
pub(crate) fn hover_washable(boxes: &[LayoutBox], node_id: usize) -> bool {
    boxes.iter().find(|b| b.node_id == node_id).is_none_or(|b| {
        b.rect.height.0 <= CONTROL_MAX_H
            || (b.rect.width.0 <= CONTROL_MAX_DIM && b.rect.height.0 <= CONTROL_MAX_DIM)
    })
}

/// Which box the hover wash should paint over, given the hovered HANDLER box.
///
/// Normally that is the handler box itself. When the handler has NO radius and
/// holds a single box that both covers it and HAS one, the wash moves to that
/// child — the child is what the user sees as the control.
///
/// Returns `None` when the hovered id has no laid-out box (a stale hover across
/// a re-layout), which is also the signal to paint nothing.
#[must_use]
pub(crate) fn hover_wash_anchor(boxes: &[LayoutBox], hovered: usize) -> Option<usize> {
    let outer = boxes.iter().find(|b| b.node_id == hovered)?;
    if outer.visual.corner_radius.top_left.0 > 0 {
        return Some(hovered);
    }
    let (ox, oy) = (outer.rect.x.0, outer.rect.y.0);
    let (ow, oh) = (outer.rect.width.0, outer.rect.height.0);
    let mut best: Option<usize> = None;
    for b in boxes {
        if b.node_id == hovered || b.visual.corner_radius.top_left.0 <= 0 {
            continue;
        }
        let covers = b.rect.x.0 >= ox
            && b.rect.y.0 >= oy
            && b.rect.x.0 + b.rect.width.0 <= ox + ow
            && b.rect.y.0 + b.rect.height.0 <= oy + oh
            && b.rect.width.0 + WASH_ANCHOR_SLACK >= ow
            && b.rect.height.0 + WASH_ANCHOR_SLACK >= oh;
        if !covers {
            continue;
        }
        // More than one rounded box fills the wrapper (a row of pills under one
        // handler): the wrapper is not a single control, so keep the wash on it
        // rather than picking an arbitrary child.
        if best.is_some() {
            return Some(hovered);
        }
        best = Some(b.node_id);
    }
    Some(best.unwrap_or(hovered))
}

#[cfg(test)]
mod tests {
    use super::hover_wash_anchor;
    use nexus_layout::LayoutBox;
    use nexus_layout_types::{CornerRadius, FxPx, Rect};

    /// `node_id`, rect and corner radius — the only three inputs the choice
    /// reads.
    fn boxed(node_id: usize, (x, y, w, h): (i32, i32, i32, i32), radius: i32) -> LayoutBox {
        let mut b = LayoutBox {
            node_id,
            rect: Rect {
                x: FxPx::new(x),
                y: FxPx::new(y),
                width: FxPx::new(w),
                height: FxPx::new(h),
            },
            ..LayoutBox::default()
        };
        b.visual.corner_radius = CornerRadius::uniform(FxPx::new(radius));
        b
    }

    #[test]
    fn a_rounded_handler_washes_itself() {
        // Every `WinTopBar` button: the handler IS the rounded box.
        let boxes = [boxed(7, (0, 0, 30, 30), 10)];
        assert_eq!(hover_wash_anchor(&boxes, 7), Some(7));
    }

    /// THE regression this module exists for: `Stack { WinActionFace { … } }
    /// on Tap` — a square wrapper around a `rounded(full)` pill.
    #[test]
    fn a_bare_wrapper_washes_the_pill_inside_it() {
        let boxes = [boxed(1, (10, 10, 66, 52), 0), boxed(2, (10, 10, 66, 52), 26)];
        assert_eq!(hover_wash_anchor(&boxes, 1), Some(2));
    }

    #[test]
    fn a_child_that_only_covers_part_of_the_wrapper_is_not_the_control() {
        // A 30px icon inside a 240px list row: the row is the control, the icon
        // is decoration. Washing the icon would highlight a sliver.
        let boxes = [boxed(1, (0, 0, 240, 40), 0), boxed(2, (4, 5, 30, 30), 8)];
        assert_eq!(hover_wash_anchor(&boxes, 1), Some(1));
    }

    #[test]
    fn several_rounded_children_keep_the_wash_on_the_wrapper() {
        // A row of pills under one handler is not a single control; picking one
        // of them would be arbitrary.
        let boxes = [
            boxed(1, (0, 0, 66, 52), 0),
            boxed(2, (0, 0, 66, 52), 26),
            boxed(3, (0, 0, 66, 52), 26),
        ];
        assert_eq!(hover_wash_anchor(&boxes, 1), Some(1));
    }

    #[test]
    fn the_slack_absorbs_a_hairline_under_a_row() {
        // `FileRow` is `Stack { row(40px) + hairline(1px) }`: the rounded row
        // falls 1px short of the wrapper and is still the control.
        let boxes = [boxed(1, (0, 0, 240, 41), 0), boxed(2, (0, 0, 240, 40), 10)];
        assert_eq!(hover_wash_anchor(&boxes, 1), Some(2));
    }

    /// The wash reaches full-width CONTROLS — this is what a file row, a
    /// sidebar entry and a menu row all are, and what the grow's ≤160px rule
    /// wrongly excluded (leaving those with no hover affordance at all).
    #[test]
    fn a_full_width_row_is_washable() {
        let boxes = [boxed(1, (0, 0, 680, 41), 10)];
        assert!(super::hover_washable(&boxes, 1));
    }

    /// …and stops at containers. An overlay backdrop or a panel body is a TAP
    /// consumer, not a control; washing it would flash the whole window.
    #[test]
    fn a_container_catch_all_is_not_washable() {
        let boxes = [boxed(1, (0, 0, 960, 570), 0)];
        assert!(!super::hover_washable(&boxes, 1));
    }

    /// A small square control qualifies on the second arm even when it is
    /// taller than a row (a 96px grid tile).
    #[test]
    fn a_small_square_tile_is_washable() {
        let boxes = [boxed(1, (0, 0, 124, 96), 12)];
        assert!(super::hover_washable(&boxes, 1));
    }

    #[test]
    fn a_stale_hover_id_washes_nothing() {
        // Hover survives a re-layout by id; if the id is gone, paint nothing
        // rather than washing box 0.
        let boxes = [boxed(1, (0, 0, 30, 30), 10)];
        assert_eq!(hover_wash_anchor(&boxes, 99), None);
    }
}
