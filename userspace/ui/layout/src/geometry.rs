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
use alloc::vec::Vec;
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

/// The per-child share of `free` space, apportioned by `grows` so that the
/// shares SUM EXACTLY to `free`.
///
/// `free * grow / total_grow` alone discards up to `total_grow - 1` pixels, and
/// on a four-key row that is a 2px gutter on the trailing edge: with `.basis(0)`
/// the cells come out equal but no longer TILE their container, which reads as a
/// misaligned right edge. `place_grid` already redistributes its own remainder
/// over its column tracks; the flex path did not.
///
/// The leftover goes by LARGEST REMAINDER, not to whoever comes first. That
/// distinction is load-bearing: in a `2/1/1` row over 330px the double-width
/// child divides exactly (165) while each single cell loses ½, so handing the
/// spare pixel to the first child would push the wide one to 176 and break the
/// `span == 2*cell + gap` relationship the calculator's `0` key depends on.
pub(crate) fn grow_shares(free: i32, total_grow: u32, grows: &[u32]) -> Vec<FxPx> {
    let mut shares: Vec<FxPx> = Vec::with_capacity(grows.len());
    if total_grow == 0 || free <= 0 {
        shares.resize(grows.len(), FxPx::ZERO);
        return shares;
    }
    let total = total_grow as i64;
    let mut handed_out = 0i64;
    // (remainder, index) for the leftover pass.
    let mut rests: Vec<(i64, usize)> = Vec::with_capacity(grows.len());
    for (index, grow) in grows.iter().enumerate() {
        let exact = free as i64 * i64::from(*grow);
        let share = exact / total;
        if *grow > 0 {
            rests.push((exact % total, index));
        }
        handed_out += share;
        shares.push(FxPx::new(share as i32));
    }
    // Largest remainder first; ties keep source order so layout stays
    // deterministic (the engine's whole contract).
    rests.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut left = free as i64 - handed_out;
    for (_, index) in rests {
        if left <= 0 {
            break;
        }
        shares[index].0 += 1;
        left -= 1;
    }
    shares
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

/// Whether this child is a `.scroll(...)` viewport. Such a node measures
/// width 0 (its clipped content must not drive the parent's flex
/// negotiation), so a COLUMN parent stretches it to the parent width at
/// placement — the block `fill-available` rule.
pub(crate) fn is_scroll_viewport(child: &LayoutNode) -> bool {
    matches!(child, LayoutNode::Stack(s, _, _) if matches!(s.overflow, Overflow::Scroll(_)))
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
