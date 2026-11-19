// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Where a box paints (TASK-0354 P8; split out of `layout_diff.rs`, the module-size ratchet):
//! the rectangle a box's own pixels and its run's lie in — shadow, halo, a stroke's caps, an
//! ellipsis past a narrow box — and the one cross-box rule of the painter, a glass root hiding
//! the text runs under it. The damage the diff names is made of these rectangles, so each is
//! checked against the painter itself (`pass_plan_diff_tests.rs`): one too small leaves old
//! pixels standing.

use super::TextRun;
use nexus_layout::LayoutBox;

/// Edges `(x0, y0, x1, y1)` of a rectangle in layout pixels.
pub(super) type Edges = (i32, i32, i32, i32);

/// Pixels around a painted rectangle that are damaged with it: a glyph's side bearing and a
/// stroked icon's round cap reach past their box (`pass_plan::GLYPH_OVERHANG`), and an edge's
/// anti-aliasing never ends flat.
pub(super) const MARGIN: i32 = 2;
const _: () = assert!(MARGIN >= crate::pass_plan::GLYPH_OVERHANG);

/// A box's painted rectangle — its shadows' reach included (`shadow_reach`: the drop shadow and
/// the text halo), and a stroked shape's caps at its sides — clipped to its viewport; `None`
/// when it paints nothing there.
pub(super) fn painted(b: &LayoutBox) -> Option<Edges> {
    reaching(b, 0)
}

/// Where the run `t` of box `b` paints: the box's painted rectangle, and past its right edge
/// what a box too narrow for its text lets the ellipsis reach (`pass_plan::run_reach`).
pub(super) fn painted_run(b: &LayoutBox, t: &TextRun) -> Option<Edges> {
    reaching(b, crate::pass_plan::run_reach(b, t))
}

/// [`painted`], reaching at least `right` px past the box's right edge.
fn reaching(b: &LayoutBox, right: i32) -> Option<Edges> {
    let (l, t, rr, bo) = nexus_scene_raster::shadow_reach(b);
    let cap = crate::pass_plan::stroke_reach(b);
    clipped(b, (l.max(cap), t, rr.max(cap).max(right), bo))
}

/// What a flat fill covers: its box in its viewport, no reach (a halo's is its run's).
pub(super) fn filled(b: &LayoutBox) -> Option<Edges> {
    clipped(b, (0, 0, 0, 0))
}

/// A box's rectangle grown by `(left, top, right, bottom)` and cut to its viewport.
fn clipped(b: &LayoutBox, (l, t, rr, bo): Edges) -> Option<Edges> {
    let r = &b.rect;
    if r.width.0 <= 0 || r.height.0 <= 0 {
        return None; // an empty box paints nothing, its shadow neither
    }
    let (mut x0, mut y0, mut x1, mut y1) =
        (r.x.0 - l, r.y.0 - t, r.x.0 + r.width.0 + rr, r.y.0 + r.height.0 + bo);
    if let Some(c) = b.clip_rect {
        (x0, y0) = (x0.max(c.x.0), y0.max(c.y.0));
        (x1, y1) = (x1.min(c.x.0 + c.width.0), y1.min(c.y.0 + c.height.0));
    }
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// Paints nothing of its own: no fill, border, shadow, highlight, shape or
/// glass — a layout container (its children are boxes of their own, its text
/// a run of its own).
pub(super) fn paints_nothing(v: &nexus_layout_types::VisualStyle) -> bool {
    v.background.is_none() && plain_rect(v)
}

/// A flat rectangle fill: one color over the whole rect and nothing else.
pub(crate) fn flat_fill(v: &nexus_layout_types::VisualStyle) -> bool {
    v.background.is_some() && plain_rect(v)
}

/// A plain rectangle: no gradient, border, corner, shadow, highlight, shape or glass.
fn plain_rect(v: &nexus_layout_types::VisualStyle) -> bool {
    use nexus_layout_types::{CornerRadius, EdgeBorder, ShapeKind};
    v.shape == ShapeKind::Rect
        && v.background_gradient.is_none()
        && v.border == EdgeBorder::default()
        && v.corner_radius == CornerRadius::default()
        && v.shadow.is_none()
        && v.inset_highlight.is_none()
        && v.material.is_none()
}

/// Whether the glyph pass drops the text run of box `t` under box `g` (`probe/paint.rs`, the one
/// rule): `g` is a glass root drawn after `t` whose box meets `t`'s. Text paints after every
/// fill, so a run under a later glass root would print over the root's reset fill — the run is
/// dropped WHOLE, not only where the root covers it.
pub(crate) fn hides(g: &LayoutBox, t: &LayoutBox) -> bool {
    g.node_id > t.node_id
        && glass_root(g)
        && t.rect.x.0 < g.rect.x.0 + g.rect.width.0
        && t.rect.x.0 + t.rect.width.0 > g.rect.x.0
        && t.rect.y.0 < g.rect.y.0 + g.rect.height.0
        && t.rect.y.0 + t.rect.height.0 > g.rect.y.0
}

/// A glass root: a blur root or an opaque surface that is not nested in another glass.
pub(super) fn glass_root(b: &LayoutBox) -> bool {
    b.visual.material.is_some_and(|m| m.blurs() || m.opaque()) && !b.glass_nested
}

/// A run's box: boxes are pre-order like the run's node index (`None`: out of shape).
pub(super) fn box_of<'a>(t: &TextRun, boxes: &'a [LayoutBox]) -> Option<&'a LayoutBox> {
    t.node.checked_sub(1).and_then(|i| boxes.get(i))
}

/// Where the runs among `runs` that glass root `g` hides paint, handed to `out`: they print, or
/// stop printing, in their whole box when the root comes, goes or moves.
pub(super) fn hidden_by<'a>(
    g: &LayoutBox,
    runs: impl Iterator<Item = &'a TextRun>,
    boxes: &[LayoutBox],
    out: &mut dyn FnMut(Edges),
) {
    for t in runs {
        let Some(tb) = box_of(t, boxes).filter(|tb| hides(g, tb)) else { continue };
        if let Some(r) = painted_run(tb, t) {
            out(r);
        }
    }
}

/// The runs the glass roots among nodes `[first, end)` hide, where they paint (margin included):
/// named when those nodes leave the base's pass or come back to it with no relayout to tell (a
/// motion layer taking its subtree, `probe/damage.rs`).
pub(crate) fn hidden_runs(
    boxes: &[LayoutBox],
    texts: &[TextRun],
    (first, end): (usize, usize),
    out: &mut dyn FnMut(Edges),
) {
    let mut wide = |r: Edges| out((r.0 - MARGIN, r.1 - MARGIN, r.2 + MARGIN, r.3 + MARGIN));
    for g in boxes.iter().filter(|g| g.node_id >= first && g.node_id < end && glass_root(g)) {
        hidden_by(g, texts.iter(), boxes, &mut wide);
    }
}
