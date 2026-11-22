// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a motion layer's store REUSED (RFC-0109 §motion layers, package B 1a): the probe's
//! side of `crate::motion_reuse`. Where a layer's store used to be thrown away — a ghost revived,
//! a re-resolve that renumbered its nodes, a new place — `reuse_store` takes what its store would
//! hold if painted now and compares it with the slot's snapshot of what it holds: keep, paint the
//! changed rows (the P5 path, `motion_rows`), or paint whole. After a paint each layer whose rows
//! were written is snapshotted again (`hold_painted`).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the decisions in `crate::motion_reuse` (host oracle); the lanes'
//!   `apphost: motion layer (… reused=`

use super::*;
use crate::motion_reuse::{Inputs, View};

impl crate::probe::DslApp {
    /// What the painter reads for motion layer `m`'s subtree that its layout does not hold: the
    /// theme, its backdrop, a node covered by an animation off identity, the focus inside, and
    /// how each scroll root its boxes name is painted.
    fn reuse_inputs(&self, m: &MotionLayer) -> Inputs {
        let mut inputs = Inputs::new(self.theme_mode, m.backdrop.map(|c| [c.r, c.g, c.b, c.a]));
        let mut anims =
            [nexus_scene_raster::NodeAnim::identity(0); crate::probe::anim::MAX_EXPANDED_ANIMS];
        let n = self.expand_node_anims(&mut anims);
        let anims = &anims[..n];
        // Only what the layer's own pass paints into the store: a band's content is its band's.
        for b in self.layout.boxes.iter().filter(|b| m.holds(b.node_id) && !self.in_band(b)) {
            let cover = nexus_scene_raster::covering_index(anims, b.node_id);
            inputs.animated |= cover.is_some_and(|i| !anims[i].is_identity());
            if let Some(r) = b.scroll_root {
                inputs.view(r as isize - m.root as isize, self.painted_view(r));
            }
        }
        inputs.focus = self.view.text_focus().is_some_and(|f| {
            m.holds(f.box_id)
                && self
                    .layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == f.box_id)
                    .is_none_or(|b| !self.in_band(b))
        });
        inputs
    }

    /// How scroll root `root`'s content is painted: in its band, at a view, or plain.
    fn painted_view(&self, root: usize) -> View {
        let shown =
            |v: nexus_scene_raster::ScrollView| View::Shown { clip: v.clip, dx: v.dx, dy: v.dy };
        match self.scroll_layers.iter().flatten().find(|l| l.root == root) {
            Some(l) if l.covered => shown(l.paint_view()),
            Some(_) => View::Band,
            None => {
                self.flat_views.iter().find(|v| v.root == root).map_or(View::Plain, |v| shown(*v))
            }
        }
    }

    /// What placed motion layer `k`'s store would hold, painted now, into the cache's scratch.
    /// `false`: not a placed, live layer.
    fn take_now(&mut self, k: usize) -> bool {
        let Some(m) = self.motion_layers[k].filter(|m| m.leaving.is_none()) else { return false };
        let Some(place) = m.store else { return false };
        let inputs = self.reuse_inputs(&m);
        let lists = (self.layout.boxes.as_slice(), self.texts.as_slice());
        // `in_band` over the scroll layers alone (the cache is borrowed beside them).
        let layers = &self.scroll_layers;
        let painted = |b: &nexus_layout::LayoutBox| {
            !b.scroll_root
                .is_some_and(|r| layers.iter().flatten().any(|l| l.root == r && !l.covered))
        };
        let at = (m.rect, place);
        self.motion_cache.now_mut().take_painted(lists, (m.root, m.end), at, inputs, &painted);
        true
    }

    /// The one decision on motion layer `k`'s store (a revive, a re-resolve, a new place): kept,
    /// its changed rows painted again (`motion_rows`), or painted whole. A layer without a place
    /// holds nothing.
    pub(super) fn reuse_store(&mut self, k: usize) {
        let taken = self.take_now(k);
        let why = if taken { self.motion_cache.why_whole(k) } else { "unplaced" };
        let r = if taken { self.motion_cache.decide(k) } else { Reuse::Whole };
        if r == Reuse::Whole {
            raw_marker(&alloc::format!("apphost: reuse whole (layer={k} why={why})"));
        }
        if r == Reuse::Whole {
            self.motion_cache.forget(k);
        }
        if let Some(m) = self.motion_layers[k].as_mut().filter(|m| m.leaving.is_none()) {
            (m.painted, m.reused) = (r != Reuse::Whole, r);
        }
        if let Reuse::Rows(rows) = r {
            self.motion_rows = union(self.motion_rows, rows);
        }
    }

    /// After a paint: each placed, live, painted layer whose rows were written (its snapshot
    /// forgotten by `redraw_motion`) holds its subtree as it is now — snapshotted.
    pub(super) fn hold_painted(&mut self) {
        for k in 0..MAX_MOTION_LAYERS {
            let painted = self.motion_layers[k].is_some_and(|m| m.painted);
            if painted && self.motion_cache.wants(k) && self.take_now(k) {
                self.motion_cache.hold_now(k);
            }
        }
    }
}
