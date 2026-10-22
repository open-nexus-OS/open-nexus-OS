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
use nexus_dsl_ir::ui_ir_capnp::BindValue;
use nexus_layout_types::{FxPx, LayoutNode};

/// A scroll layer as the hit-test sees it (RFC-0109): the scroll container's node id (its
/// content's `LayoutBox::scroll_root`), its viewport on the surface `(x0, y0, x1, y1)` and its
/// offset — a surface point inside the viewport lands on the content point moved by `(dx, dy)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollLayerView {
    pub root: usize,
    pub viewport: (i32, i32, i32, i32),
    pub dx: i32,
    pub dy: i32,
}

/// What a triggered handler does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerAction {
    /// Dispatch a store event (payload captured at emit time).
    Dispatch { event: u32, case: u32, payload: Vec<Value> },
    /// Navigate to a route path (evaluated at emit time).
    Navigate { path: alloc::string::String },
    /// Two-way binding write target: (store index, field symbol path) plus the
    /// rule that turns the interaction into a value (`crate::bind`). The rule
    /// travels WITH the handler because the compiler decided it — see IR v1.8;
    /// the runtime never infers it from a widget kind.
    Bind { store: u32, path: Vec<u32>, value: BindValue },
}

/// One interactive region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerEntry {
    /// Child-index path from the scene root to the handler's node.
    pub path: Vec<u32>,
    /// The INSTANCE this handler belongs to (TASK-0077B P1). An interaction
    /// inside a keyed collection item reduces into that item's own state, not
    /// into a store shared by every row.
    pub instance: u64,
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
    /// The node carries `.autofocus(true)` (TASK-0067B): a text-field bind that takes
    /// focus while no field holds it — see `View::autofocus_box`.
    pub autofocus: bool,
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

/// Where an interaction landed.
///
/// `box_id` is the hover/presentation anchor, `entry` the action — and `rect`
/// plus (`x`, `y`) are the handler's box and the point IN THAT BOX'S SPACE,
/// with the scroll transform already applied. A bind rule that derives its
/// value from geometry (`crate::bind::BindValue::TrackFraction`) must measure
/// against exactly the point the painter used, and that transform is resolved
/// here, once: re-deriving it at the call site would be a second copy of the
/// scroll math free to disagree with the pixels.
#[derive(Debug, Clone, Copy)]
pub struct Hit<'h> {
    pub box_id: usize,
    pub entry: &'h HandlerEntry,
    pub rect: nexus_layout_types::Rect,
    pub x: FxPx,
    pub y: FxPx,
}

/// Finds the innermost handler for `trigger_sym` whose box contains (x, y).
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
) -> Option<Hit<'h>> {
    hit_scrolled(handlers, boxes, trigger_sym, x, y, &[], None)
}

