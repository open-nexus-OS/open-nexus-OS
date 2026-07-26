// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//!
//! CONTEXT: Layout OUTPUT types — the boxes the engine produces and the
//!          scroll-damage arithmetic over them. Split out of `engine.rs`
//!          (RFC-0057 / TASK-0306) so the engine file holds algorithms and
//!          this one holds the data contract the renderer and hit-tester read.
//! OWNERS: @ui
//! STATUS: Done
//! API_STABILITY: Unstable
//! TEST_COVERAGE: engine_tests, nexus-dsl-runtime::interact::hit_slop_tests
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

use alloc::vec::Vec;
use nexus_layout_types::{FxPx, Overflow, Rect, VisualStyle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox {
    pub node_id: usize,
    pub id: Option<&'static str>,
    pub rect: Rect,
    pub z_index: i16,
    pub visual: VisualStyle,
    /// Optional scissor rect inherited from nearest Overflow::Hidden ancestor.
    /// The renderer must clip paint output to this rect (in content coordinates).
    pub clip_rect: Option<Rect>,
    /// Scroll offset (dx, dy) for overflow containers. Children are shifted by this
    /// amount relative to the container origin. Non-scrollable boxes have (0, 0).
    pub scroll_offset: (FxPx, FxPx),
    /// The container's overflow mode. Used by renderer to decide whether to apply
    /// scissor clipping and scrollbar rendering.
    pub overflow: Overflow,
    /// `.hitSlop(n)` — outward growth of the INPUT rect only (see
    /// [`nexus_layout_types::FlexItem::hit_slop`]). `rect` stays the painted
    /// rect, so pixels and layout are untouched; only `hit_scrolled` reads it.
    pub hit_slop: FxPx,
}

impl Default for LayoutBox {
    /// An inert box at the origin: no id, no clip, no slop, `Overflow::Visible`.
    /// Exists so test fixtures and future construction sites name only the
    /// fields they actually care about (`..LayoutBox::default()`).
    fn default() -> Self {
        Self {
            node_id: 0,
            id: None,
            rect: Rect::zero(),
            z_index: 0,
            visual: VisualStyle::default(),
            clip_rect: None,
            scroll_offset: (FxPx::ZERO, FxPx::ZERO),
            overflow: Overflow::Visible,
            hit_slop: FxPx::ZERO,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResult {
    pub boxes: Vec<LayoutBox>,
    pub content_height: FxPx,
}

/// Scroll damage — at most two dirty rects per scroll delta.
/// Allocation-free (stack-only), bounded size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollDamage {
    /// Up to two damage rects. A `None` entry means no rect at that slot.
    pub rects: [Option<Rect>; 2],
}

impl ScrollDamage {
    pub const EMPTY: Self = Self { rects: [None, None] };

    pub fn is_empty(&self) -> bool {
        self.rects[0].is_none() && self.rects[1].is_none()
    }
}

/// Compute the dirty area when a viewport scrolls from `old_offset` to `new_offset`.
/// Returns at most two rects: the newly-exposed strip and the newly-hidden strip
/// (which need invalidation and repaint respectively).
///
/// Integer-only, deterministic, order-agnostic.
pub fn compute_scroll_damage(
    old_offset: (FxPx, FxPx),
    new_offset: (FxPx, FxPx),
    viewport: Rect,
) -> ScrollDamage {
    let dx = new_offset.0 - old_offset.0;
    let dy = new_offset.1 - old_offset.1;
    if dx.0 == 0 && dy.0 == 0 {
        return ScrollDamage::EMPTY;
    }
    let mut damage = ScrollDamage::EMPTY;
    let abs_dx = FxPx::new(dx.0.abs());
    let abs_dy = FxPx::new(dy.0.abs());

    if dx.0 != 0 {
        // Newly-exposed strip at the leading edge
        let exposed = if dx.0 > 0 {
            // Scrolling right: left side shifts out, right side becomes visible
            Rect::new(
                viewport.x + viewport.width.saturating_sub(abs_dx),
                viewport.y,
                abs_dx.min(viewport.width),
                viewport.height,
            )
        } else {
            // Scrolling left: right side shifts out, left side becomes visible
            Rect::new(viewport.x, viewport.y, abs_dx.min(viewport.width), viewport.height)
        };
        if exposed.width > FxPx::ZERO && exposed.height > FxPx::ZERO {
            damage.rects[0] = Some(exposed);
        }
    }

    if dy.0 != 0 {
        let exposed = if dy.0 > 0 {
            Rect::new(
                viewport.x,
                viewport.y + viewport.height.saturating_sub(abs_dy),
                viewport.width,
                abs_dy.min(viewport.height),
            )
        } else {
            Rect::new(viewport.x, viewport.y, viewport.width, abs_dy.min(viewport.height))
        };
        if exposed.width > FxPx::ZERO && exposed.height > FxPx::ZERO {
            let slot = if damage.rects[0].is_some() { 1 } else { 0 };
            damage.rects[slot] = Some(exposed);
        }
    }

    damage
}

impl LayoutResult {
    /// Reposition all boxes inside the scroll container identified by `container_node_id`
    /// to reflect a new scroll offset. Returns the scroll damage rects.
    ///
    /// This is place-only: no remeasurement, no text reshaping.
    /// Allocation-free (mutates existing boxes).
    pub fn reposition_scroll(
        &mut self,
        container_node_id: usize,
        new_offset: (FxPx, FxPx),
    ) -> ScrollDamage {
        let mut old_offset = (FxPx::ZERO, FxPx::ZERO);
        let mut viewport = Rect::zero();
        let mut container_found = false;

        // Find the container
        for b in &self.boxes {
            if b.node_id == container_node_id {
                old_offset = b.scroll_offset;
                viewport = b.rect;
                container_found = true;
                break;
            }
        }
        if !container_found {
            return ScrollDamage::EMPTY;
        }

        let delta_x = new_offset.0 - old_offset.0;
        let delta_y = new_offset.1 - old_offset.1;

        if delta_x.0 == 0 && delta_y.0 == 0 {
            return ScrollDamage::EMPTY;
        }

        let damage = compute_scroll_damage(old_offset, new_offset, viewport);

        // Shift descendant boxes: only those with node_id > container_node_id
        // AND the same old scroll_offset. In DFS order, descendants always have
        // higher node_ids than their ancestor.
        for b in &mut self.boxes {
            if b.node_id > container_node_id && b.scroll_offset == old_offset {
                b.scroll_offset = new_offset;
                b.rect.x += delta_x;
                b.rect.y += delta_y;
            }
        }

        // Update the container's own scroll_offset
        for b in &mut self.boxes {
            if b.node_id == container_node_id {
                b.scroll_offset = new_offset;
                break;
            }
        }

        damage
    }
}
