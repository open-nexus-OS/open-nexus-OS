// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a paint pass planned once (TASK-0367 P1). The pass picks its boxes, their animation
//! slots and their text runs once (`probe/paint.rs::prepare_picks_with`), snapshots the animated
//! nodes' transforms and the focused field's caret, and hands the row painter this plan: borrowed
//! slices and copied values, nothing a row writes back. So any band of the pass's rows is painted
//! from the plan by any thread, byte-equal to painting the rows one after another — the ground
//! fill, the boxes by the shared painter (`nexus-scene-raster`), the glyph runs with their
//! selection and shadows, the straight-alpha conversion (RFC-0109). P2 hands the bands to the
//! workpool; until then the probe paints them in order on its own thread.
//! NOT target-gated (compiled for the OS build and for host tests), so the band parity runs on
//! the host.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `pass_plan_tests.rs` (host): band parity over splits 1..8 painted on threads,
//!   bands in any order, a row alone equal to its row in the whole; the plan is `Sync`
//! INVARIANTS: painting reads the plan only; a row's pixels depend on its row index and the plan,
//!   never on which rows were painted before it

use nexus_layout::LayoutBox;
use nexus_scene_raster::{NodeAnim, ScrollView};

use crate::layout_diff::TextRun;
use crate::straight_alpha::unpremultiply;

/// The focused field's selection for the row painters (TASK-0067B): the text node it belongs
/// to, the selection's x span (relative to the run's origin) and its highlight (accent,
/// translucent). The caret is the compositor's (RFC-0113 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaretPaint {
    pub(crate) nid: usize,
    pub(crate) selection: Option<(i32, i32)>,
    pub(crate) highlight: [u8; 4],
    /// The field's horizontal text offset (TASK-0346 P2: its caret stays in view).
    pub(crate) scroll: i32,
}

/// One pass, planned: what its rows read. Built once per pass, then only read.
#[derive(Clone, Copy)]
pub(crate) struct PassPlan<'a> {
    /// The surface's width in px (a row's canvas).
    pub(crate) width: i32,
    pub(crate) boxes: &'a [LayoutBox],
    /// The boxes that paint in this pass, in paint order (indices into `boxes`).
    pub(crate) picks: &'a [u32],
    /// Per pick: its slot in `anims`, `-1` without one.
    pub(crate) pick_anims: &'a [i16],
    /// The text runs that paint in this pass: `(box index, run index into texts)`.
    pub(crate) runs: &'a [(u32, u32)],
    pub(crate) texts: &'a [TextRun],
    /// The animated nodes' transforms this frame.
    pub(crate) anims: &'a [NodeAnim],
    pub(crate) caret: Option<CaretPaint>,
    /// The scroll views a covered layer's content is placed at.
    pub(crate) views: &'a [ScrollView],
}