/// The hit-test over the page's layers (RFC-0109), mirroring exactly what the compositor shows
/// — input and pixels can never disagree.
///
/// A box of the base is tested at the point. A box of a scroll layer's content
/// (`LayoutBox::scroll_root`) is hittable only inside that layer's viewport, at the point moved
/// by the layer's offset (`layers`; a layer without a view is at offset 0 in its container's
/// own viewport). Either way the box's clip — the clips inside its content — is tested in the
/// box's own space. Every scroll layer of a page is hittable at once: a sidebar beside a
/// scrolling pane, two panes, a pager.
#[must_use]
///
/// `confine` (TASK-0074): the topmost modal's node path — while a modal is
/// open only handlers under it are candidates, whatever the geometry says.
/// The hit-test is the ONE place this is decided, so hover, press, focus and
/// the backdrop rule can never disagree about what is reachable.
pub fn hit_scrolled<'h>(
    handlers: &'h [(usize, HandlerEntry)],
    boxes: &[nexus_layout::LayoutBox],
    trigger_sym: u32,
    x: FxPx,
    y: FxPx,
    layers: &[ScrollLayerView],
    confine: Option<&[u32]>,
) -> Option<Hit<'h>> {
    let mut best: Option<Hit<'h>> = None;
    // (edge distance², hit) — the runner-up pass, see below.
    let mut slop_best: Option<(i64, Hit<'h>)> = None;
    for (box_id, entry) in handlers {
        if entry.trigger != trigger_sym || !crate::overlay::reachable(&entry.path, confine) {
            continue;
        }
        let Some(layout_box) = boxes.iter().find(|b| b.node_id == *box_id) else {
            continue;
        };
        let (px, py) = match layout_box.scroll_root {
            Some(root) => {
                let Some(view) = layer_view(root, layers, boxes) else { continue };
                let (vx0, vy0, vx1, vy1) = view.viewport;
                if x.0 < vx0 || x.0 >= vx1 || y.0 < vy0 || y.0 >= vy1 {
                    continue;
                }
                (FxPx::new(x.0 + view.dx), FxPx::new(y.0 + view.dy))
            }
            None => (x, y),
        };
        if layout_box.clip_rect.is_some_and(|clip| !contains(clip, FxPx::ZERO, px, py)) {
            continue;
        }
        let rect = layout_box.rect;
        // The point is reported in the BOX'S space (scroll applied), so a
        // geometry-derived bind value measures what the painter drew.
        let landed = Hit { box_id: *box_id, entry, rect, x: px, y: py };
        if contains(rect, FxPx::ZERO, px, py) {
            if best.is_none_or(|b| *box_id > b.box_id) {
                best = Some(landed);
            }
            continue;
        }
        // Slop candidate: remember the CLOSEST one, don't let it win yet.
        let slop = layout_box.hit_slop;
        if slop > FxPx::ZERO && contains(rect, slop, px, py) {
            let d = edge_distance_sq(rect, px, py);
            if slop_best.is_none_or(|(bd, b)| d < bd || (d == bd && *box_id > b.box_id)) {
                slop_best = Some((d, landed));
            }
        }
    }
    // An exact hit ALWAYS wins over a slop hit, no matter the tree order:
    // slop must never steal a tap that landed squarely on a neighbour.
    best.or(slop_best.map(|(_, hit)| hit))
}

