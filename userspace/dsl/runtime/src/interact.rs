// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Interaction routing: handler regions + hit-testing.
//!
//! Handlers are collected during emission with their **path in the final
//! scene tree** (child indices from the root). Because the layout engine
//! assigns `LayoutBox::node_id` in pre-order (one box per node, counter
//! starts at 1), a path converts to the box id by a counting walk — no ids
//! smuggled through `&'static str`, no parallel bookkeeping in the engine.
//!
//! Dispatch payloads are evaluated **at emit time**: a collection item's
//! handler captures that item's values (the only correct reading once the
//! loop binding is gone). Handlers whose payload reads state therefore
//! record a Paint-class dependency — any change re-emits and re-captures.

use crate::store::Value;
use alloc::vec::Vec;
use nexus_layout_types::{FxPx, LayoutNode};

/// A scrolled viewport for hit-testing: `(viewport (x0,y0,x1,y1), dx, dy)`.
pub type ScrollView = ((i32, i32, i32, i32), i32, i32);

/// What a triggered handler does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerAction {
    /// Dispatch a store event (payload captured at emit time).
    Dispatch { event: u32, case: u32, payload: Vec<Value> },
    /// Navigate to a route path (evaluated at emit time).
    Navigate { path: alloc::string::String },
    /// Two-way binding write target: (store index, field symbol path).
    /// The interaction supplies the value (Tap on a bound Toggle flips the
    /// Bool; text input writes the new text).
    Bind { store: u32, path: Vec<u32> },
}

/// One interactive region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerEntry {
    /// Child-index path from the scene root to the handler's node.
    pub path: Vec<u32>,
    /// Interaction trigger symbol id (`Tap`, `Change`, …).
    pub trigger: u32,
    pub action: HandlerAction,
    /// Pre-order box-id OFFSET from the handler's node to the part the PRESS
    /// interaction animates (`registry::press_offset` — a structural constant
    /// per widget kind, like `child_path`). `0` = the node itself (the
    /// uniform press dip); the toggle points at its thumb (`+1`) so the press
    /// stretches the knob along the travel axis instead of squeezing the
    /// whole track.
    pub press_offset: u32,
}

/// Pre-order box id (1-based, matches `LayoutBox::node_id`) for a path.
///
/// The count mirrors the ENGINE'S visit order, not declaration order: a
/// Stack places its `Position::Absolute` (`.overlay()`) children AFTER every
/// in-flow child, and node ids are assigned at visit time. Counting the
/// declared order shifted every handler id behind a non-last open overlay —
/// an open menu with closed picker placeholders after it resolved its own
/// rows to the placeholders' boxes and went dead. Grids place in declaration
/// order and count as before.
#[must_use]
pub fn path_to_box_id(scene: &LayoutNode, path: &[u32]) -> Option<usize> {
    fn count_nodes(node: &LayoutNode) -> usize {
        match node {
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
                1 + children.iter().map(count_nodes).sum::<usize>()
            }
            LayoutNode::Spacer(_) | LayoutNode::Text(_, _) | LayoutNode::TextInput(_, _) => 1,
        }
    }
    fn is_absolute(node: &LayoutNode) -> bool {
        matches!(node.item().position, nexus_layout_types::Position::Absolute)
    }
    let mut id = 1usize; // the root
    let mut node = scene;
    for &index in path {
        let (children, absolutes_last) = match node {
            LayoutNode::Stack(_, _, children) => (children, true),
            LayoutNode::Grid(_, _, children) => (children, false),
            _ => return None,
        };
        let index = index as usize;
        if index >= children.len() {
            return None;
        }
        // Skip ourselves (the parent) + every child the engine VISITS before
        // the target: for a Stack, all in-flow children precede any absolute
        // one; within each group, declaration order holds.
        let target_abs = absolutes_last && is_absolute(&children[index]);
        let mut skipped = 1usize;
        for (ci, child) in children.iter().enumerate() {
            let child_abs = absolutes_last && is_absolute(child);
            let visited_before =
                if target_abs { !child_abs || ci < index } else { !child_abs && ci < index };
            if visited_before {
                skipped += count_nodes(child);
            }
        }
        id += skipped;
        node = &children[index];
    }
    Some(id)
}

