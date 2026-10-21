// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The textured work of a frame window, by pass kind (TASK-0335 P0, RFC-0108): the
//! board's GPU is memory-bound at a few nanoseconds per textured pixel — a full-screen pass
//! took 12 ms in the boot trace, the raw fill 1 ns a pixel — so a frame's cost follows the
//! pixels its passes draw, not their count. `gpud: gpu work (…)` beside `gpu frame us` names
//! the slot's pixels, the glass scenes' and the blurs', and the GPU's time per pixel; every cut
//! of the frame-cost package is judged by it. Split out of `desktop.rs` (structure ratchet).
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `Board::work_px` on the host (`tests/frame_board.rs`), the write causes in
//!   `tests/frame_motion.rs`; the lines on the board

use nexus_gfx::frame::{Target, Writes};

use super::emit_fmt;

/// Textured pixels by pass kind: the slot's, the glass scenes' (scratch), the blurs' (half and
/// cache rows); and the glass writes by cause (TASK-0336 P0: band, backdrop, moved, new, full).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Work([u64; 3], [u64; 5]);

impl Work {
    /// A pass into `target` drew `px` textured pixels.
    pub(super) fn note(&mut self, target: Target, px: u64) {
        let k = match target {
            Target::Slot => 0,
            Target::Scratch => 1,
            Target::Half | Target::Cache => 2,
        };
        self.0[k] += px;
    }

    /// A frame's glass writes, by cause.
    pub(super) fn note_writes(&mut self, w: Writes) {
        for (acc, n) in self.1.iter_mut().zip([w.band, w.backdrop, w.moved, w.new, w.full]) {
            *acc += u64::from(n);
        }
    }

    /// Folds a frame's work into the window's.
    pub(super) fn add(&mut self, frame: Work) {
        for (acc, w) in self.0.iter_mut().zip(frame.0) {
            *acc += w;
        }
        for (acc, w) in self.1.iter_mut().zip(frame.1) {
            *acc += w;
        }
    }

    /// The window's line: pixels per frame by kind, and the GPU's wait per pixel.
    pub(super) fn emit(&self, frames: u64, wait_ns: u64) {
        let total = self.0.iter().sum::<u64>().max(1);
        emit_fmt(format_args!(
            "gpud: gpu work (slot_px={} scene_px={} blur_px={} ns_per_px={})",
            self.0[0] / frames,
            self.0[1] / frames,
            self.0[2] / frames,
            wait_ns / total
        ));
        // TASK-0336 P0: why the glasses were blurred again — the cause each cut aims at.
        let w = self.1;
        emit_fmt(format_args!(
            "gpud: glass writes (band={} backdrop={} moved={} new={} full={})",
            w[0], w[1], w[2], w[3], w[4]
        ));
    }
}