/// Scroll layer `root`'s view: the caller's when it has one, else its container's own viewport
/// (the clip it imposes on its content) at offset 0 — a container past the layer bound,
/// flattened. `None`
/// when the container is not laid out (a stale root).
fn layer_view(
    root: usize,
    layers: &[ScrollLayerView],
    boxes: &[nexus_layout::LayoutBox],
) -> Option<ScrollLayerView> {
    if let Some(l) = layers.iter().find(|l| l.root == root) {
        return Some(*l);
    }
    let c = boxes.iter().find(|b| b.node_id == root)?.content_clip?;
    let viewport = (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0);
    Some(ScrollLayerView { root, viewport, dx: 0, dy: 0 })
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
    use nexus_layout_types::{Rect, VisualStyle};

    const TAP: u32 = 7;

    fn boxed(node_id: usize, x: i32, y: i32, w: i32, h: i32, slop: i32) -> LayoutBox {
        LayoutBox {
            node_id,
            id: None,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            z_index: 0,
            visual: VisualStyle::default(),
            hit_slop: FxPx::new(slop),
            ..LayoutBox::default()
        }
    }

    fn handler(node_id: usize) -> (usize, HandlerEntry) {
        (
            node_id,
            HandlerEntry {
                path: alloc::vec![],
                instance: crate::store::ROOT_INSTANCE,
                trigger: TAP,
                action: HandlerAction::Dispatch { event: 0, case: 0, payload: alloc::vec![] },
                press_offset: 0,
                autofocus: false,
            },
        )
    }

    fn hit(
        handlers: &[(usize, HandlerEntry)],
        boxes: &[LayoutBox],
        x: i32,
        y: i32,
    ) -> Option<usize> {
        hit_scrolled(handlers, boxes, TAP, FxPx::new(x), FxPx::new(y), &[], None).map(|h| h.box_id)
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

    fn content(
        node_id: usize,
        (x, y, w, h): (i32, i32, i32, i32),
        root: Option<usize>,
        clip: Option<(i32, i32, i32, i32)>,
    ) -> LayoutBox {
        LayoutBox {
            node_id,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            visual: VisualStyle::default(),
            clip_rect: clip.map(|c| {
                Rect::new(
                    FxPx::new(c.0),
                    FxPx::new(c.1),
                    FxPx::new(c.2 - c.0),
                    FxPx::new(c.3 - c.1),
                )
            }),
            overflow: Overflow::Visible,
            scroll_root: root,
            ..LayoutBox::default()
        }
    }

    fn handler(node_id: usize) -> (usize, HandlerEntry) {
        (
            node_id,
            HandlerEntry {
                path: alloc::vec![],
                instance: crate::store::ROOT_INSTANCE,
                trigger: TAP,
                action: HandlerAction::Dispatch { event: 0, case: 0, payload: alloc::vec![] },
                press_offset: 0,
                autofocus: false,
            },
        )
    }

    /// Settings: a scrolling sidebar BESIDE a scrolling content pane — two scroll layers of one
    /// page, both hittable at once, each at its own offset (RFC-0109).
    #[test]
    fn every_scroll_layer_of_a_page_is_hittable_at_once() {
        let layers = [
            ScrollLayerView { root: 2, viewport: (0, 40, 200, 500), dx: 0, dy: 0 },
            ScrollLayerView { root: 8, viewport: (260, 40, 960, 500), dx: 0, dy: 50 },
        ];
        let handlers = [handler(3), handler(9)];
        let boxes = [
            content(3, (10, 100, 180, 40), Some(2), None), // a sidebar row
            content(9, (270, 130, 600, 40), Some(8), None), // a content row (layout y)
        ];
        let hit = |x: i32, y: i32| {
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(x), FxPx::new(y), &layers, None)
                .map(|h| h.box_id)
        };
        assert_eq!(hit(50, 120), Some(3), "the sidebar's layer at its own offset");
        // The content row: layout y 130..170, on the surface at 80..120.
        assert_eq!(hit(300, 100), Some(9), "the pane's layer at the moved point");
        assert_eq!(hit(300, 140), None, "the row's layout position is not on the surface");
        assert_eq!(hit(220, 120), None, "between the viewports");
    }

    /// A clip inside a layer's content moves with it: tested in the content's space.
    #[test]
    fn a_clip_inside_the_content_scrolls_with_it() {
        let layers = [ScrollLayerView { root: 8, viewport: (260, 40, 960, 500), dx: 0, dy: 50 }];
        let handlers = [handler(5)];
        let boxes = [content(5, (270, 210, 600, 40), Some(8), Some((260, 200, 960, 280)))];
        let hit = |x: i32, y: i32| {
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(x), FxPx::new(y), &layers, None)
                .map(|h| h.box_id)
        };
        assert_eq!(hit(300, 170), Some(5), "content y 220: inside the clip and the box");
        assert_eq!(hit(300, 220), None, "content y 270: the clip, not the box");
    }

    /// Content laid out below the viewport is not under the pointer there: a box of a layer is
    /// hit inside its viewport only.
    #[test]
    fn test_reject_a_content_hit_outside_its_viewport() {
        let layers = [ScrollLayerView { root: 8, viewport: (260, 40, 960, 500), dx: 0, dy: 0 }];
        let handlers = [handler(9)];
        let boxes = [content(9, (270, 600, 600, 40), Some(8), None)];
        let hit = |x: i32, y: i32| {
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(x), FxPx::new(y), &layers, None)
                .map(|h| h.box_id)
        };
        assert_eq!(hit(300, 610), None, "below the viewport: covered by the base");
        // Scrolled by 200: the row is on the surface at 400..440.
        let layers = [ScrollLayerView { dy: 200, ..layers[0] }];
        assert_eq!(
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(300), FxPx::new(410), &layers, None)
                .map(|h| h.box_id),
            Some(9)
        );
    }

    /// A root the caller has no view for sits at offset 0 in its container's own viewport.
    #[test]
    fn a_layer_without_a_view_is_at_rest_in_its_container() {
        let mut container = content(8, (260, 40, 700, 460), None, Some((260, 40, 960, 500)));
        container.overflow = Overflow::Scroll(nexus_layout_types::ScrollAxis::Vertical);
        // The viewport is the clip it imposes on its content; nothing clips the container itself.
        container.content_clip = container.clip_rect.take();
        let handlers = [handler(9)];
        let boxes = [container, content(9, (270, 60, 600, 40), Some(8), None)];
        assert_eq!(
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(300), FxPx::new(70), &[], None)
                .map(|h| h.box_id),
            Some(9)
        );
        assert_eq!(
            hit_scrolled(&handlers, &boxes, TAP, FxPx::new(300), FxPx::new(30), &[], None)
                .map(|h| h.box_id),
            None,
            "above the viewport"
        );
    }
}
