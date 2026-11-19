// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The picking rules of a pass planned over columns (TASK-0367 P3; split out of `pass_plan.rs`,
//! the module-size ratchet): which boxes and runs may write a pixel of columns `[c0, c1)` —
//! and the reaches past a box's sides the layout diff names its rectangles by (TASK-0354 P8).
//! A pick too many costs its rows' clipped work, a pick too few loses pixels: every rule here
//! is checked against the painter by `pass_plan_tests.rs` and `pass_plan_diff_tests.rs`.

use crate::layout_diff::TextRun;
use nexus_layout::LayoutBox;

/// Columns a glyph or a stroke may paint outside its box (a side bearing, a stroked icon's round
/// cap — `pass_plan_tests` catches the icon's cap at 0): every box is widened by it when a pass
/// is planned over columns, and every damaged rectangle by at least it (`layout_diff::MARGIN`).
pub(crate) const GLYPH_OVERHANG: i32 = 2;

/// Whether box `b` may write a pixel of surface columns `[c0, c1)` (TASK-0367 P3) — the picking
/// rule of a pass planned over columns: the box, its drop shadow's reach, its text's halo and a
/// glyph's overhang, placed `dx` px left of its layout position (a scroll view's offset). An
/// animated box (`animated`) is moved and scaled by its transform, so it is always picked: a
/// pick too many only costs its rows' clipped work, a pick too few loses pixels.
pub(crate) fn meets_cols(b: &LayoutBox, dx: i32, animated: bool, (c0, c1): (i32, i32)) -> bool {
    if animated {
        return true;
    }
    let (sl, _, sr, _) = nexus_scene_raster::shadow_reach(b);
    let halo = b
        .visual
        .text_shadows
        .iter()
        .flatten()
        .map(|s| {
            nexus_text_baked::blur_reach(u32::from(s.blur_radius)) as i32
                + i32::from(s.offset_x).abs()
        })
        .max()
        .unwrap_or(0);
    let (x, over) = (b.rect.x.0 - dx, GLYPH_OVERHANG + stroke_reach(b));
    x - sl.max(halo) - over < c1 && x + b.rect.width.0 + sr.max(halo) + over > c0
}

/// Columns the run `t` may paint past the right edge of its box `b`: a box too narrow for any
/// of its text prints the ellipsis whole (`ellipsis_cut` cuts to no character, never the mark).
pub(crate) fn run_reach(b: &LayoutBox, t: &TextRun) -> i32 {
    use nexus_text_baked::measure_text::BakedTextMeasure;
    let font = match b.text_px {
        Some(px) => nexus_text_baked::FontSize::nearest(px.0, BakedTextMeasure::weight(t.weight)),
        None => t.font,
    };
    let mark = core::iter::once(nexus_text_baked::ELLIPSIS);
    (nexus_text_baked::measure(mark, font, t.tracking) as i32 - b.rect.width.0).max(0)
}

/// Whether the run `t` of box `b` may write a pixel of columns `[c0, c1)` where the box itself
/// does not ([`meets_cols`]): its ellipsis past a narrow box. Such a box is picked with its run.
pub(crate) fn run_meets_cols(b: &LayoutBox, t: &TextRun, dx: i32, (c0, c1): (i32, i32)) -> bool {
    let (x, reach) = (b.rect.x.0 - dx, run_reach(b, t));
    reach > 0 && x - GLYPH_OVERHANG < c1 && x + b.rect.width.0 + reach + GLYPH_OVERHANG > c0
}

/// Columns a stroked shape reaches past its box's sides beyond [`GLYPH_OVERHANG`]: its round
/// caps and joins lie half the stroke's width outside a path that touches the box's edge
/// (`nexus_scene_raster`'s strokes — a 32 px icon's are within the overhang, a larger one's not).
pub(crate) fn stroke_reach(b: &LayoutBox) -> i32 {
    match &b.visual.shape {
        nexus_layout_types::ShapeKind::Stroke { width_milli, .. } => {
            let side = i64::from(b.rect.width.0.min(b.rect.height.0).max(0));
            (i64::from(*width_milli) * side / 2000) as i32
        }
        _ => 0,
    }
}
