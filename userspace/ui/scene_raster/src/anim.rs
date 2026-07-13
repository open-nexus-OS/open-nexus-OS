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

use crate::RowCanvas;
use nexus_layout::LayoutBox;
use nexus_layout_types::Rgba8;

/// Anchor sentinel: the transform scales about the box's OWN center.
const SELF_ANCHOR: i32 = i32::MIN;

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
    /// Horizontal scale, in percent (`100` = identity), about the anchor.
    /// Also the vertical scale unless `scale_y_pct` overrides it (uniform
    /// scale = the common case, and the byte-identical legacy behavior).
    pub scale_pct: u16,
    /// Vertical scale override. `None` = mirror `scale_pct` (uniform). Only
    /// non-uniform interactions set it (e.g. the toggle thumb stretching along
    /// its travel axis while pressed — the fill stays a capsule: the corner
    /// radius follows the SMALLER axis, see [`NodeAnim::radius_pct`]).
    pub scale_y_pct: Option<u16>,
    /// Scale anchor (surface px). `SELF_ANCHOR` sentinel = the box's own
    /// center. A SUBTREE cascade (interaction hover-grow on a container)
    /// derives per-child anims anchored at the CONTAINER's center, so the
    /// whole control — tile, glyph, children — grows as one shape.
    pub anchor_x: i32,
    pub anchor_y: i32,
}

impl NodeAnim {
    /// The identity (no-op) transform for `node_id`.
    #[must_use]
    pub fn identity(node_id: usize) -> Self {
        Self {
            node_id,
            opacity: 255,
            dx: 0,
            dy: 0,
            scale_pct: 100,
            scale_y_pct: None,
            anchor_x: SELF_ANCHOR,
            anchor_y: SELF_ANCHOR,
        }
    }

    /// Re-anchor this transform at an explicit center (the subtree cascade).
    #[must_use]
    pub fn anchored_at(mut self, cx: i32, cy: i32) -> Self {
        self.anchor_x = cx;
        self.anchor_y = cy;
        self
    }

    /// True when the transform has no visible effect (skip the anim path).
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.opacity == 255
            && self.dx == 0
            && self.dy == 0
            && self.scale_pct == 100
            && self.scale_y_pct.map_or(true, |v| v == 100)
    }

    /// The corner-radius scale: the SMALLER axis, so a non-uniform stretch
    /// keeps round end-caps (a stretched circle reads as a capsule, not an
    /// ellipse). For the uniform case this is exactly `scale_pct`.
    #[must_use]
    pub fn radius_pct(&self) -> u16 {
        self.scale_pct.min(self.scale_y_pct.unwrap_or(self.scale_pct))
    }

    /// The box rect after scale about the anchor + translate.
    #[must_use]
    pub fn transform_rect(&self, x: i32, y: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
        let s = self.scale_pct.max(1) as i64;
        let sy = self.scale_y_pct.map_or(s, |v| v.max(1) as i64);
        let nw = (w as i64 * s / 100) as i32;
        let nh = (h as i64 * sy / 100) as i32;
        let (nx, ny) = if self.anchor_x == SELF_ANCHOR {
            // Keep the box's own center fixed.
            (x + (w - nw) / 2, y + (h - nh) / 2)
        } else {
            // Scale the box's position about the shared anchor so a whole
            // subtree moves coherently outward/inward.
            (
                (self.anchor_x as i64 + (x as i64 - self.anchor_x as i64) * s / 100) as i32,
                (self.anchor_y as i64 + (y as i64 - self.anchor_y as i64) * sy / 100) as i32,
            )
        };
        (nx + self.dx, ny + self.dy, nw, nh)
    }
}

/// Multiplies a color's alpha by `opacity/255` (fade toward the page base via
/// src-over when this fill draws over the already-painted background).
#[inline]
fn fade(c: Rgba8, opacity: u8) -> Rgba8 {
    let a = (c.a as u32 * opacity as u32 / 255) as u8;
    Rgba8 { r: c.r, g: c.g, b: c.b, a }
}

/// Paints one animated box for the current row, transformed and alpha-scaled,
/// through the SHARED shape dispatch (`paint_box_row_at`) — every `ShapeKind`
/// (rect/triangles/circle/path/vector) scales + translates, so icon glyphs and
/// round controls animate as whole shapes (the interaction hover-grow /
/// press-bounce contract), not just their bounding fill.
pub(crate) fn paint_anim_box_row(canvas: &mut RowCanvas<'_>, b: &LayoutBox, a: &NodeAnim) {
    let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    if w <= 0 || h <= 0 {
        return;
    }
    let (nx, ny, nw, nh) = a.transform_rect(x, y, w, h);
    let bg = b.visual.background.map(|c| fade(c, a.opacity));
    crate::paint_box_row_at(canvas, b, nx, ny, nw, nh, bg, a.radius_pct());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_detects_no_op() {
        assert!(NodeAnim::identity(3).is_identity());
        assert!(!NodeAnim { opacity: 128, ..NodeAnim::identity(3) }.is_identity());
        assert!(!NodeAnim { dx: 4, ..NodeAnim::identity(3) }.is_identity());
    }

    #[test]
    fn scale_keeps_center() {
        // Halving a 100x40 box about its center leaves a 50x20 box centered.
        let a = NodeAnim { scale_pct: 50, ..NodeAnim::identity(1) };
        let (nx, ny, nw, nh) = a.transform_rect(0, 0, 100, 40);
        assert_eq!((nw, nh), (50, 20));
        assert_eq!((nx, ny), (25, 10));
    }

    #[test]
    fn non_uniform_stretch_is_superset() {
        // X-stretch with pinned Y: width grows, height stays, radius follows
        // the SMALLER axis (capsule contract) — and the uniform path is
        // untouched when scale_y_pct is None.
        let a = NodeAnim { scale_pct: 150, scale_y_pct: Some(100), ..NodeAnim::identity(1) };
        let (nx, ny, nw, nh) = a.transform_rect(10, 20, 40, 20);
        assert_eq!((nw, nh), (60, 20));
        assert_eq!((nx, ny), (0, 20)); // centered in x, pinned in y
        assert_eq!(a.radius_pct(), 100);
        assert!(!a.is_identity());
        assert!(NodeAnim { scale_y_pct: Some(100), ..NodeAnim::identity(1) }.is_identity());
        let uniform = NodeAnim { scale_pct: 50, ..NodeAnim::identity(1) };
        assert_eq!(uniform.radius_pct(), 50);
        assert_eq!(uniform.transform_rect(0, 0, 100, 40), (25, 10, 50, 20));
    }

    #[test]
    fn fade_scales_alpha() {
        let c = Rgba8 { r: 10, g: 20, b: 30, a: 200 };
        assert_eq!(fade(c, 255).a, 200);
        assert_eq!(fade(c, 0).a, 0);
        assert_eq!(fade(c, 128).a, (200u32 * 128 / 255) as u8);
    }
}
