// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: structural-repaint damage — the RECTANGLES that changed between two retained layouts
//! (TASK-0354 P8).
//!
//! A `Damage::Layout` dispatch used to force a full re-render and a full present: the app's
//! whole frame CPU-rastered per click. Most structural taps change a bounded region — an
//! overlay opens or closes, a button changes its look, a row swaps its trailing value.
//!
//! The bound is an ALIGNMENT of the box lists (and of the text runs; `align.rs`): the common
//! prefix and suffix, then a greedy order-preserving resync through what lies between. A matched
//! pair compares EQUAL in everything the painter reads (`box_pixels_eq`; `node_id` is ignored —
//! ids shift when the structure changes, pixels do not care), so it damages nothing. A box no
//! match holds adds the rectangle it paints (`paints.rs`: its shadow's and its text halo's
//! reach, cut to its clip) and a margin; one that paints nothing adds nothing. Soundness: the
//! matching is monotone, so a pixel outside every unmatched box's rectangle is covered by the
//! same equal boxes in the same order before and after. One span from the first change to the
//! last named every box between them — a bar button and five boxes far below it were the whole
//! desktop, 75 ms for every panel opened.
//!
//! What the painter decides ACROSS boxes is named with the box that causes it: a glass root
//! that comes or goes adds the boxes of the text runs it hides (the glyph pass drops a run
//! under a later glass root whole, [`hides`]). What the layout does not hold is not this
//! module's: an animated node's transform, a scroll layer's view (`probe/damage.rs`).
//!
//! No degrade to "everything" for size: a change that is most of the frame is one rectangle
//! the row painter paints. Everything remains for doubt alone — a text run without its box.
//! Past a bound (the alignment's budget, the set's four rectangles) the damage grows.
//!
//! Geometry-only changes are tighter (RFC-0095, the drag gesture): where a gap keeps its
//! structure (same nodes, same order), a box whose look did not change damages only what its
//! new geometry can change — a box that paints nothing of its own (a layout container) damages
//! nothing, a flat rectangle fill damages only the rows its coverage left or took (a scrim band
//! growing by 10 px is 10 rows, not the band). A selection dragged over a dimmed frame then
//! repaints the hole's rows, not the frame.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `layout_diff/tests.rs`, `layout_diff/rect_tests.rs`, `layout_diff/align.rs`;
//!   repainting the rectangles equals the full frame: `pass_plan_diff_tests.rs` (host)
//! INVARIANTS: repainting the named rectangles into the old frame gives the new frame; a bound
//!   costs damage, never correctness

extern crate alloc;

use crate::band_math::Pending;
use nexus_layout::LayoutBox;

mod align;
mod paints;

use paints::{
    box_of, filled, glass_root, hidden_by, painted, painted_run, paints_nothing, Edges, MARGIN,
};
pub(crate) use paints::{flat_fill, hidden_runs, hides};

/// One collected text run: `(node index, content, resolved face, BGRA, the
/// REQUESTED weight)`.
///
/// The weight rides along because `FontSize` cannot give it back — a Light
/// request at 36px has already resolved to the SemiBold rung, so re-resolving
/// a `.textFit` size from the face alone would silently change weight.
///
/// SHARED on purpose: `probe::state`, `probe::paint::collect` and this module must agree, and
/// three hand-written copies of a tuple is exactly how the OS build once broke while
/// `just check` stayed green — a named struct breaks loudly instead.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextRun {
    /// The node's pre-order id (= its `LayoutBox::node_id`).
    pub(crate) node: usize,
    pub(crate) content: alloc::string::String,
    /// The baked face the style resolves to.
    pub(crate) font: nexus_text_baked::FontSize,
    /// BGRA.
    pub(crate) color: [u8; 4],
    /// The requested weight: a `.textFit` size re-resolves with it, since the face alone cannot
    /// give it back.
    pub(crate) weight: nexus_layout_types::FontWeight,
    /// Letter-spacing (RFC-0116: the text style's tracking).
    pub(crate) tracking: nexus_text_baked::Tracking,
    /// The style's line box in px; the face's own band sits in it with half-leading.
    pub(crate) line: i32,
    /// `.textAlign`: where a run narrower than its box sits in it.
    pub(crate) align: nexus_layout_types::TextAlign,
}

