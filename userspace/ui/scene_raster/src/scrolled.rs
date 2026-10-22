// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The row loops over the retained boxes — the picked/animated/indexed painters — and
//! each box's place on the row: its own clip (the clips inside its content, RFC-0109) honoured
//! in its own space, and, for a box of a scroll layer's content the caller paints where the page
//! shows it, that layer's view (viewport + offset). A scroll layer's content is normally drawn
//! once into its band at identity and moved by the compositor; a view is passed only to flatten
//! a covered layer into the base (a modal box over it). Mirrors `interact::hit_scrolled` in the
//! DSL runtime — pixels and input agree. Split out of `lib.rs` (module-size ratchet).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: golden tests in `tests/` + dsl_apps_conformance drive these through the
//! app-host painters; `tests` below for the clip and the view.

use crate::anim;
use crate::{paint_box_row, NodeAnim, RowCanvas};
use nexus_layout::LayoutBox;

/// A scroll layer painted where the page shows it (RFC-0109): the scroll container's node id
/// (its content's `LayoutBox::scroll_root`), its viewport on the surface (x0, y0, x1, y1
/// exclusive) and its offset — a content pixel at layout `(x, y)` lands at `(x - dx, y - dy)`,
/// inside the viewport only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollView {
    pub root: usize,
    pub clip: (i32, i32, i32, i32),
    pub dx: i32,
    pub dy: i32,
}

/// Paint every box's contribution to this row, in box (z) order, at identity.
pub fn paint_row(canvas: &mut RowCanvas<'_>, boxes: &[LayoutBox]) {
    let surface_y = canvas.y;
    for b in boxes {
        paint_one(canvas, b, &[], surface_y, None);
    }
}

/// [`paint_row`] over a PRE-FILTERED index list (`pick` = indices into `boxes` that intersect the
/// repaint span), each box of a scroll layer with a view at that view. The caller computes the
/// visibility set ONCE per repaint — the per-row cost is then proportional to what is on screen,
/// not to the page's total box count. Alloc-free.
pub fn paint_row_picked(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    views: &[ScrollView],
) {
    paint_row_picked_animated(canvas, boxes, pick, views, &[]);
}

/// [`paint_row_picked`] with per-node **animation** transforms (opacity fade + translate +
/// uniform scale, keyed by `node_id`) applied to matching boxes — the paint tail of the DSL
/// `.animate`/`.transition`/`.effect` binding (docs/dev/ui/foundations/animation.md). An
/// identity/absent `NodeAnim` paints exactly as [`paint_row_picked`]. Alloc-free; `anims` is
/// bounded by the host's active-animation cap.
pub fn paint_row_picked_animated(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    views: &[ScrollView],
    anims: &[NodeAnim],
) {
    let surface_y = canvas.y;
    for &i in pick {
        let Some(b) = boxes.get(i as usize) else { continue };
        let anim = anims.iter().find(|a| a.node_id == b.node_id && !a.is_identity());
        paint_one(canvas, b, views, surface_y, anim);
    }
}

/// [`paint_row_picked_animated`] with a PRECOMPUTED per-picked-box animation index (`anim_of[k]`
/// = index into `anims` for `pick[k]`, `-1` = none) — the caller resolves the box→anim mapping
/// ONCE per repaint instead of the painter scanning the anims slice per box per row.
pub fn paint_row_picked_indexed(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    anim_of: &[i16],
    views: &[ScrollView],
    anims: &[NodeAnim],
) {
    let surface_y = canvas.y;
    for (k, &i) in pick.iter().enumerate() {
        let Some(b) = boxes.get(i as usize) else { continue };
        let anim = anim_of
            .get(k)
            .and_then(|&ai| if ai >= 0 { anims.get(ai as usize) } else { None })
            .filter(|a| !a.is_identity());
        paint_one(canvas, b, views, surface_y, anim);
    }
}