/// Finds the innermost handler for `trigger_sym` whose box contains (x, y),
/// returning its pre-order box id alongside the entry (the id is the hover/
/// presentation anchor; the entry is the action).
///
/// `boxes` is the flat `LayoutResult::boxes`; deeper nodes have larger
/// pre-order ids, so the max matching id wins (innermost target).
#[must_use]
pub fn hit<'h>(
    handlers: &'h [(usize, HandlerEntry)],
    boxes: &[nexus_layout::LayoutBox],
    trigger_sym: u32,
    x: FxPx,
    y: FxPx,
) -> Option<(usize, &'h HandlerEntry)> {
    hit_scrolled(handlers, boxes, trigger_sym, x, y, None)
}

/// Paint-time scroll transform for hit-testing, mirroring exactly what the
/// scrolled painter does — input and pixels can never disagree.
///
/// A clipped box is tested against its OWN viewport, and the scroll shift
/// applies only to boxes whose clip lies inside the ACTIVE scroll container
/// (`scroll` = that container's viewport + dx/dy). Three cases:
///
///   * clip == active viewport — direct scroll content: hittable on the
///     viewport, tested at the shifted point `(x+dx, y+dy)`.
///   * clip strictly INSIDE the active viewport — a nested clip that rides
///     the scrolled content: its window itself is shifted on the surface.
///   * clip elsewhere — a second, static viewport (another `.scroll`
///     container, a widget-internal clip): hittable inside its own window at
///     the identity point. This used to `continue` — a page with a scrolling
///     content pane AND a sidebar rejected every sidebar-or-pane handler
///     whose tap fell outside the ONE viewport the caller picked, which read
///     as "the whole content area is dead".
#[must_use]
pub fn hit_scrolled<'h>(
    handlers: &'h [(usize, HandlerEntry)],
    boxes: &[nexus_layout::LayoutBox],
    trigger_sym: u32,
    x: FxPx,
    y: FxPx,
    scroll: Option<ScrollView>,
) -> Option<(usize, &'h HandlerEntry)> {
    let mut best: Option<(usize, &HandlerEntry)> = None;
    // (edge distance², box_id, handler) — the runner-up pass, see below.
    let mut slop_best: Option<(i64, usize, &HandlerEntry)> = None;
    for (box_id, entry) in handlers {
        if entry.trigger != trigger_sym {
            continue;
        }
        let Some(layout_box) = boxes.iter().find(|b| b.node_id == *box_id) else {
            continue;
        };
        let (mut px, mut py) = (x, y);
        if let Some(clip) = layout_box.clip_rect {
            let (cx0, cy0) = (clip.x.0, clip.y.0);
            let (cx1, cy1) = (cx0 + clip.width.0, cy0 + clip.height.0);
            // The box's hittable WINDOW in surface space, and whether the
            // point shifts (scrolls) inside it.
            let (window, shifted) = match scroll {
                Some((vc, _, _)) if (cx0, cy0, cx1, cy1) == vc => (vc, true),
                Some((vc, dx, dy)) if cx0 >= vc.0 && cy0 >= vc.1 && cx1 <= vc.2 && cy1 <= vc.3 => {
                    // Nested clip riding the scrolled content: its window is
                    // shifted by the offset, bounded by the viewport.
                    let w = (
                        (cx0 - dx).max(vc.0),
                        (cy0 - dy).max(vc.1),
                        (cx1 - dx).min(vc.2),
                        (cy1 - dy).min(vc.3),
                    );
                    (w, true)
                }
                _ => ((cx0, cy0, cx1, cy1), false),
            };
            if x.0 < window.0 || x.0 >= window.2 || y.0 < window.1 || y.0 >= window.3 {
                continue;
            }
            if shifted {
                if let Some((_, dx, dy)) = scroll {
                    px = FxPx::new(x.0 + dx);
                    py = FxPx::new(y.0 + dy);
                }
            }
        }
        let rect = layout_box.rect;
        if contains(rect, FxPx::ZERO, px, py) {
            if best.is_none_or(|(id, _)| *box_id > id) {
                best = Some((*box_id, entry));
            }
            continue;
        }
        // Slop candidate: remember the CLOSEST one, don't let it win yet.
        let slop = layout_box.hit_slop;
        if slop > FxPx::ZERO && contains(rect, slop, px, py) {
            let d = edge_distance_sq(rect, px, py);
            if slop_best.is_none_or(|(bd, id, _)| d < bd || (d == bd && *box_id > id)) {
                slop_best = Some((d, *box_id, entry));
            }
        }
    }
    // An exact hit ALWAYS wins over a slop hit, no matter the tree order:
    // slop must never steal a tap that landed squarely on a neighbour.
    best.or(slop_best.map(|(_, id, entry)| (id, entry)))
}