/// Pixel-equality of two boxes for damage purposes: everything the painter reads from a box.
/// `node_id` is deliberately ignored (see module doc); `hit_slop` never paints. The EFFECTIVE
/// opacity is its own property (an ancestor's fade changes it and leaves the child's `visual`
/// as it was), and so is being a scroll layer's content (such a box is placed by its layer's
/// view, or drawn in its band). WHICH layer is an id, and ids shift: the layers' own change is
/// the plumbing's (`probe/damage.rs`).
fn box_pixels_eq(a: &LayoutBox, b: &LayoutBox) -> bool {
    a.rect == b.rect
        && a.z_index == b.z_index
        && a.clip_rect == b.clip_rect
        && a.overflow == b.overflow
        && a.glass_nested == b.glass_nested
        && a.visual == b.visual
        && a.opacity == b.opacity
        && a.scroll_root.is_some() == b.scroll_root.is_some()
        // `.textFit`: the LAYOUT-chosen size is a PIXEL property. A run whose
        // box and content are unchanged but whose fitted size stepped (the
        // window grew, the number got longer) would otherwise be declared
        // pixel-equal and never repainted.
        && a.text_px == b.text_px
}

/// A box no match holds: where it paints (nothing, for one that paints nothing) — and, for a
/// glass root, the boxes of the runs it hides among `runs` (they paint, or stop painting, in
/// their whole box).
fn unmatched(b: &LayoutBox, runs: &[&TextRun], boxes: &[LayoutBox], out: Out<'_>) {
    if !paints_nothing(&b.visual) {
        if let Some(r) = painted(b) {
            out(r, b.scroll_root.is_some());
        }
    }
    if glass_root(b) {
        hidden_by(b, runs.iter().copied(), boxes, &mut |r| out(r, false));
    }
}

/// Where box `b` and its run `run` paint, the margin included — the rectangles a change of them
/// names, at their layout place (RFC-0121: the base's ink, `crate::ink`). `None`: nothing.
pub(crate) fn paints_within(b: &LayoutBox, run: Option<&TextRun>) -> Option<Edges> {
    let own = if paints_nothing(&b.visual) { None } else { painted(b) };
    let r = match (own, run.and_then(|t| painted_run(b, t))) {
        (Some(a), Some(t)) => (a.0.min(t.0), a.1.min(t.1), a.2.max(t.2), a.3.max(t.3)),
        (a, t) => a.or(t)?,
    };
    Some((r.0 - MARGIN, r.1 - MARGIN, r.2 + MARGIN, r.3 + MARGIN))
}

/// Whether two boxes differ in geometry only (rect / viewport clip), so
/// [`geometry`] may bound their damage.
fn geometry_only(a: &LayoutBox, b: &LayoutBox) -> bool {
    a.node_id == b.node_id
        && a.visual == b.visual
        && a.z_index == b.z_index
        && a.overflow == b.overflow
        && a.glass_nested == b.glass_nested
        && a.opacity == b.opacity
        && a.scroll_root.is_some() == b.scroll_root.is_some()
        && a.text_px == b.text_px
}

/// Whether the compositor's regions moved (a region box's rect, material, radius, shadow or
/// opacity — what the structure's regions declare, RFC-0116: a blur root or an opaque surface):
/// only then must a geometry change re-declare them.
pub(crate) fn glass_roots_changed(old: &[LayoutBox], new: &[LayoutBox]) -> bool {
    let roots = |boxes: &[LayoutBox]| {
        boxes
            .iter()
            .filter(|b| {
                b.visual.material.is_some_and(|m| (m.blurs() && !b.glass_nested) || m.opaque())
            })
            .map(|b| {
                (
                    b.rect,
                    b.visual.material,
                    b.visual.corner_radius,
                    b.visual.shadow.is_some(),
                    b.visual.opacity,
                )
            })
            .collect::<alloc::vec::Vec<_>>()
    };
    roots(old) != roots(new)
}

/// What a relayout changed: the BASE's rectangles (everything, for a shape that gives no
/// bound), the layout rows of what changed inside the motion layers (TASK-0354 P5: their boxes
/// were never the base's pixels — their change paints the layers' store rows only; `None`:
/// nothing), the base's comparison `[old boxes, new boxes, common prefix, common suffix]` for a
/// tap's trace line — and `shifted`: the least scroll root (NEW id) whose content compared
/// equal to content of a root with another id. Node ids are positions, so a box that came or
/// went before a scroll container moves its number: the pixels are equal, but a band kept by
/// number from that id on is not known to be this content's (`probe/damage.rs` draws it anew).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Changed {
    pub(crate) base: Pending,
    pub(crate) layers: Option<(i32, i32)>,
    pub(crate) window: [u16; 4],
    pub(crate) shifted: Option<usize>,
}

