// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-node **animation transform** for the CPU scene painter — the
//! paint tail of the DSL `.animate`/`.transition`/`.effect` binding
//! (docs/dev/ui/foundations/animation.md, ADR-0031). A [`NodeAnim`] keyed by
//! `LayoutBox::node_id` carries the animation engine's current interpolated
//! opacity + translate + scale for one node — and, beyond those, a rotation of
//! its glyph shape, a base opacity (a box whose scene rest is hidden, painted
//! during its exit) and a rounded clip (`xform`); the picked painter draws
//! that node's fill transformed and alpha-scaled instead of at rest. Like the
//! `ScrollView` (a box placed by its scroll layer's view) it is
//! presentation-only — never a re-layout, alloc-free.
//! OWNERS: @ui
//! STATUS: In progress (TASK-0062/0075 DSL animation binding)
//!
//! SCOPE: the fill (`ShapeKind::Rect`, rounded) + border transform. Non-rect
//! shapes on an animated node fall back to an identity draw (rare — animated
//! DSL nodes are panels/bars/text); text is faded/translated by the caller's
//! separate glyph pass, not here.

use crate::{RoundClip, RowCanvas};
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
    /// A motion's rotation, degrees clockwise, ADDED to the box's rest rotation
    /// (`VisualStyle::rotate_deg`); `0` = the rest rotation alone. Turns the
    /// box's own glyph shape about the drawn rect's centre, ink cut to that rect.
    pub rot_deg: i16,
    /// The box painted as if its effective opacity (`LayoutBox::opacity`) were
    /// this value; `None` = its own. A box whose scene opacity is 0 is painted
    /// through this during its exit (the scene is the rest, the motion the
    /// deviation from it).
    pub base_opacity: Option<u8>,
    /// A rounded rect the node is painted under (a pip or a band entering its
    /// track from outside): nothing of the node is painted outside it, and at
    /// its rounded ends the node takes the coverage the rect's own fill has.
    pub clip: Option<RoundClip>,
    /// The exclusive end of the pre-order node range this transform covers: the node itself
    /// (`node_id + 1`), or its whole subtree (a pressed control dips its glyph and label with it,
    /// a swapped month slides every cell). A box with its own entry keeps that one
    /// ([`covering_index`]).
    pub span_end: usize,
}

/// The transform box `node_id` is painted with: its own entry, else the innermost entry whose
/// subtree covers it (`a.node_id < node_id < a.span_end`, the largest `a.node_id`). Its index in
/// `anims`.
#[must_use]
pub fn covering_index(anims: &[NodeAnim], node_id: usize) -> Option<usize> {
    if let Some(i) = anims.iter().position(|a| a.node_id == node_id) {
        return Some(i);
    }
    anims
        .iter()
        .enumerate()
        .filter(|(_, a)| a.node_id < node_id && node_id < a.span_end)
        .max_by_key(|(_, a)| a.node_id)
        .map(|(i, _)| i)
}

impl NodeAnim {
    /// The identity (no-op) transform for `node_id`.
    #[must_use]
    pub const fn identity(node_id: usize) -> Self {
        Self {
            node_id,
            opacity: 255,
            dx: 0,
            dy: 0,
            scale_pct: 100,
            scale_y_pct: None,
            anchor_x: SELF_ANCHOR,
            anchor_y: SELF_ANCHOR,
            rot_deg: 0,
            base_opacity: None,
            clip: None,
            span_end: node_id + 1,
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
            && self.scale_y_pct.is_none_or(|v| v == 100)
            && self.rot_deg == 0
            && self.base_opacity.is_none()
            && self.clip.is_none()
    }

    /// The corner-radius scale. A uniform scale scales the radius with the
    /// box (`scale_pct`). A SPLIT axis (`scale_y_pct` set) keeps the radius
    /// as authored (100): the fill clamps it to half the smaller side, so a
    /// capsule stretched or squeezed along one axis stays a capsule with
    /// round end-caps instead of ends that square off as it shrinks.
    #[must_use]
    pub fn radius_pct(&self) -> u16 {
        if self.scale_y_pct.is_some() {
            100
        } else {
            self.scale_pct
        }
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
    let turn = i32::from(b.visual.rotate_deg) + i32::from(a.rot_deg);
    let opacity = a.base_opacity.unwrap_or(b.opacity);
    crate::paint_box_row_at(canvas, b, nx, ny, nw, nh, bg, a.radius_pct(), turn, opacity);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box takes its own entry, else the innermost one whose subtree covers it.
    #[test]
    fn a_box_takes_its_own_transform_else_the_innermost_covering_one() {
        let outer = NodeAnim { span_end: 10, dx: 4, ..NodeAnim::identity(2) };
        let inner = NodeAnim { span_end: 7, dx: 9, ..NodeAnim::identity(4) };
        let own = NodeAnim { dx: 1, ..NodeAnim::identity(6) };
        let anims = [outer, inner, own];
        assert_eq!(covering_index(&anims, 3), Some(0), "under the outer only");
        assert_eq!(covering_index(&anims, 5), Some(1), "the inner subtree wins");
        assert_eq!(covering_index(&anims, 6), Some(2), "its own entry wins");
        assert_eq!(covering_index(&anims, 8), Some(0), "past the inner's end");
        assert_eq!(covering_index(&anims, 10), None, "the end is exclusive");
        assert_eq!(covering_index(&anims, 1), None, "before every range");
    }

    #[test]
    fn identity_detects_no_op() {
        assert!(NodeAnim::identity(3).is_identity());
        assert!(!NodeAnim { opacity: 128, ..NodeAnim::identity(3) }.is_identity());
        assert!(
            NodeAnim { span_end: 9, ..NodeAnim::identity(3) }.is_identity(),
            "a range alone moves nothing"
        );
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
