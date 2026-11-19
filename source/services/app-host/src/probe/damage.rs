// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's pending damage (TASK-0354 P8): ONE set of rectangles in layout pixels
//! (`DslApp::pending`, `band_math::Pending`) that whoever changes a pixel adds to, and the
//! redraw that paints it. The layout diff names what a relayout changed (`layout_diff`); here is
//! what the diff cannot see — a focused field's selection, a scroll layer an overlay came over
//! or left, a dispatch that laid nothing out — and the redraw itself: per rectangle the base's
//! columns of its rows, per run of rows the motion layers' store rows and the bands' content
//! rows. Everything (`Change::Whole`) is for doubt alone: mount, a resize, a theme, a dispatch
//! that named nothing.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the set and its bound in `band_math/tests.rs`; that the rectangles a diff
//!   names repaint to the full frame in `pass_plan_diff_tests.rs` (host); the QEMU lanes
//!   (`apphost: tap (… base=rects(`, `apphost: hops motion … px=`).
//! INVARIANTS: only the event loop takes `pending`; everything is never narrowed; a bound costs
//!   damage, never a pixel left standing

use super::scroll::MAX_SCROLL_LAYERS;
use super::surface_mem::SurfaceMem;
use super::*;
use crate::band_math::{Change, PresentDamage};
use crate::pass_plan::GLYPH_OVERHANG;

/// Edges `(x0, y0, x1, y1)`.
type Edges = (i32, i32, i32, i32);

/// Where the scroll layers' content shows: each covered layer's viewport and offset (its
/// content paints in the base there), each flattened container's viewport — and every layer's
/// root and viewport, which its band's rows are counted from.
pub(super) struct BaseShows {
    covered: [Option<(Edges, i32, i32)>; MAX_SCROLL_LAYERS],
    flat: alloc::vec::Vec<Edges>,
    layers: [Option<(usize, Edges)>; MAX_SCROLL_LAYERS],
}

/// Everything, for an app that is there (a presentation change, a preview, a pressure level).
pub(super) fn whole(app: &mut Option<DslApp>) {
    if let Some(dsl) = app.as_mut() {
        dsl.pending.whole();
    }
}

impl DslApp {
    /// Runs a dispatch that says whether the model changed. A change that ran no bounded relayout
    /// (`relayout_bounded`) named no rectangle — a paint-only change, a caret — and is everything.
    pub(super) fn dispatched(&mut self, run: impl FnOnce(&mut Self) -> bool) -> bool {
        let diffs = self.diffs;
        let changed = run(self);
        if changed && self.diffs == diffs {
            self.pending.whole();
        }
        changed
    }

    /// Rectangle `r` of the layout, pending.
    pub(super) fn damage_rect(&mut self, r: Edges) {
        self.pending.add(r, (self.w, self.layout_bottom().max(0) as u32));
    }

    /// A motion layer's subtree left the base's pass or came back to it (`MotionLayer::rest`):
    /// its rest paints again there, and so do the runs its glass roots hide while they are the
    /// base's (`layout_diff::hidden_runs`) — in their whole box.
    pub(super) fn damage_layer(&mut self, (rest, nodes): (Edges, (usize, usize))) {
        let (size, o) = ((self.w, self.layout_bottom().max(0) as u32), GLYPH_OVERHANG);
        let mut named = crate::band_math::Pending::default();
        named.add((rest.0 - o, rest.1 - o, rest.2 + o, rest.3 + o), size);
        let hidden = &mut |r: Edges| named.add(r, size);
        crate::layout_diff::hidden_runs(&self.layout.boxes, &self.texts, nodes, hidden);
        self.pending.merge(named);
    }

    /// The focused field's rows: its selection, its text's offset and its uncut run paint in
    /// its run's box (`pass_plan`), and none of them is the layout's.
    pub(super) fn damage_field(&mut self) {
        if let Some(f) = self.view.text_focus() {
            self.damage_field_of(f.box_id);
        }
    }

    /// The rows of the text field under handler box `box_id` (a field without a run box paints
    /// nothing focused).
    pub(super) fn damage_field_of(&mut self, box_id: usize) {
        let mut idx = 0usize;
        let run = super::paint::caret_input(self.view.scene(), &mut idx, box_id, false)
            .and_then(|(nid, ..)| self.layout.boxes.iter().find(|b| b.node_id == nid));
        let Some(b) = run else { return };
        let (_, above, _, below) = nexus_scene_raster::shadow_reach(b);
        let (y0, y1) = (b.rect.y.0 - above, b.rect.y.0 + b.rect.height.0 + below);
        let bottom = self.layout_bottom();
        self.pending
            .rows(((y0 - GLYPH_OVERHANG).max(0), (y1 + GLYPH_OVERHANG).min(bottom)), self.w);
    }

    /// Where the scroll layers' content shows now — taken before a relayout or a scroll step.
    pub(super) fn base_shows(&self) -> BaseShows {
        let (mut covered, mut layers) = ([None; MAX_SCROLL_LAYERS], [None; MAX_SCROLL_LAYERS]);
        for (slot, l) in layers.iter_mut().zip(self.scroll_layers.iter().flatten()) {
            *slot = Some((l.root, l.viewport));
        }
        let under = self.scroll_layers.iter().flatten().filter(|l| l.covered);
        for (slot, l) in covered.iter_mut().zip(under) {
            let v = l.paint_view();
            *slot = Some((v.clip, v.dx, v.dy));
        }
        BaseShows { covered, flat: self.flat_views.iter().map(|v| v.clip).collect(), layers }
    }