/// Point-in-rect, optionally grown outward by `slop` on all four sides.
fn contains(rect: nexus_layout_types::Rect, slop: FxPx, px: FxPx, py: FxPx) -> bool {
    px >= rect.x - slop
        && py >= rect.y - slop
        && px < rect.x + rect.width + slop
        && py < rect.y + rect.height + slop
}

/// Squared distance from the point to the rect's edge (0 when inside). Used to
/// break ties between overlapping slop regions — the nearest control wins,
/// which is what a user means when two enlarged targets both cover the pixel.
fn edge_distance_sq(rect: nexus_layout_types::Rect, px: FxPx, py: FxPx) -> i64 {
    let dx = (rect.x.0 - px.0).max(px.0 - (rect.x.0 + rect.width.0 - 1)).max(0) as i64;
    let dy = (rect.y.0 - py.0).max(py.0 - (rect.y.0 + rect.height.0 - 1)).max(0) as i64;
    dx * dx + dy * dy
}

#[cfg(test)]
mod hit_slop_tests {
    use super::*;
    use nexus_layout::LayoutBox;
    use nexus_layout_types::{Overflow, Rect, VisualStyle};

    const TAP: u32 = 7;

    fn boxed(node_id: usize, x: i32, y: i32, w: i32, h: i32, slop: i32) -> LayoutBox {
        LayoutBox {
            node_id,
            id: None,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            z_index: 0,
            visual: VisualStyle::default(),
            clip_rect: None,
            scroll_offset: (FxPx::ZERO, FxPx::ZERO),
            overflow: Overflow::Visible,
            hit_slop: FxPx::new(slop),
            glass_nested: false,
            text_px: None,
        }
    }

    fn handler(node_id: usize) -> (usize, HandlerEntry) {
        (
            node_id,
            HandlerEntry {
                path: alloc::vec![],
                trigger: TAP,
                action: HandlerAction::Dispatch { event: 0, case: 0, payload: alloc::vec![] },
                press_offset: 0,
            },
        )
    }

    fn hit(
        handlers: &[(usize, HandlerEntry)],
        boxes: &[LayoutBox],
        x: i32,
        y: i32,
    ) -> Option<usize> {
        hit_scrolled(handlers, boxes, TAP, FxPx::new(x), FxPx::new(y), None).map(|(id, _)| id)
    }

    /// The bug this exists for: a 29x28 status pill in a 36px-tall top bar is
    /// physically unhittable — the observed miss was 20px BELOW the pill, on a
    /// pixel that belongs to no control at all. Slop must catch it.
    #[test]
    fn slop_catches_a_tap_just_outside_a_small_pill() {
        let handlers = [handler(1)];
        let boxes = [boxed(1, 1240, 4, 29, 28, 16)];
        assert_eq!(hit(&handlers, &boxes, 1254, 18), Some(1), "dead centre must hit");
        assert_eq!(hit(&handlers, &boxes, 1254, 45), Some(1), "17px below → inside slop");
        assert_eq!(hit(&handlers, &boxes, 1254, 60), None, "32px below → outside slop");
    }

    /// Slop must never steal a tap that landed squarely on a neighbour, no
    /// matter the tree order. Node 1 is declared LAST (it would win the
    /// `box_id >` tie-break) and its slop covers all of node 2.
    #[test]
    fn an_exact_hit_beats_a_slop_hit_regardless_of_tree_order() {
        let handlers = [handler(2), handler(9)];
        let boxes = [
            boxed(2, 100, 0, 20, 20, 0),  // exact target, low id
            boxed(9, 140, 0, 20, 20, 40), // high id, slop swallows the neighbour
        ];
        assert_eq!(hit(&handlers, &boxes, 110, 10), Some(2), "exact hit wins over slop");
        assert_eq!(hit(&handlers, &boxes, 150, 10), Some(9), "its own rect still wins");
    }

