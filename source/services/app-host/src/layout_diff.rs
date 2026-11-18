// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Structural-repaint damage: the changed ROW SPAN between two retained
//! layouts.
//!
//! A `Damage::Layout` dispatch used to force a FULL re-render + full present,
//! which was the app's whole frame
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

/// The painted row extent of a box — the box and the rows its own shadow reaches
/// (`nexus_scene_raster::shadow_reach`) — clipped to its viewport, or `None` when it paints
/// nothing.
fn box_rows(b: &LayoutBox) -> Option<(i32, i32)> {
    if b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
        return None;
    }
    let (_, above, _, below) = nexus_scene_raster::shadow_reach(b);
    let (mut y0, mut y1) = (b.rect.y.0 - above, b.rect.y.0 + b.rect.height.0 + below);
    if let Some(c) = b.clip_rect {
        y0 = y0.max(c.y.0);
        y1 = y1.min(c.y.0 + c.height.0);
    }
    (y1 > y0).then_some((y0, y1))
}

/// A box's painted rectangle as `(x0, y0, x1, y1)` — its shadow's reach included — clipped to
/// its viewport; `None` when it paints nothing there.
fn painted(b: &LayoutBox) -> Option<(i32, i32, i32, i32)> {
    let r = &b.rect;
    if r.width.0 <= 0 || r.height.0 <= 0 {
        return None; // an empty box paints nothing, its shadow neither
    }
    let (l, t, rr, bo) = nexus_scene_raster::shadow_reach(b);
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
fn paints_nothing(v: &nexus_layout_types::VisualStyle) -> bool {
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
#[cfg(test)]
pub(crate) fn changed_row_span(
    old_boxes: &[LayoutBox],
    new_boxes: &[LayoutBox],
    old_texts: &[TextRun],
    new_texts: &[TextRun],
    surface_h: i32,
) -> Option<(i32, i32)> {
    let none = |_: usize| false;
    changed_rows(old_boxes, new_boxes, (old_texts, new_texts), surface_h, (&none, &none)).base
}

/// What a relayout changed: the BASE's rows (`None`: everything, `Some((0, 0))`: nothing — the
/// [`changed_row_span`] convention) and the layout rows of what changed inside the motion layers
/// (TASK-0354 P5: their boxes were never the base's pixels — their change paints the layers'
/// store rows only; `None`: nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Changed {
    pub(crate) base: Option<(i32, i32)>,
    pub(crate) layers: Option<(i32, i32)>,
}

/// Whether a node belongs to a motion layer: in the OLD layout, in the NEW one.
pub(crate) type Held<'a> = (&'a dyn Fn(usize) -> bool, &'a dyn Fn(usize) -> bool);

/// The rows the change from the OLD to the NEW retained state touched, split by `held`: a box (or
/// text run) a motion layer holds goes to the layers' rows, every other to the base's — a modal
/// opening over its scrim changes no base row at all.
///
/// The two parts are compared apart (TASK-0354 P8): what a layer holds was never the base's
/// pixels, so the base's window is trimmed over the base's boxes alone. One window over both
/// reached from a bar button that changed its look (near the start of the list) to its panel
/// (at the end, in a layer) and named every row between them: the whole desktop painted again
/// for each panel opened and closed.
pub(crate) fn changed_rows(
    old_boxes: &[LayoutBox],
    new_boxes: &[LayoutBox],
    (old_texts, new_texts): (&[TextRun], &[TextRun]),
    surface_h: i32,
    (held_old, held_new): Held<'_>,
) -> Changed {
    let lists = (old_boxes, new_boxes, old_texts, new_texts);
    let (base_old, base_new) = (|id: usize| !held_old(id), |id: usize| !held_new(id));
    let parts = part_rows(lists, (&base_old, &base_new), true)
        .and_then(|base| Some((base, part_rows(lists, (held_old, held_new), false)?)));
    // Any doubt about either part's shape: everything, anew.
    let Some((base, layers)) = parts else { return Changed { base: None, layers: None } };
    // Safety margin (sub-pixel AA at span edges) + clamp.
    let margin = |span: Option<(i32, i32)>| {
        span.map(|(y0, y1)| ((y0 - 2).max(0), (y1 + 2).min(surface_h))).filter(|s| s.1 > s.0)
    };
    let base = match margin(base) {
        None => Some((0, 0)), // state moved, the base's pixels did not
        // A span most of the frame tall gains nothing over a full pass — degrade to full there.
        Some((y0, y1)) if (y1 - y0) * 10 >= surface_h * 8 => None,
        rows => rows,
    };
    Changed { base, layers: margin(layers) }
}

/// A run's box: boxes are pre-order like the run's node index (`None`: out of shape).
fn box_of<'a>(t: &TextRun, boxes: &'a [LayoutBox]) -> Option<&'a LayoutBox> {
    t.node.checked_sub(1).and_then(|i| boxes.get(i))
}

