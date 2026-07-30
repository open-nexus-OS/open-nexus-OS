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

extern crate alloc;

use nexus_layout::LayoutBox;

type TextRun = (usize, alloc::string::String, nexus_text_baked::FontSize, [u8; 4]);

/// Pixel-equality of two boxes for damage purposes. `node_id` is
/// deliberately ignored (see module doc); `hit_slop` never paints.
fn box_pixels_eq(a: &LayoutBox, b: &LayoutBox) -> bool {
    a.rect == b.rect
        && a.z_index == b.z_index
        && a.clip_rect == b.clip_rect
        && a.overflow == b.overflow
        && a.glass_nested == b.glass_nested
        && a.visual == b.visual
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
    for b in &old_boxes[prefix..o - suffix] {
        union(&mut span, box_rows(b));
    }
    for b in &new_boxes[prefix..n - suffix] {
        union(&mut span, box_rows(b));
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
        let new = vec![boxed(1, 0, 789)];
        assert_eq!(changed_row_span(&old, &new, &[], &[], 800), None);
    }

    /// A pure TEXT change on retained boxes damages the text's box rows.
    #[test]
    fn a_text_change_damages_its_box_rows() {
        let boxes = vec![boxed(1, 0, 40), boxed(2, 100, 30)];
        let old_t = vec![(2usize, alloc_string("Aus"), font(), [0, 0, 0, 255])];
        let new_t = vec![(2usize, alloc_string("An"), font(), [0, 0, 0, 255])];
        let span = changed_row_span(&boxes, &boxes.clone(), &old_t, &new_t, 800);
        assert_eq!(span, Some((98, 132)));
    }

    fn alloc_string(s: &str) -> alloc::string::String {
        alloc::string::String::from(s)
    }
    fn font() -> nexus_text_baked::FontSize {
        nexus_text_baked::FontSize::Small
    }
}
