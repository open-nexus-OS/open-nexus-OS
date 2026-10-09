// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Structural-repaint damage: the changed ROW SPAN between two retained
//! layouts.
//!
//! A `Damage::Layout` dispatch used to force a FULL re-render + full present,
//! which on the plain (non-banded) surface path is the app's whole frame
//! CPU-rastered per click — ~1.4s wall on the QEMU TCG boot for a
//! settings-sized page, long enough that a user's second click toggled the
//! menu they never saw ("no menu opens"). But most structural taps change a
//! bounded region: an overlay opens/closes, a row swaps its trailing value.
//!
//! The bound is computed by PREFIX/SUFFIX comparison of the box lists (and
//! text runs): everything outside the common prefix and suffix is the
//! changed window, and the union of those boxes' painted rows (old ∪ new) is
//! the honest damage span. Soundness: a box outside the diff window compares
//! EQUAL in geometry and visual, so its pixels cannot have changed; the
//! suffix comparison ignores `node_id` (ids shift when the structure
//! changes, pixels do not care). Text runs are diffed the same way — a pure
//! content change (same boxes) still damages its box's rows.
//!
//! Fallback is always FULL (`None`): a span above the cap, or any doubt,
//! degrades to correctness, never the other way.
//!
//! Geometry-only changes are tighter (RFC-0095, the drag gesture): when the
//! changed window keeps its structure (same nodes, same order), a box whose
//! look did not change damages only what its new geometry can change — a box
//! that paints nothing of its own (a layout container) damages nothing, a
//! flat rectangle fill damages only the rows its coverage left or took (a
//! scrim band growing by 10 px is 10 rows, not the band). A selection dragged
//! over a dimmed frame then repaints the hole's rows, not the frame.

extern crate alloc;

use nexus_layout::LayoutBox;

/// One collected text run: `(node index, content, resolved face, BGRA, the
/// REQUESTED weight)`.
///
/// The weight rides along because `FontSize` cannot give it back — a Light
/// request at 36px has already resolved to the SemiBold rung, so re-resolving
/// a `.textFit` size from the face alone would silently change weight.
///
/// SHARED alias on purpose: `probe::state`, `probe::paint::collect` and this
/// module must agree, and three hand-written copies of the tuple is exactly
/// how the OS build broke while `just check` stayed green.
pub(crate) type TextRun = (
    usize,
    alloc::string::String,
    nexus_text_baked::FontSize,
    [u8; 4],
    nexus_layout_types::FontWeight,
);

/// Pixel-equality of two boxes for damage purposes. `node_id` is
/// deliberately ignored (see module doc); `hit_slop` never paints.
fn box_pixels_eq(a: &LayoutBox, b: &LayoutBox) -> bool {
    a.rect == b.rect
        && a.z_index == b.z_index
        && a.clip_rect == b.clip_rect
        && a.overflow == b.overflow
        && a.glass_nested == b.glass_nested
        && a.visual == b.visual
        // `.textFit`: the LAYOUT-chosen size is a PIXEL property. A run whose
        // box and content are unchanged but whose fitted size stepped (the
        // window grew, the number got longer) would otherwise be declared
        // pixel-equal and never repainted.
        && a.text_px == b.text_px
}

/// The painted row extent of a box (clipped to its viewport), or `None` when
/// it paints nothing.
fn box_rows(b: &LayoutBox) -> Option<(i32, i32)> {
    if b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
        return None;
    }
    let (mut y0, mut y1) = (b.rect.y.0, b.rect.y.0 + b.rect.height.0);
    if let Some(c) = b.clip_rect {
        y0 = y0.max(c.y.0);
        y1 = y1.min(c.y.0 + c.height.0);
    }
    (y1 > y0).then_some((y0, y1))
}

