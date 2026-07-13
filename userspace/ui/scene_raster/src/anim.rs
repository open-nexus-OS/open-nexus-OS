// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-node **animation transform** for the CPU scene painter — the
//! paint tail of the DSL `.animate`/`.transition`/`.effect` binding
//! (docs/dev/ui/foundations/animation.md, ADR-0031). A [`NodeAnim`] keyed by
//! `LayoutBox::node_id` carries the animation engine's current interpolated
//! opacity + translate + uniform scale for one node; the picked painter draws
//! that node's fill transformed and alpha-scaled instead of at rest. This
//! mirrors the existing `HoverWash` (per-node paint state) and `ScrollView`
//! (per-box paint transform) primitives — presentation-only, never a
//! re-layout, alloc-free.
//! OWNERS: @ui
//! STATUS: In progress (TASK-0062/0075 DSL animation binding)
//!
//! SCOPE: the fill (`ShapeKind::Rect`, rounded) + border transform. Non-rect
//! shapes on an animated node fall back to an identity draw (rare — animated
//! DSL nodes are panels/bars/text); text is faded/translated by the caller's
//! separate glyph pass, not here.

use crate::{paint_borders_row, paint_box_row, RowCanvas};
use nexus_layout::LayoutBox;
use nexus_layout_types::{Rgba8, ShapeKind};

/// The animation engine's current values for one node, ready for paint. `100`
/// scale + `255` opacity + zero translate is the identity (no visible change).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeAnim {
    /// Pre-order box id this transform applies to (`LayoutBox::node_id`).
    pub node_id: usize,
    /// Multiplicative opacity, `0..=255` (255 = opaque). The fill's alpha is
    /// scaled by this; over the already-painted page base a reduced alpha
    /// reads as a fade toward the background (src-over).
    pub opacity: u8,
    /// Surface-space translate applied to the node (px).
    pub dx: i32,
    pub dy: i32,
    /// Uniform scale about the box center, in percent (`100` = identity).
    pub scale_pct: u16,
}

impl NodeAnim {
    /// The identity (no-op) transform for `node_id`.
    #[must_use]
    pub fn identity(node_id: usize) -> Self {
        Self { node_id, opacity: 255, dx: 0, dy: 0, scale_pct: 100 }
    }

    /// True when the transform has no visible effect (skip the anim path).
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.opacity == 255 && self.dx == 0 && self.dy == 0 && self.scale_pct == 100
    }

    /// The box rect after uniform scale about its center + translate.
    #[must_use]
    pub fn transform_rect(&self, x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
        let s = self.scale_pct.max(1) as i64;
        let nw = (w as i64 * s / 100) as i32;
        let nh = (h as i64 * s / 100) as i32;
        // Keep the center fixed, then translate.
        let nx = x + (w - nw) / 2 + self.dx;
        let ny = y + (h - nh) / 2 + self.dy;
        (nx, ny, nw, nh)
    }
}

/// Multiplies a color's alpha by `opacity/255` (fade toward the page base via
/// src-over when this fill draws over the already-painted background).
#[inline]
fn fade(c: Rgba8, opacity: u8) -> Rgba8 {
    let a = (c.a as u32 * opacity as u32 / 255) as u8;
    Rgba8 { r: c.r, g: c.g, b: c.b, a }
}

/// Paints one animated box's fill + border for the current row, transformed
/// and alpha-scaled. Rect/rounded fills honor the full transform; other shapes
/// fall back to the identity `paint_box_row` (they are not the animated-node
/// case this binding targets).
pub(crate) fn paint_anim_box_row(canvas: &mut RowCanvas<'_>, b: &LayoutBox, a: &NodeAnim) {
    let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    if w <= 0 || h <= 0 {
        return;
    }
    // Non-rect shapes: no transform path — draw at rest so the node stays
    // honest (documented scope limit).
    if !matches!(b.visual.shape, ShapeKind::Rect) {
        paint_box_row(canvas, b);
        return;
    }
    let (nx, ny, nw, nh) = a.transform_rect(x, y, w, h);
    if nw <= 0 || nh <= 0 || canvas.y < ny || canvas.y >= ny + nh {
        return;
    }
    let radius = b.visual.corner_radius.top_left.0.max(0);
    let radius = (radius as i64 * a.scale_pct.max(1) as i64 / 100) as i32;
    if let Some(bg) = b.visual.background {
        canvas.fill_round_rect_row(nx, ny, nw, nh, radius, fade(bg, a.opacity));
    }
    paint_borders_row(canvas, nx, ny, nw, nh, radius, &b.visual.border);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_detects_no_op() {
        assert!(NodeAnim::identity(3).is_identity());
        assert!(!NodeAnim { node_id: 3, opacity: 128, dx: 0, dy: 0, scale_pct: 100 }.is_identity());
        assert!(!NodeAnim { node_id: 3, opacity: 255, dx: 4, dy: 0, scale_pct: 100 }.is_identity());
    }

    #[test]
    fn scale_keeps_center() {
        // Halving a 100x40 box about its center leaves a 50x20 box centered.
        let a = NodeAnim { node_id: 1, opacity: 255, dx: 0, dy: 0, scale_pct: 50 };
        let (nx, ny, nw, nh) = a.transform_rect(0, 0, 100, 40);
        assert_eq!((nw, nh), (50, 20));
        assert_eq!((nx, ny), (25, 10));
    }

    #[test]
    fn fade_scales_alpha() {
        let c = Rgba8 { r: 10, g: 20, b: 30, a: 200 };
        assert_eq!(fade(c, 255).a, 200);
        assert_eq!(fade(c, 0).a, 0);
        assert_eq!(fade(c, 128).a, (200u32 * 128 / 255) as u8);
    }
}