/// One box on this row: placed by its layer's view when the caller gives one (the content of a
/// covered layer flattened into the base), at identity otherwise (the base, or a band painted
/// at identity); scissored to its own clip in its own space, and to the view's viewport.
#[inline]
fn paint_one(
    canvas: &mut RowCanvas<'_>,
    b: &LayoutBox,
    views: &[ScrollView],
    surface_y: i32,
    anim: Option<&NodeAnim>,
) {
    // The canvas's own placement (a horizontal band paints its columns shifted): a view adds to
    // it, and it is restored after the box.
    let (shift0, clip0) = (canvas.shift_x, canvas.clip_x);
    let meet = |a: Option<(i32, i32)>, b: (i32, i32)| match a {
        Some((x0, x1)) => (x0.max(b.0), x1.min(b.1)),
        None => b,
    };
    let view = b.scroll_root.and_then(|r| views.iter().find(|v| v.root == r));
    let (layout_y, shift, mut clip_x) = match view {
        Some(v) => {
            if surface_y < v.clip.1 || surface_y >= v.clip.3 {
                return;
            }
            (surface_y + v.dy, shift0 + v.dx, Some(meet(clip0, (v.clip.0, v.clip.2))))
        }
        None => (surface_y, shift0, clip0),
    };
    if let Some(c) = b.clip_rect {
        let (cx0, cy0) = (c.x.0, c.y.0);
        let (cx1, cy1) = (cx0 + c.width.0, cy0 + c.height.0);
        if layout_y < cy0 || layout_y >= cy1 {
            return;
        }
        clip_x = Some(meet(clip_x, (cx0 - shift, cx1 - shift)));
    }
    canvas.y = layout_y;
    canvas.shift_x = shift;
    canvas.clip_x = clip_x;
    // Per-node animation transform (opacity fade + translate + uniform scale). A matching
    // non-identity `NodeAnim` replaces the box's fill with a transformed, alpha-scaled draw;
    // otherwise the box paints as usual. Text is faded/translated by the caller's glyph pass.
    match anim {
        Some(a) => anim::paint_anim_box_row(canvas, b, a),
        None => paint_box_row(canvas, b),
    }
    canvas.y = surface_y;
    canvas.shift_x = shift0;
    canvas.clip_x = clip0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FxPx, Rect, Rgba8, VisualStyle};

    fn filled(
        node_id: usize,
        (x, y, w, h): (i32, i32, i32, i32),
        root: Option<usize>,
    ) -> LayoutBox {
        LayoutBox {
            node_id,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            visual: VisualStyle {
                background: Some(Rgba8::new(200, 0, 0, 255)),
                ..VisualStyle::default()
            },
            scroll_root: root,
            ..LayoutBox::default()
        }
    }

    fn red_at(row: &[u8], x: usize) -> bool {
        row[x * 4 + 2] == 200
    }

    /// A box's own clip holds wherever it is painted (a `.overflow(hidden)` card), not only
    /// while something scrolls.
    #[test]
    fn a_box_is_clipped_to_its_own_clip_at_identity() {
        let mut b = filled(1, (0, 0, 20, 10), None);
        b.clip_rect = Some(Rect::new(FxPx::new(5), FxPx::new(0), FxPx::new(10), FxPx::new(4)));
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 2, 20);
        paint_row(&mut canvas, &[b.clone()]);
        assert!(!red_at(&row, 2) && red_at(&row, 6) && !red_at(&row, 16));
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 6, 20);
        paint_row(&mut canvas, &[b]);
        assert!(!red_at(&row, 6), "below the clip");
    }

    /// A covered layer flattened into the base: its content at the view's offset, inside the
    /// viewport only; without a view the same box paints at identity (into its band).
    #[test]
    fn a_layers_content_follows_its_view_and_stays_in_its_viewport() {
        let b = filled(5, (0, 100, 20, 10), Some(3));
        let view = ScrollView { root: 3, clip: (0, 40, 20, 80), dx: 0, dy: 50 };
        // Layout row 105 shows on surface row 55 with the view.
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 55, 20);
        paint_row_picked(&mut canvas, &[b.clone()], &[0], &[view]);
        assert!(red_at(&row, 3));
        // Surface row 105 is outside the viewport.
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 105, 20);
        paint_row_picked(&mut canvas, &[b.clone()], &[0], &[view]);
        assert!(!red_at(&row, 3));
        // No view: identity (the band's paint).
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 105, 20);
        paint_row_picked(&mut canvas, &[b], &[0], &[]);
        assert!(red_at(&row, 3));
    }
}