    /// Two enlarged targets overlapping the same pixel: the NEARER one wins,
    /// which is what a user means when they aim between two buttons.
    #[test]
    fn overlapping_slop_regions_resolve_to_the_nearest_control() {
        let handlers = [handler(1), handler(2)];
        let boxes = [boxed(1, 0, 0, 20, 20, 40), boxed(2, 100, 0, 20, 20, 40)];
        assert_eq!(hit(&handlers, &boxes, 35, 10), Some(1), "15px from #1, 65px from #2");
        assert_eq!(hit(&handlers, &boxes, 85, 10), Some(2), "65px from #1, 15px from #2");
    }

    /// Zero slop is the default and must behave exactly as before.
    #[test]
    fn zero_slop_is_the_old_behaviour() {
        let handlers = [handler(1)];
        let boxes = [boxed(1, 10, 10, 20, 20, 0)];
        assert_eq!(hit(&handlers, &boxes, 29, 29), Some(1));
        assert_eq!(hit(&handlers, &boxes, 30, 30), None, "half-open rect, unchanged");
    }
}

#[cfg(test)]
mod multi_viewport_tests {
    use super::*;
    use nexus_layout::LayoutBox;
    use nexus_layout_types::{Overflow, Rect, VisualStyle};

    const TAP: u32 = 7;

    fn clipped(
        node_id: usize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        clip: (i32, i32, i32, i32),
    ) -> LayoutBox {
        LayoutBox {
            node_id,
            id: None,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            z_index: 0,
            visual: VisualStyle::default(),
            clip_rect: Some(Rect::new(
                FxPx::new(clip.0),
                FxPx::new(clip.1),
                FxPx::new(clip.2 - clip.0),
                FxPx::new(clip.3 - clip.1),
            )),
            scroll_offset: (FxPx::ZERO, FxPx::ZERO),
            overflow: Overflow::Visible,
            hit_slop: FxPx::ZERO,
            glass_nested: false,
            text_px: None,
        }
    }

    fn handler(node_id: usize) -> (usize, HandlerEntry) {
        (
            node_id,
            HandlerEntry {
                path: alloc::vec![],
                trigger: TAP,
                action: HandlerAction::Dispatch { event: 0, case: 0, payload: alloc::vec![] },
                press_offset: 0,
            },
        )
    }

    /// The settings-window regression: a static sidebar viewport NEXT TO the
    /// active (scrolling) content viewport. The old code tested every clipped
    /// box against the ONE active viewport, so the sidebar's own handlers —
    /// and, when the sidebar happened to be picked as active, the whole
    /// content pane — read as dead ("apphost: input tap miss" on every row).
    #[test]
    fn a_second_static_viewport_stays_hittable() {
        // Sidebar viewport x 0..200, content viewport x 260..960 (active,
        // scrolled down by 50).
        let active = ((260, 40, 960, 500), 0, 50);
        let handlers = [handler(3), handler(9)];
        let boxes = [
            clipped(3, 10, 100, 180, 40, (0, 40, 200, 500)), // sidebar row
            clipped(9, 270, 130, 600, 40, (260, 40, 960, 500)), // scrolled row (model y)
        ];
        // Sidebar row: identity hit inside its own viewport.
        let hit = |x: i32, y: i32| {
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(x), FxPx::new(y), Some(active))
                .map(|(id, _)| id)
        };
        assert_eq!(hit(50, 120), Some(3), "static sidebar viewport must hit");
        // Scrolled row: model y 130..170, on the surface at 80..120.
        assert_eq!(hit(300, 100), Some(9), "active viewport hits at the shifted point");
        assert_eq!(hit(300, 140), None, "the row's model position is NOT on the surface");
        // Outside both viewports: nothing.
        assert_eq!(hit(220, 120), None);
    }

    /// A nested clip riding the scrolled content: its hit window shifts with
    /// the scroll offset instead of staying at the model position.
    #[test]
    fn a_nested_clip_inside_the_active_viewport_scrolls_with_it() {
        let active = ((260, 40, 960, 500), 0, 50);
        let handlers = [handler(5)];
        // The box's nearest clip is a nested hidden container at model y
        // 200..280, inside the active viewport; on the surface it sits at
        // 150..230.
        let boxes = [clipped(5, 270, 210, 600, 40, (260, 200, 960, 280))];
        let hit = |x: i32, y: i32| {
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(x), FxPx::new(y), Some(active))
                .map(|(id, _)| id)
        };
        // Box model y 210..250 → surface 160..200.
        assert_eq!(hit(300, 170), Some(5), "shifted window hits");
        assert_eq!(hit(300, 220), None, "the model position no longer hits");
    }
}
