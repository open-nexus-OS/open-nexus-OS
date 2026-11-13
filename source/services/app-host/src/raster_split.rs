// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where a painted row's time goes (TASK-0367 P0) — the accounting of one staged pass:
//! the pick (once per pass) and, per row, the ground fill, the boxes (fills, blends, corners,
//! shadows), the text runs and the straight-alpha conversion. A stage is timed by a clock read on
//! each side of it, so every stage's sum holds one clock read per row; the line states the
//! measured cost of a read (`clock_ns`) and the stages with it taken off — numbers, no verdict.
//! Same one-cfg shape as `motion_math`: `probe/paint/split.rs` runs the staged pass once per
//! process, on its first full surface.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use core::fmt;

/// The stages of a row, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The row's ground: the page base, one fill.
    Ground = 0,
    /// The picked boxes: fills, blends, corners, gradients, shadows, shapes.
    Boxes = 1,
    /// The picked text runs: glyphs, their shadows, the selection.
    Text = 2,
    /// The row leaves in straight alpha.
    Alpha = 3,
}

/// One staged pass's sums.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RasterSplit {
    /// Rows painted and the pixels in them.
    pub(crate) rows: u32,
    pub(crate) px: u64,
    /// The pick, once for the pass (ns).
    pub(crate) pick_ns: u64,
    /// Each stage over every row (ns, one clock read per row included).
    pub(crate) stage_ns: [u64; 4],
    /// What the pass picked: boxes and text runs.
    pub(crate) boxes: u32,
    pub(crate) runs: u32,
}

impl RasterSplit {
    /// One row's stage took `ns` between its two clock reads.
    pub(crate) fn add(&mut self, stage: Stage, ns: u64) {
        let slot = &mut self.stage_ns[stage as usize];
        *slot = slot.saturating_add(ns);
    }

    /// A stage's sum in µs with the clock reads taken off (`clock_ns` a read, one per row).
    pub(crate) fn stage_us(&self, stage: Stage, clock_ns: u64) -> u64 {
        let reads = u64::from(self.rows).saturating_mul(clock_ns);
        self.stage_ns[stage as usize].saturating_sub(reads) / 1000
    }

    /// The cost of one clock read (ns) from `reads` back-to-back reads that took `total_ns`.
    pub(crate) fn clock_ns(total_ns: u64, reads: u32) -> u64 {
        total_ns / u64::from(reads.max(1))
    }

    /// The line's body for a clock read of `clock_ns`: `rows= px= boxes= runs= pick_us=
    /// ground_us= boxes_us= text_us= alpha_us= clock_ns=`.
    pub(crate) fn line(&self, clock_ns: u64) -> Line<'_> {
        Line { split: self, clock_ns }
    }
}

/// A pass's sums as its line's body (see [`RasterSplit::line`]).
pub(crate) struct Line<'a> {
    split: &'a RasterSplit,
    clock_ns: u64,
}

impl fmt::Display for Line<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (s, c) = (self.split, self.clock_ns);
        write!(
            f,
            "rows={} px={} boxes={} runs={} pick_us={} ground_us={} boxes_us={} text_us={} \
             alpha_us={} clock_ns={c}",
            s.rows,
            s.px,
            s.boxes,
            s.runs,
            s.pick_ns / 1000,
            s.stage_us(Stage::Ground, c),
            s.stage_us(Stage::Boxes, c),
            s.stage_us(Stage::Text, c),
            s.stage_us(Stage::Alpha, c),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pass of 1 000 rows whose stages each hold one 500-ns clock read per row: the line shows
    /// the stages without the reads.
    #[test]
    fn a_stage_is_stated_without_its_clock_reads() {
        let mut s =
            RasterSplit { rows: 1_000, px: 1_920_000, boxes: 40, runs: 12, ..Default::default() };
        s.pick_ns = 250_000;
        for _ in 0..1_000 {
            s.add(Stage::Ground, 900);
            s.add(Stage::Boxes, 120_500);
            s.add(Stage::Text, 30_500);
            s.add(Stage::Alpha, 8_500);
        }
        assert_eq!(s.stage_us(Stage::Ground, 500), 400);
        assert_eq!(s.stage_us(Stage::Boxes, 500), 120_000);
        assert_eq!(
            s.line(500).to_string(),
            "rows=1000 px=1920000 boxes=40 runs=12 pick_us=250 ground_us=400 boxes_us=120000 \
             text_us=30000 alpha_us=8000 clock_ns=500"
        );
    }

    /// A stage cheaper than the clock reads it holds is stated as nothing, never as a negative
    /// or a wrapped number; a clock that never ticked between reads costs nothing.
    #[test]
    fn a_stage_below_the_clock_is_zero() {
        let mut s = RasterSplit { rows: 10, ..Default::default() };
        s.add(Stage::Ground, 3_000);
        assert_eq!(s.stage_us(Stage::Ground, 500), 0);
        s.add(Stage::Alpha, u64::MAX);
        s.add(Stage::Alpha, 5);
        assert_eq!(s.stage_ns[Stage::Alpha as usize], u64::MAX, "a sum saturates");
        assert_eq!(RasterSplit::clock_ns(0, 64), 0);
        assert_eq!(RasterSplit::clock_ns(32_000, 64), 500);
        assert_eq!(RasterSplit::clock_ns(32_000, 0), 32_000);
    }
}