impl PassPlan<'_> {
    /// Row `y` of the base over `ground` (premultiplied BGRA): filled, painted, left in the
    /// straight alpha the compositor samples. The base pass's row, whoever paints it.
    pub(crate) fn paint_ground_row(&self, row: &mut [u8], y: i32, ground: [u8; 4]) {
        nexus_paint_kernels::fill(row, ground);
        self.paint_row(row, y, 0);
        unpremultiply(row);
    }

    /// Columns `[c0, c1)` of base row `y` over `ground` (TASK-0367 P3: a row costs its damaged
    /// columns, not its width): byte-equal there to [`Self::paint_ground_row`]; every pixel
    /// outside them is left as the row held it. A pass planned over those columns
    /// (`prepare_picks_with`'s `cols`) gives the same pixels as one planned over the whole row.
    pub(crate) fn paint_ground_cols(
        &self,
        row: &mut [u8],
        y: i32,
        ground: [u8; 4],
        (c0, c1): (i32, i32),
    ) {
        let n = (row.len() / 4) as i32;
        let (c0, c1) = (c0.clamp(0, n), c1.clamp(0, n));
        if c1 <= c0 {
            return;
        }
        let span = c0 as usize * 4..c1 as usize * 4;
        nexus_paint_kernels::fill(&mut row[span.clone()], ground);
        self.paint_row_boxes(row, y, 0, Some((c0, c1)));
        self.paint_row_text(row, y, 0, Some((c0, c1)));
        unpremultiply(&mut row[span]);
    }

    /// The rows `[y0, y0 + n)` of the base held one after another in `band` (`n` whole rows of
    /// `width` px): each as [`Self::paint_ground_row`] paints it alone. The unit a worker paints
    /// (P2, TASK-0366's workpool); until a worker calls it, only the band parity does.
    #[cfg(test)]
    pub(crate) fn paint_ground_band(&self, band: &mut [u8], y0: i32, ground: [u8; 4]) {
        let row_bytes = self.width.max(0) as usize * 4;
        if row_bytes == 0 {
            return;
        }
        for (i, row) in band.chunks_exact_mut(row_bytes).enumerate() {
            self.paint_ground_row(row, y0 + i as i32, ground);
        }
    }

    /// Paints row `y` (surface row of the base; layout row of a band) over what `row` holds:
    /// the picked boxes' fills by the shared painter, then their glyph runs. `shift` moves the
    /// canvas (a horizontal band's run of columns).
    pub(crate) fn paint_row(&self, row: &mut [u8], y: i32, shift: i32) {
        self.paint_row_boxes(row, y, shift, None);
        self.paint_row_text(row, y, shift, None);
    }

    /// Row `y`'s picked boxes, by the shared painter; with `cols`, only the surface columns
    /// `[c0, c1)` are written (every painter writes through the canvas's scissor).
    pub(crate) fn paint_row_boxes(
        &self,
        row: &mut [u8],
        y: i32,
        shift: i32,
        cols: Option<(i32, i32)>,
    ) {
        let mut canvas = nexus_scene_raster::RowCanvas::new(row, y, self.width);
        canvas.shift_x = shift;
        canvas.clip_x = cols;
        nexus_scene_raster::paint_row_picked_indexed(
            &mut canvas,
            self.boxes,
            self.picks,
            self.pick_anims,
            self.views,
            self.anims,
        );
    }

    /// Row `y`'s picked text runs over its boxes; with `cols`, only the surface columns
    /// `[c0, c1)` are written.
    pub(crate) fn paint_row_text(
        &self,
        row: &mut [u8],
        y: i32,
        shift: i32,
        cols: Option<(i32, i32)>,
    ) {
        let (anims, views) = (self.anims, self.views);
        // The focused field's selection highlight (TASK-0067B); its caret is the compositor's
        // layer (RFC-0113 §5, `interaction/text_caret.rs`), never painted here.
        let caret = self.caret;
        let row_px = (row.len() / 4) as i32;
        for &(bi, ti) in self.runs {
            let b = &self.boxes[bi as usize];
            let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
            if bw <= 0 || bh <= 0 {
                continue;
            }
            // Where this run is sampled and shifted: a scroll view's content at its view, inside
            // its viewport; the rest at the row itself.
            let view = b.scroll_root.and_then(|r| views.iter().find(|v| v.root == r));
            let (y_eff, dx, mut left, mut right) = match view {
                Some(v) => {
                    if y < v.clip.1 || y >= v.clip.3 {
                        continue;
                    }
                    (y + v.dy, shift + v.dx, v.clip.0, v.clip.2)
                }
                None => (y, shift, 0, row_px),
            };
            if let Some((c0, c1)) = cols {
                left = left.max(c0);
                right = right.min(c1);
            }
            // The rows its text shadows reach (the halo's blur) are its rows too.
            let (above, below) = b.visual.text_shadows.iter().flatten().fold((0, 0), |r, s| {
                let reach = nexus_text_baked::blur_reach(u32::from(s.blur_radius)) as i32;
                let oy = i32::from(s.offset_y);
                (r.0.max(reach - oy), r.1.max(reach + oy))
            });
            if y_eff < by - above || y_eff >= by + bh + below {
                continue;
            }
            // The box's own clip (its content's clips), in its space.
            if let Some(c) = b.clip_rect {
                if y_eff < c.y.0 || y_eff >= c.y.0 + c.height.0 {
                    continue;
                }
                left = left.max(c.x.0 - dx);
                right = right.min(c.x.0 + c.width.0 - dx);
            }
            // The run paints into the row from `left` on — cut there as at `right`.
            let (left, right) = (left.clamp(0, row_px), right.min(row_px));
            if right <= left {
                continue;
            }
            let row = &mut row[left as usize * 4..];
            let (bx, right) = (bx - left, (right - left) as u32);
            let t = &self.texts[ti as usize];
            let (content, color, tracking) = (&t.content, &t.color, t.tracking);
            // `.textFit`: the LAYOUT chose this run's size from its container's box, so it wins
            // over the style's — re-resolved with the REQUESTED weight, its line box scaled alike.
            let (font, line) = match b.text_px {
                Some(px) => (
                    nexus_text_baked::FontSize::nearest(
                        px.0,
                        nexus_text_baked::measure_text::BakedTextMeasure::weight(t.weight),
                    ),
                    nexus_text_baked::line_height(nexus_text_baked::FontSize::nearest(
                        px.0,
                        nexus_text_baked::measure_text::BakedTextMeasure::weight(t.weight),
                    )) as i32,
                ),
                None => (t.font, t.line),
            };
            let font = &font;
            // RFC-0116: the face's own band sits in the style's line box with half-leading
            // (`body` is a 17 px band in a 20 px line).
            let by = by + (line - nexus_text_baked::line_height(*font) as i32) / 2;
            // Animated text node: fade its glyphs by the node opacity and shift them by the
            // horizontal translate (vertical translate + scale on TEXT are the fill path's).
            let (mut color, bx_glyph) = match nexus_scene_raster::covering_index(anims, b.node_id)
                .map(|i| &anims[i])
                .filter(|a| !a.is_identity())
            {
                Some(a) => {
                    let mut c = *color;
                    c[3] = (c[3] as u32 * a.opacity as u32 / 255) as u8;
                    (c, bx - dx - a.dx)
                }
                None => (*color, bx - dx),
            };
            // A faded box (its own and its ancestors' opacity — a disabled control) fades its text.
            color[3] = (u32::from(color[3]) * u32::from(b.opacity) / 255) as u8;
            // The focused field shows its WHOLE text, scrolled so its caret stays in view
            // (TASK-0346 P2) and clipped to its box; any other run wider than its box is CUT —
            // marked, so a truncated label reads as truncated instead of as a shorter label.
            let focused = caret.filter(|c| c.nid == b.node_id);
            let (row, bx_glyph, right) = match focused {
                Some(c) => {
                    let box_l = (bx - dx).max(0) as usize;
                    if box_l * 4 >= row.len() {
                        continue;
                    }
                    let box_r = ((bx - dx + bw).max(0) as u32).min(right);
                    let shifted = bx_glyph - c.scroll - box_l as i32;
                    (&mut row[box_l * 4..], shifted, box_r.saturating_sub(box_l as u32))
                }
                None => (row, bx_glyph, right),
            };
            let (cut, ell) = match focused {
                Some(_) => (content.chars().count(), false),
                None => nexus_text_baked::ellipsis_cut(content, *font, tracking, bw.max(0) as u32),
            };
            let run = || {
                content
                    .chars()
                    .take(cut)
                    .chain(core::iter::once(nexus_text_baked::ELLIPSIS).take(ell as usize))
            };
            // `.textAlign`: a run narrower than its box sits at the box's centre or right edge (the
            // focused field keeps its scroll origin).
            let bx_glyph = match (focused, t.align) {
                (
                    None,
                    nexus_layout_types::TextAlign::Center | nexus_layout_types::TextAlign::Right,
                ) => {
                    let w = nexus_text_baked::measure(run(), *font, tracking) as i32;
                    let slack = (bw - w).max(0);
                    let right = t.align == nexus_layout_types::TextAlign::Right;
                    bx_glyph + if right { slack } else { slack / 2 }
                }
                _ => bx_glyph,
            };
            // TASK-0067B: the selection BEHIND the run.
            if let Some(c) = focused {
                paint_selection_row(row, y_eff, by, bh, bx_glyph, c.selection, right, c.highlight);
            }
            // The text shadows (the label halo, `.shadow(halo)` in `labelShadow`), back to front,
            // blurred as the design draws them and faded with the text.
            for sh in b.visual.text_shadows.iter().rev().flatten().filter(|s| s.color.a > 0) {
                let sc = sh.color;
                nexus_text_baked::draw_text_blur_row(
                    row,
                    y_eff as u32,
                    by + i32::from(sh.offset_y),
                    bx_glyph + i32::from(sh.offset_x),
                    right,
                    run(),
                    *font,
                    tracking,
                    [sc.b, sc.g, sc.r, (u32::from(sc.a) * u32::from(color[3]) / 255) as u8],
                    u32::from(sh.blur_radius),
                );
            }
            nexus_text_baked::draw_text_row(
                row,
                y_eff as u32,
                by,
                bx_glyph,
                right,
                run(),
                *font,
                tracking,
                color,
            );
        }
    }
}

