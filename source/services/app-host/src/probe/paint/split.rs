// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the staged pass (TASK-0367 P0) — the process's first full surface painted exactly
//! as `render_rows` paints it, with a clock read between a row's stages: the ground fill, the
//! boxes, the text runs, the straight-alpha conversion. It prints `apphost: raster split (…)`
//! once: where a row's time goes on this machine, with the cost of the clock reads stated and
//! taken off (`raster_split.rs` holds the accounting). Every later pass runs unstaged.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `raster_split.rs` (host); QEMU and board: the line itself.

use super::super::surface_mem::SurfaceMem;
use super::super::*;
use super::Pass;
use crate::raster_split::{RasterSplit, Stage};
use crate::straight_alpha::unpremultiply;

/// Back-to-back clock reads the cost of one is measured over.
const CLOCK_READS: u32 = 64;

impl DslApp {
    /// The whole base, staged (see the module header). The pixels are `render_rows`'s.
    pub(super) fn render_rows_split(&mut self, mem: &mut SurfaceMem) -> bool {
        self.split_done = true;
        let base = self.page_base();
        let h = self.h as i32;
        let row_bytes = self.w as usize * 4;
        let mut split = RasterSplit::default();
        let t0 = nsec_now();
        let mut views = core::mem::take(&mut self.view_scratch);
        self.base_views(&mut views);
        self.prepare_picks_with(Pass::Base, (0, h), None, &views);
        let mut at = nsec_now();
        split.pick_ns = at.saturating_sub(t0);
        split.boxes = self.vis_pick.len() as u32;
        split.runs = self.vis_text.len() as u32;
        let mut ok = true;
        // A stage ends with a clock read, which starts the next: one read per row and stage.
        let mut lap = |split: &mut RasterSplit, stage: Stage| {
            let now = nsec_now();
            split.add(stage, now.saturating_sub(at));
            at = now;
        };
        for y in 0..h {
            let Some(row) = mem.row_mut(y as u32).and_then(|r| r.get_mut(..row_bytes)) else {
                ok = false;
                break;
            };
            nexus_paint_kernels::fill(row, base);
            lap(&mut split, Stage::Ground);
            self.paint_row_boxes(row, y, 0, &views);
            lap(&mut split, Stage::Boxes);
            self.paint_row_text(row, y, 0, &views);
            lap(&mut split, Stage::Text);
            unpremultiply(row);
            lap(&mut split, Stage::Alpha);
            split.rows += 1;
        }
        self.view_scratch = views;
        split.px = u64::from(split.rows) * u64::from(self.w);
        let done = nsec_now();
        self.motion_hops.painted(done.saturating_sub(t0) / 1000, split.rows);
        // What a clock read costs here, measured after the pass (its caches are the pass's).
        let c0 = nsec_now();
        for _ in 0..CLOCK_READS {
            let _ = nsec_now();
        }
        let clock_ns = RasterSplit::clock_ns(nsec_now().saturating_sub(c0), CLOCK_READS + 1);
        raw_marker(&alloc::format!("apphost: raster split ({})", split.line(clock_ns)));
        ok
    }
}