/// Whether a node belongs to a motion layer: in the OLD layout, in the NEW one.
pub(crate) type Held<'a> = (&'a dyn Fn(usize) -> bool, &'a dyn Fn(usize) -> bool);

/// What the change from the OLD to the NEW retained state touched on a layout `(w, h)` large,
/// split by `held`: a box (or text run) a motion layer holds goes to the layers' rows, every
/// other to the base's rectangles — a modal opening over its scrim changes no base pixel at all.
///
/// The two parts are compared apart (TASK-0354 P8): what a layer holds was never the base's
/// pixels, so the base is aligned over the base's boxes alone.
pub(crate) fn changed_rows(
    old_boxes: &[LayoutBox],
    new_boxes: &[LayoutBox],
    (old_texts, new_texts): (&[TextRun], &[TextRun]),
    (w, h): (i32, i32),
    (held_old, held_new): Held<'_>,
) -> Changed {
    let lists = (old_boxes, new_boxes, old_texts, new_texts);
    let (base_old, base_new) = (|id: usize| !held_old(id), |id: usize| !held_new(id));
    let mut base = Pending::default();
    let size = (w.max(0) as u32, h.max(0) as u32);
    // Scroll content is placed by its layer (a band, or a view in the base): its rows are known,
    // its columns are the layer's.
    let moved = core::cell::Cell::new(None);
    let to_base = &mut |r: Edges, rows_only: bool| {
        let (x0, x1) = if rows_only { (0, w) } else { (r.0 - MARGIN, r.2 + MARGIN) };
        base.add((x0, r.1 - MARGIN, x1, r.3 + MARGIN), size);
    };
    let window = part(lists, (&base_old, &base_new), (true, &moved), to_base);
    let mut layers: Option<(i32, i32)> = None;
    let held = part(lists, (held_old, held_new), (false, &moved), &mut |r: Edges, _| {
        let (y0, y1) = ((r.1 - MARGIN).max(0), (r.3 + MARGIN).min(h));
        if y1 > y0 {
            layers = Some(layers.map_or((y0, y1), |s| (s.0.min(y0), s.1.max(y1))));
        }
    });
    let shifted = moved.get();
    match (window, held) {
        (Some(window), Some(_)) => Changed { base, layers, window, shifted },
        // Any doubt about either part's shape: everything, anew.
        _ => {
            base.whole();
            Changed { base, layers: None, window: window.unwrap_or_default(), shifted }
        }
    }
}

type Lists<'a> = (&'a [LayoutBox], &'a [LayoutBox], &'a [TextRun], &'a [TextRun]);

/// The least scroll layer (its root's NEW id) whose content was called equal to another root's.
type Shifted<'a> = &'a core::cell::Cell<Option<usize>>;

/// [`box_pixels_eq`], noting a scroll layer's content called equal to ANOTHER root's: the same
/// pixels, but a band kept under the root's number would not be this content's.
fn same(a: &LayoutBox, b: &LayoutBox, shifted: Shifted<'_>) -> bool {
    let same = box_pixels_eq(a, b);
    if let (true, Some(root)) = (same && a.scroll_root != b.scroll_root, b.scroll_root) {
        shifted.set(Some(shifted.get().map_or(root, |s| s.min(root))));
    }
    same
}

/// Where a part's change goes: a rectangle, and whether only its rows are known (scroll content).
type Out<'a> = &'a mut dyn FnMut(Edges, bool);

/// The rows a box's own pixels can change between two geometries of the SAME look (`a` old, `b`
/// new; RFC-0095): nothing for a box that paints nothing, the rows its coverage left or took —
/// in its columns — for a flat fill that kept them, both rectangles otherwise.
fn geometry(a: &LayoutBox, b: &LayoutBox, out: Out<'_>) {
    if paints_nothing(&a.visual) {
        return;
    }
    let rows_only = a.scroll_root.is_some();
    // A flat fill covers its box and no more (a halo's reach is its run's, named with the run).
    match (filled(a), filled(b)) {
        (Some(p), Some(q)) if flat_fill(&a.visual) && (p.0, p.2) == (q.0, q.2) => {
            for (y0, y1) in [(p.1.min(q.1), p.1.max(q.1)), (p.3.min(q.3), p.3.max(q.3))] {
                if y1 > y0 {
                    out((p.0, y0, p.2, y1), rows_only);
                }
            }
        }
        _ => [painted(a), painted(b)].into_iter().flatten().for_each(|r| out(r, rows_only)),
    }
}