    /// The scroll layers show something else than `before`. In the base: a layer an overlay came
    /// over paints its content there now, one it left paints it there no more, a covered one
    /// moved — each is its viewport. In its band, drawn anew from its visible window out: a
    /// layer an overlay left (what changed under it was painted in the base, never in the band),
    /// one whose viewport moved (its rows are counted from it), and every one from root
    /// `shifted` on (`layout_diff::Changed::shifted`: a layer keeps its band by its root's
    /// number, and from there on the numbers moved — the band may be another container's).
    pub(super) fn damage_views(&mut self, before: &BaseShows, shifted: Option<usize>) {
        let now = self.base_shows();
        for (a, b) in [(&before.covered, &now.covered), (&now.covered, &before.covered)] {
            for v in a.iter().flatten().filter(|v| !b.contains(&Some(**v))) {
                self.damage_rect(v.0);
            }
        }
        for l in self.scroll_layers.iter_mut().flatten().filter(|l| !l.covered) {
            let kept = before.layers.contains(&Some((l.root, l.viewport)))
                && !before.covered.iter().flatten().any(|v| v.0 == l.viewport)
                && !shifted.is_some_and(|s| l.root >= s);
            if !kept && l.band.valid_len > 0 {
                (l.band.valid_start, l.band.valid_len) = (l.offset.max(0) as u32, 0);
                self.tree_dirty = true;
            }
        }
        // A container that fell out of the layers' bound, or came back into it.
        if before.flat != now.flat {
            self.pending.rows((0, self.layout_bottom()), self.w);
        }
    }

    /// Redraws what a model change named. Everything: the base, the patch, every band from its
    /// visible window out, every motion layer. Rectangles (layout pixels): the base's columns of
    /// their rows — and, once per run of rows, a covered layer's content where its view shows
    /// it, the motion layers' store rows and each band's content rows there. What is drawn is
    /// added to `damage`; `false` when the surface refused a write.
    pub(super) fn redraw(
        &mut self,
        mem: &mut SurfaceMem,
        change: Change,
        damage: &mut PresentDamage,
    ) -> bool {
        let (w, h) = (self.w, self.h as i32);
        let rects = match change {
            Change::Rects(rects) => rects,
            Change::Whole => {
                // Every motion layer paints anew: a ghost has no subtree to paint.
                self.cut_ghosts("whole");
                if !self.render(mem) || !self.draw_patch(mem, damage) {
                    return false;
                }
                damage.add((0, 0, w, h as u32));
                self.invalidate_bands();
                self.fill_bands(mem, None, damage);
                return self.paint_motion_layers(mem, damage, true);
            }
        };
        // TASK-0351 P15 / TASK-0367 P3: the damage names what changed, and only those columns
        // of those rows are painted. A rectangle below the surface is a band's content alone.
        let (t0, mut px, mut rows) = (nsec_now(), 0u64, 0u32);
        for &(x, y, rw, rh) in rects.rects() {
            let (y0, y1) = (y as i32, ((y + rh) as i32).min(h));
            if y1 <= y0 {
                continue;
            }
            if !self.render_rows(mem, (y0, y1), Some((x as i32, (x + rw) as i32))) {
                return false;
            }
            damage.add((x, y, rw, (y1 - y0) as u32));
            px += u64::from(rw) * (y1 - y0) as u64;
            rows += (y1 - y0) as u32;
        }
        if rows > 0 {
            // A motion episode's raster cost and its damage, once a present.
            self.motion_hops.painted(nsec_now().saturating_sub(t0) / 1000, rows);
            self.motion_hops.damaged(px);
        }
        let (runs, n) = rects.row_intervals();
        for &span in &runs[..n] {
            if !self.redraw_moved(mem, span, damage)
                || !self.redraw_motion_rows(mem, span, damage)
                || !self.redraw_band_rows(mem, span, damage)
            {
                return false;
            }
        }
        true
    }

    /// Layout rows `[y0, y1)` of each covered layer that is scrolled: its content paints in the
    /// base moved by its view, so the surface rows it shows them at are painted again too —
    /// inside its viewport.
    fn redraw_moved(
        &mut self,
        mem: &mut SurfaceMem,
        (y0, y1): (i32, i32),
        damage: &mut PresentDamage,
    ) -> bool {
        for k in 0..MAX_SCROLL_LAYERS {
            let Some(l) = self.scroll_layers[k].filter(|l| l.covered) else { continue };
            let v = l.paint_view();
            let (x0, x1) = (v.clip.0.max(0), v.clip.2);
            let (r0, r1) =
                ((y0 - v.dy).max(v.clip.1).max(0), (y1 - v.dy).min(v.clip.3).min(self.h as i32));
            if v.dy == 0 || r1 <= r0 || x1 <= x0 {
                continue;
            }
            if !self.render_rows(mem, (r0, r1), Some((x0, x1))) {
                return false;
            }
            damage.add((x0 as u32, r0 as u32, (x1 - x0) as u32, (r1 - r0) as u32));
        }
        true
    }
}
