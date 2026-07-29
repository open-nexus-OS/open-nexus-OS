// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The free geometry helpers of flex placement — clamps, justify/
//! align offset math, clip intersection, and the post-placement box fix-up.
//! Split out of `engine.rs` (the module-size ratchet: the placement loops
//! stay in one file, the arithmetic they share lives here).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: Covered by src/engine_tests.rs + tests/ui_v4_host/
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md

use crate::LayoutBox;
use nexus_layout_types::{Align, FlexItem, FxPx, Justify, LayoutNode, Overflow, Rect};

pub(crate) fn clamp_width(value: FxPx, min: Option<FxPx>, max: Option<FxPx>) -> FxPx {
    let mut out = value.max(FxPx::ZERO);
    if let Some(max) = max {
        out = out.min(max);
    }
    if let Some(min) = min {
        out = out.max(min);
    }
    out
}

pub(crate) fn clamp_height(value: FxPx, min: Option<FxPx>, max: Option<FxPx>) -> FxPx {
    let mut out = value.max(FxPx::ZERO);
    if let Some(max) = max {
        out = out.min(max);
    }
    if let Some(min) = min {
        out = out.max(min);
    }
    out
}

pub(crate) fn clamp_to_max_height(value: FxPx, max_height: Option<FxPx>) -> FxPx {
    match max_height {
        Some(max_height) => value.min(max_height),
        None => value,
    }
}

/// The child's flex data with `Spacer::flex_grow` honored: a `Spacer` grows
/// by its OWN declared factor (default 1) even when its generic `FlexItem`
/// says 0 — the spacer's whole purpose is absorbing free space; reading only
/// `FlexItem.flex_grow` made every default spacer inert (top-left greeter).
pub(crate) fn effective_item(child: &LayoutNode) -> FlexItem {
    let mut item = *child.item();
    if let LayoutNode::Spacer(spacer) = child {
        item.flex_grow = item.flex_grow.max(spacer.flex_grow);
    }
    item
}

pub(crate) fn update_box_geometry(
    boxes: &mut [LayoutBox],
    node_id: usize,
    node: &LayoutNode,
    x: FxPx,
    y: FxPx,
    width: FxPx,
    height: FxPx,
    parent_clip: Option<Rect>,
) {
    let Some(layout_box) = boxes.iter_mut().find(|layout_box| layout_box.node_id == node_id) else {
        return;
    };
    layout_box.rect = Rect::new(x, y, width, height);
    match node {
        LayoutNode::Stack(stack, _, _)
            if matches!(stack.overflow, Overflow::Hidden | Overflow::Scroll(_)) =>
        {
            let own = Rect::new(
                x + stack.padding.left,
                y + stack.padding.top,
                width.saturating_sub(stack.padding.horizontal()),
                height.saturating_sub(stack.padding.vertical()),
            );
            layout_box.clip_rect = intersect_clip(Some(own), parent_clip);
        }
        LayoutNode::Grid(grid, _, _)
            if matches!(grid.overflow, Overflow::Hidden | Overflow::Scroll(_)) =>
        {
            let own = Rect::new(
                x + grid.padding.left,
                y + grid.padding.top,
                width.saturating_sub(grid.padding.horizontal()),
                height.saturating_sub(grid.padding.vertical()),
            );
            layout_box.clip_rect = intersect_clip(Some(own), parent_clip);
        }
        _ => {}
    }
}

pub(crate) fn justify_offsets(
    justify: Justify,
    free_space: FxPx,
    count: usize,
    base_gap: FxPx,
) -> (FxPx, FxPx) {
    if count <= 1 {
        return match justify {
            Justify::Center => (free_space / 2, FxPx::ZERO),
            Justify::End => (free_space, FxPx::ZERO),
            _ => (FxPx::ZERO, base_gap),
        };
    }
    match justify {
        Justify::Start => (FxPx::ZERO, base_gap),
        Justify::Center => (free_space / 2, base_gap),
        Justify::End => (free_space, base_gap),
        Justify::SpaceBetween => (FxPx::ZERO, base_gap + free_space / (count as i32 - 1)),
        Justify::SpaceAround => {
            let slot = free_space / count as i32;
            (slot / 2, base_gap + slot)
        }
        Justify::SpaceEvenly => {
            let slot = free_space / (count as i32 + 1);
            (slot, base_gap + slot)
        }
    }
}

pub(crate) fn align_offset(align: Align, free_space: FxPx) -> FxPx {
    match align {
        Align::Start | Align::Stretch => FxPx::ZERO,
        Align::Center => free_space / 2,
        Align::End => free_space,
    }
}

/// Intersect two optional clip rects. Returns the intersection, or None if disjoint.
pub(crate) fn intersect_clip(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let x = a.x.max(b.x);
            let y = a.y.max(b.y);
            let x2 = (a.x + a.width).min(b.x + b.width);
            let y2 = (a.y + a.height).min(b.y + b.height);
            if x2 > x && y2 > y {
                Some(Rect::new(x, y, x2 - x, y2 - y))
            } else {
                None
            }
        }
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