/// What one part of the layout changed — the boxes and text runs `part` names on each side (the
/// base's, or the motion layers') — handed to `out`. Returns the boxes' comparison `[old, new,
/// common prefix, common suffix]`; `None`: a shape that gives no bound. `tight` bounds a
/// geometry-only change by what the box paints (the base); the layers' rows are the boxes' own.
fn part(
    (old_boxes, new_boxes, old_texts, new_texts): Lists<'_>,
    (in_old, in_new): Held<'_>,
    (tight, shifted): (bool, Shifted<'_>),
    out: Out<'_>,
) -> Option<[u16; 4]> {
    use alloc::vec::Vec;
    let ob: Vec<&LayoutBox> = old_boxes.iter().filter(|b| in_old(b.node_id)).collect();
    let nb: Vec<&LayoutBox> = new_boxes.iter().filter(|b| in_new(b.node_id)).collect();
    let ot: Vec<&TextRun> = old_texts.iter().filter(|t| in_old(t.node)).collect();
    let nt: Vec<&TextRun> = new_texts.iter().filter(|t| in_new(t.node)).collect();
    // ---- boxes: the common ends, then the alignment of what lies between ----
    let mut eq = |a: &&LayoutBox, b: &&LayoutBox| Some(same(a, b, shifted));
    let mut gap = |og: &[&LayoutBox], ng: &[&LayoutBox], spent: bool| {
        // Same nodes in the same order pair up (never past the budget: nothing there is
        // called equal); any other run is unmatched, box by box.
        let pairs = !spent
            && og.len() == ng.len()
            && og.iter().zip(ng).all(|(a, b)| a.node_id == b.node_id);
        if !pairs {
            og.iter().for_each(|a| unmatched(a, &ot, old_boxes, &mut *out));
            ng.iter().for_each(|b| unmatched(b, &nt, new_boxes, &mut *out));
            return Some(());
        }
        for (a, b) in og.iter().zip(ng).filter(|(a, b)| !same(a, b, shifted)) {
            // A geometry-only change is bounded by what the box paints — a glass root's also
            // moves what it hides.
            if tight && geometry_only(a, b) && !glass_root(a) {
                geometry(a, b, &mut *out);
            } else {
                unmatched(a, &ot, old_boxes, &mut *out);
                unmatched(b, &nt, new_boxes, &mut *out);
            }
        }
        Some(())
    };
    let (prefix, suffix) = align::trim(&ob, &nb, &mut eq)?;
    let (old_win, new_win) = (&ob[prefix..ob.len() - suffix], &nb[prefix..nb.len() - suffix]);
    let paired = old_win.len() == new_win.len()
        && old_win.iter().zip(new_win).all(|(a, b)| a.node_id == b.node_id);
    if paired {
        gap(old_win, new_win, false)?;
    } else {
        align::walk(old_win, new_win, &mut eq, &mut gap)?;
    }
    // ---- text runs, aligned the same way: content or style on an otherwise-equal box ----
    // A run's damage is its BOX's painted rectangle (glyphs paint inside it, their halo within
    // its reach). The run stores the pre-order node index; boxes are pre-order too, so
    // `boxes[idx - 1]` is its box — guarded, a miss gives no bound. A run is equal only on a box
    // equal in every pixel property: its fitted size, its fade and its halo are the box's.
    let mut run_eq = |a: &&TextRun, b: &&TextRun| {
        if a.content != b.content
            || a.font != b.font
            || a.color != b.color
            || a.weight != b.weight
            || a.tracking != b.tracking
            || a.line != b.line
            || a.align != b.align
        {
            return Some(false);
        }
        Some(box_pixels_eq(box_of(a, old_boxes)?, box_of(b, new_boxes)?))
    };
    let mut run_gap = |og: &[&TextRun], ng: &[&TextRun], _spent: bool| {
        for (runs, boxes) in [(og, old_boxes), (ng, new_boxes)] {
            for t in runs {
                let b = box_of(t, boxes)?;
                if let Some(r) = painted_run(b, t) {
                    out(r, b.scroll_root.is_some());
                }
            }
        }
        Some(())
    };
    let (tp, ts) = align::trim(&ot, &nt, &mut run_eq)?;
    align::walk(&ot[tp..ot.len() - ts], &nt[tp..nt.len() - ts], &mut run_eq, &mut run_gap)?;
    let short = |v: usize| v.min(usize::from(u16::MAX)) as u16;
    Some([short(ob.len()), short(nb.len()), short(prefix), short(suffix)])
}

#[cfg(test)]
mod rect_tests;
#[cfg(test)]
mod tests;