/// The picking rules of a pass planned over columns, and the reaches past a box's sides.
mod cols;
pub(crate) use cols::{meets_cols, run_meets_cols, run_reach, stroke_reach, GLYPH_OVERHANG};

/// Blends the selection highlight over `[bx + x0, bx + x1)` of one row inside the field's
/// box — drawn BEFORE the glyphs so the text stays on top.
#[allow(clippy::too_many_arguments)]
fn paint_selection_row(
    row: &mut [u8],
    y: i32,
    by: i32,
    bh: i32,
    bx: i32,
    selection: Option<(i32, i32)>,
    right: u32,
    color: [u8; 4],
) {
    let Some((x0, x1)) = selection else { return };
    if y < by + 2 || y >= by + bh - 2 || x1 <= x0 {
        return;
    }
    let a = u32::from(color[3]);
    for px in (bx + x0).max(0)..(bx + x1 + 1).max(0) {
        let o = px as usize * 4;
        if px as u32 >= right || o + 4 > row.len() {
            break;
        }
        for c in 0..3 {
            row[o + c] =
                ((u32::from(row[o + c]) * (255 - a) + u32::from(color[c]) * a) / 255) as u8;
        }
    }
}

/// TASK-0354 P8: a diff's rectangles, painted by the column pass, are the full frame.
#[cfg(test)]
#[path = "pass_plan_diff_tests.rs"]
mod diff_tests;
#[cfg(test)]
#[path = "pass_plan_tests.rs"]
mod tests;