type Lists<'a> = (&'a [LayoutBox], &'a [LayoutBox], &'a [TextRun], &'a [TextRun]);

/// The rows one part of the layout changed — the boxes and text runs `part` names on each side
/// (the base's, or the motion layers'): `Some(None)` nothing, `None` a shape that gives no bound.
/// `tight` bounds a geometry-only change by what the box paints (the base); the layers' rows are
/// the boxes' own.
fn part_rows(
    (old_boxes, new_boxes, old_texts, new_texts): Lists<'_>,
    (in_old, in_new): Held<'_>,
    tight: bool,
) -> Option<Option<(i32, i32)>> {
    use alloc::vec::Vec;
    let ob: Vec<&LayoutBox> = old_boxes.iter().filter(|b| in_old(b.node_id)).collect();
    let nb: Vec<&LayoutBox> = new_boxes.iter().filter(|b| in_new(b.node_id)).collect();
    // ---- boxes: common prefix / suffix, changed middle ----
    let prefix = ob.iter().zip(&nb).take_while(|(a, b)| box_pixels_eq(a, b)).count();
    let suffix = ob[prefix..]
        .iter()
        .rev()
        .zip(nb[prefix..].iter().rev())
        .take_while(|(a, b)| box_pixels_eq(a, b))
        .count();
    let mut span: Option<(i32, i32)> = None;
    let (old_win, new_win) = (&ob[prefix..ob.len() - suffix], &nb[prefix..nb.len() - suffix]);
    if tight {
        WINDOW.store(
            [ob.len(), nb.len(), prefix, suffix]
                .iter()
                .fold(0u64, |w, &v| (w << 16) | v.min(usize::from(u16::MAX)) as u64),
            core::sync::atomic::Ordering::Relaxed,
        );
    }
    let same_shape = old_win.len() == new_win.len()
        && old_win.iter().zip(new_win).all(|(a, b)| a.node_id == b.node_id);
    if same_shape {
        // Same nodes, same order: pair them, and bound a geometry-only change.
        for (a, b) in old_win.iter().zip(new_win) {
            if box_pixels_eq(a, b) {
                continue;
            }
            if tight && geometry_only(a, b) {
                geometry_rows(a, b, &mut span);
            } else {
                union(&mut span, box_rows(a));
                union(&mut span, box_rows(b));
            }
        }
    } else {
        for b in old_win.iter().chain(new_win) {
            union(&mut span, box_rows(b));
        }
    }
    // ---- text runs: content/style change on an otherwise-equal box ----
    // A run's damage is its BOX's rows (glyphs never paint outside it). The
    // run stores the pre-order node index; boxes are pre-order too, so
    // `boxes[idx - 1]` is its box — guarded, a miss degrades to full.
    let ot: Vec<&TextRun> = old_texts.iter().filter(|t| in_old(t.node)).collect();
    let nt: Vec<&TextRun> = new_texts.iter().filter(|t| in_new(t.node)).collect();
    let run_eq = |a: &TextRun, b: &TextRun| {
        if a.content != b.content
            || a.font != b.font
            || a.color != b.color
            || a.tracking != b.tracking
            || a.line != b.line
            || a.align != b.align
        {
            return Some(false);
        }
        let (ab, bb) = (box_of(a, old_boxes)?, box_of(b, new_boxes)?);
        Some(ab.rect == bb.rect && ab.clip_rect == bb.clip_rect)
    };
    let common = ot.len().min(nt.len());
    let mut tp = 0;
    while tp < common && run_eq(ot[tp], nt[tp])? {
        tp += 1;
    }
    let mut ts = 0;
    while ts < common - tp && run_eq(ot[ot.len() - 1 - ts], nt[nt.len() - 1 - ts])? {
        ts += 1;
    }
    for t in &ot[tp..ot.len() - ts] {
        union(&mut span, box_rows(box_of(t, old_boxes)?));
    }
    for t in &nt[tp..nt.len() - ts] {
        union(&mut span, box_rows(box_of(t, new_boxes)?));
    }
    Some(span)
}

/// The base's last comparison `[old boxes, new boxes, common prefix, common suffix]` — what a
/// tap's trace says when its change named every row.
static WINDOW: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The base's last comparison ([`WINDOW`]).
pub(crate) fn last_window() -> [u16; 4] {
    let w = WINDOW.load(core::sync::atomic::Ordering::Relaxed);
    [(w >> 48) as u16, (w >> 32) as u16, (w >> 16) as u16, w as u16]
}

#[cfg(test)]
mod tests;