/// A box's painted rectangle as `(x0, y0, x1, y1)` (clipped to its viewport);
/// `None` when it paints nothing there.
fn painted(b: &LayoutBox) -> Option<(i32, i32, i32, i32)> {
    let r = &b.rect;
    let (mut x0, mut y0, mut x1, mut y1) = (r.x.0, r.y.0, r.x.0 + r.width.0, r.y.0 + r.height.0);
    if let Some(c) = b.clip_rect {
        (x0, y0) = (x0.max(c.x.0), y0.max(c.y.0));
        (x1, y1) = (x1.min(c.x.0 + c.width.0), y1.min(c.y.0 + c.height.0));
    }
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// Paints nothing of its own: no fill, border, shadow, highlight, shape or
/// glass — a layout container (its children are boxes of their own, its text
/// a run of its own).
fn paints_nothing(v: &nexus_layout_types::VisualStyle) -> bool {
    v.background.is_none() && plain_rect(v)
}

/// A flat rectangle fill: one color over the whole rect and nothing else.
fn flat_fill(v: &nexus_layout_types::VisualStyle) -> bool {
    v.background.is_some() && plain_rect(v)
}

/// A plain rectangle: no gradient, border, corner, shadow, highlight, shape or glass.
fn plain_rect(v: &nexus_layout_types::VisualStyle) -> bool {
    use nexus_layout_types::{CornerRadius, EdgeBorder, ShapeKind, SurfaceMaterial};
    v.shape == ShapeKind::Rect
        && v.background_gradient.is_none()
        && v.border == EdgeBorder::default()
        && v.corner_radius == CornerRadius::default()
        && v.shadow.is_none()
        && v.inset_highlight.is_none()
        && v.material == SurfaceMaterial::Opaque
}

/// The rows a box's own pixels can change between two geometries of the SAME
/// look (`a` old, `b` new; RFC-0095): nothing for a box that paints nothing,
/// the rows its coverage left or took for a flat fill that kept its columns,
/// both rects otherwise.
fn geometry_rows(a: &LayoutBox, b: &LayoutBox, span: &mut Option<(i32, i32)>) {
    if paints_nothing(&a.visual) {
        return;
    }
    match (painted(a), painted(b)) {
        (Some(p), Some(q)) if flat_fill(&a.visual) && (p.0, p.2) == (q.0, q.2) => {
            union(span, Some((p.1.min(q.1), p.1.max(q.1))).filter(|r| r.1 > r.0));
            union(span, Some((p.3.min(q.3), p.3.max(q.3))).filter(|r| r.1 > r.0));
        }
        _ => {
            union(span, box_rows(a));
            union(span, box_rows(b));
        }
    }
}

/// Whether two boxes differ in geometry only (rect / viewport clip), so
/// [`geometry_rows`] may bound their damage.
fn geometry_only(a: &LayoutBox, b: &LayoutBox) -> bool {
    a.node_id == b.node_id
        && a.visual == b.visual
        && a.z_index == b.z_index
        && a.overflow == b.overflow
        && a.glass_nested == b.glass_nested
        && a.text_px == b.text_px
}

/// Whether the compositor's glass regions moved (a glass ROOT's rect, level,
/// radius or shadow — what `submit_layers` declares): only then must a
/// geometry change re-declare them.
pub(crate) fn glass_roots_changed(old: &[LayoutBox], new: &[LayoutBox]) -> bool {
    use nexus_layout_types::SurfaceMaterial;
    let roots = |boxes: &[LayoutBox]| {
        boxes
            .iter()
            .filter(|b| !b.glass_nested && matches!(b.visual.material, SurfaceMaterial::Glass(_)))
            .map(|b| (b.rect, b.visual.material, b.visual.corner_radius, b.visual.shadow.is_some()))
            .collect::<alloc::vec::Vec<_>>()
    };
    roots(old) != roots(new)
}

/// Merges a damage span into the present loop's pending state: `None` pending
/// with `dirty` = a full repaint already owed (it wins); spans union.
pub(crate) fn merge_span(dirty: &mut bool, rows: &mut Option<(i32, i32)>, span: (i32, i32)) {
    *rows = match (*dirty, *rows) {
        (true, None) => None,
        (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
        (false, None) => Some(span),
    };
    *dirty = true;
}

fn union(span: &mut Option<(i32, i32)>, add: Option<(i32, i32)>) {
    if let Some((a0, a1)) = add {
        *span = Some(match *span {
            Some((s0, s1)) => (s0.min(a0), s1.max(a1)),
            None => (a0, a1),
        });
    }
}

/// The changed row span between the OLD and NEW retained state, or `None`
/// for "repaint everything". `Some((0, 0))` = nothing visible changed.
pub(crate) fn changed_row_span(
    old_boxes: &[LayoutBox],
    new_boxes: &[LayoutBox],
    old_texts: &[TextRun],
    new_texts: &[TextRun],
    surface_h: i32,
) -> Option<(i32, i32)> {
    // ---- boxes: common prefix / suffix, changed middle ----
    let (o, n) = (old_boxes.len(), new_boxes.len());
    let mut prefix = 0;
    while prefix < o && prefix < n && box_pixels_eq(&old_boxes[prefix], &new_boxes[prefix]) {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < o - prefix
        && suffix < n - prefix
        && box_pixels_eq(&old_boxes[o - 1 - suffix], &new_boxes[n - 1 - suffix])
    {
        suffix += 1;
    }
    let mut span: Option<(i32, i32)> = None;
    let (old_win, new_win) = (&old_boxes[prefix..o - suffix], &new_boxes[prefix..n - suffix]);
    let same_shape = old_win.len() == new_win.len()
        && old_win.iter().zip(new_win).all(|(a, b)| a.node_id == b.node_id);
    if same_shape {
        // Same nodes, same order: pair them, and bound a geometry-only change.
        for (a, b) in old_win.iter().zip(new_win) {
            if box_pixels_eq(a, b) {
                continue;
            }
            if geometry_only(a, b) {
                geometry_rows(a, b, &mut span);
            } else {
                union(&mut span, box_rows(a));
                union(&mut span, box_rows(b));
            }
        }
    } else {
        for b in old_win {
            union(&mut span, box_rows(b));
        }
        for b in new_win {
            union(&mut span, box_rows(b));
        }
    }
    // ---- text runs: content/style change on an otherwise-equal box ----
    // A run's damage is its BOX's rows (glyphs never paint outside it). The
    // run stores the pre-order node index; boxes are pre-order too, so
    // `boxes[idx - 1]` is its box — guarded, a miss degrades to full.
    let (ot, nt) = (old_texts.len(), new_texts.len());
    let run_eq = |a: &TextRun, at: &[LayoutBox], b: &TextRun, bt: &[LayoutBox]| {
        if a.1 != b.1 || a.2 != b.2 || a.3 != b.3 {
            return Some(false);
        }
        let (ab, bb) = (at.get(a.0.checked_sub(1)?)?, bt.get(b.0.checked_sub(1)?)?);
        Some(ab.rect == bb.rect && ab.clip_rect == bb.clip_rect)
    };
    let mut tp = 0;
    loop {
        if tp >= ot || tp >= nt {
            break;
        }
        match run_eq(&old_texts[tp], old_boxes, &new_texts[tp], new_boxes) {
            Some(true) => tp += 1,
            Some(false) => break,
            None => return None, // index out of shape — full repaint
        }
    }
    let mut ts = 0;
    loop {
        if ts >= ot - tp || ts >= nt - tp {
            break;
        }
        match run_eq(&old_texts[ot - 1 - ts], old_boxes, &new_texts[nt - 1 - ts], new_boxes) {
            Some(true) => ts += 1,
            Some(false) => break,
            None => return None,
        }
    }
    for t in &old_texts[tp..ot - ts] {
        union(&mut span, t.0.checked_sub(1).and_then(|i| old_boxes.get(i)).and_then(box_rows));
        if t.0 == 0 || t.0 > old_boxes.len() {
            return None;
        }
    }
    for t in &new_texts[tp..nt - ts] {
        union(&mut span, t.0.checked_sub(1).and_then(|i| new_boxes.get(i)).and_then(box_rows));
        if t.0 == 0 || t.0 > new_boxes.len() {
            return None;
        }
    }
    let Some((y0, y1)) = span else {
        return Some((0, 0)); // state moved, pixels did not
    };
    // Safety margin (sub-pixel AA at span edges) + clamp; a span most of the
    // frame tall gains nothing over a full pass — degrade to full there.
    let (y0, y1) = ((y0 - 2).max(0), (y1 + 2).min(surface_h));
    if y1 <= y0 {
        return Some((0, 0));
    }
    if (y1 - y0) * 10 >= surface_h * 8 {
        return None;
    }
    Some((y0, y1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FxPx, GlassLevel, Rect, Rgba8, SurfaceMaterial, VisualStyle};

    fn boxed(id: usize, y: i32, h: i32) -> LayoutBox {
        LayoutBox {
            node_id: id,
            rect: Rect::new(FxPx::new(0), FxPx::new(y), FxPx::new(100), FxPx::new(h)),
            visual: VisualStyle {
                background: Some(Rgba8 { r: 10, g: 10, b: 10, a: 200 }),
                ..VisualStyle::default()
            },
            ..LayoutBox::default()
        }
    }

    /// The menu case: the in-flow prefix is untouched, an overlay subtree
    /// appears at the END of the box list — the span is exactly the overlay's
    /// rows (± margin), never the whole frame.
    #[test]
    fn an_appended_overlay_damages_only_its_rows() {
        let old = vec![boxed(1, 0, 40), boxed(2, 40, 400)];
        let mut menu = boxed(3, 60, 175);
        menu.visual.material = SurfaceMaterial::Glass(GlassLevel::Overlay);
        let new = vec![boxed(1, 0, 40), boxed(2, 40, 400), menu];
        let span = changed_row_span(&old, &new, &[], &[], 800);
        assert_eq!(span, Some((58, 237)), "overlay rows + 2px margin");
        // Closing it damages the same rows.
        let span = changed_row_span(&new, &old, &[], &[], 800);
        assert_eq!(span, Some((58, 237)));
    }

    /// Identical lists = empty span (state moved, pixels did not).
    #[test]
    fn identical_layouts_yield_an_empty_span() {
        let a = vec![boxed(1, 0, 40), boxed(2, 40, 400)];
        assert_eq!(changed_row_span(&a, &a.clone(), &[], &[], 800), Some((0, 0)));
    }

    /// A change spanning most of the frame degrades to FULL (`None`).
    #[test]
    fn a_near_full_change_degrades_to_full() {
        let old = vec![boxed(1, 0, 790)];
        let mut new = vec![boxed(1, 0, 790)];
        new[0].visual.background = Some(Rgba8 { r: 90, g: 10, b: 10, a: 200 });
        assert_eq!(changed_row_span(&old, &new, &[], &[], 800), None);
    }

    fn at(id: usize, x: i32, y: i32, w: i32, h: i32) -> LayoutBox {
        let mut b = boxed(id, y, h);
        b.rect = Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h));
        b
    }

    fn container(id: usize, x: i32, y: i32, w: i32, h: i32) -> LayoutBox {
        LayoutBox { visual: VisualStyle::default(), ..at(id, x, y, w, h) }
    }

    /// A flat band that grows damages only the rows it took — not the band.
    #[test]
    fn a_growing_flat_band_damages_only_the_rows_it_took() {
        let old = vec![boxed(1, 0, 100), boxed(2, 100, 700)];
        let new = vec![boxed(1, 0, 120), boxed(2, 120, 680)];
        assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((98, 122)));
    }

    /// The screenshot tool's selection (RFC-0095): four scrim bands around a
    /// framed hole. Dragging the hole 10 px down repaints the hole's rows (old
    /// ∪ new, + margin) — the bands above and below are the same scrim.
    #[test]
    fn a_dragged_selection_damages_the_hole_rows_not_the_frame() {
        let frame = |top: i32| {
            let mut hole = at(5, 120, top, 400, 280);
            hole.visual = VisualStyle::default();
            hole.visual.border = nexus_layout_types::EdgeBorder::all(
                FxPx::new(2),
                Rgba8 { r: 255, g: 255, b: 255, a: 255 },
            );
            vec![
                container(1, 0, 0, 1280, 800),
                at(2, 0, 0, 1280, top),
                container(3, 0, top, 1280, 280),
                at(4, 0, top, 120, 280),
                hole,
                at(6, 520, top, 760, 280),
                at(7, 0, top + 280, 1280, 520 - top),
            ]
        };
        let span = changed_row_span(&frame(200), &frame(210), &[], &[], 800);
        assert_eq!(span, Some((198, 492)));
    }

    /// A layout container that moves paints nothing by itself.
    #[test]
    fn a_moved_container_damages_nothing_by_itself() {
        let old = vec![container(1, 0, 0, 100, 40), boxed(2, 100, 30)];
        let new = vec![container(1, 0, 10, 100, 40), boxed(2, 100, 30)];
        assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((0, 0)));
    }

    /// A flat box that moves sideways changes both rects' rows.
    #[test]
    fn a_sideways_move_damages_both_rects() {
        let old = vec![at(1, 0, 100, 50, 40)];
        let new = vec![at(1, 30, 110, 50, 40)];
        assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((98, 152)));
    }

    /// A structure change (a node added inside the window) keeps the union.
    #[test]
    fn a_structure_change_keeps_the_union() {
        let old = vec![boxed(1, 0, 40), boxed(2, 100, 50), boxed(9, 700, 10)];
        let new = vec![boxed(1, 0, 40), boxed(2, 100, 60), boxed(3, 160, 10), boxed(9, 700, 10)];
        assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((98, 172)));
    }

    /// Glass regions are re-declared only when a glass root moved.
    #[test]
    fn glass_roots_change_only_with_a_glass_root() {
        let mut panel = boxed(3, 600, 120);
        panel.visual.material = SurfaceMaterial::Glass(GlassLevel::Panel);
        let old = vec![boxed(1, 0, 100), panel.clone()];
        let moved_band = vec![boxed(1, 0, 120), panel.clone()];
        assert!(!glass_roots_changed(&old, &moved_band));
        let mut moved_panel = panel;
        moved_panel.rect.y = FxPx::new(580);
        assert!(glass_roots_changed(&old, &[boxed(1, 0, 100), moved_panel]));
    }

    /// The present loop's span merge: a pending full repaint wins, spans union.
    #[test]
    fn merge_span_unions_and_full_wins() {
        let (mut dirty, mut rows) = (false, None);
        merge_span(&mut dirty, &mut rows, (10, 20));
        merge_span(&mut dirty, &mut rows, (5, 12));
        assert_eq!((dirty, rows), (true, Some((5, 20))));
        let (mut dirty, mut rows) = (true, None);
        merge_span(&mut dirty, &mut rows, (10, 20));
        assert_eq!((dirty, rows), (true, None));
    }

    /// A pure TEXT change on retained boxes damages the text's box rows.
    #[test]
    fn a_text_change_damages_its_box_rows() {
        let boxes = vec![boxed(1, 0, 40), boxed(2, 100, 30)];
        let old_t = vec![run(2, "Aus")];
        let new_t = vec![run(2, "An")];
        let span = changed_row_span(&boxes, &boxes.clone(), &old_t, &new_t, 800);
        assert_eq!(span, Some((98, 132)));
    }

    /// A text run for `node_id` carrying `content`, built THROUGH the
    /// [`TextRun`] alias — the tuple's shape lives in exactly one place, so a
    /// field added to the alias breaks here loudly instead of the tests
    /// silently building a differently-shaped tuple (which is how they stopped
    /// compiling when the requested weight was appended).
    fn run(node_id: usize, content: &str) -> TextRun {
        (
            node_id,
            alloc::string::String::from(content),
            nexus_text_baked::FontSize::Small,
            [0, 0, 0, 255],
            nexus_layout_types::FontWeight::Regular,
        )
    }
}
