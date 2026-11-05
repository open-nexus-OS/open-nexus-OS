// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The textured work of a frame window, by pass kind (TASK-0335 P0, RFC-0108): the
//! board's GPU is memory-bound at a few nanoseconds per textured pixel — a full-screen pass
//! took 12 ms in the boot trace, the raw fill 1 ns a pixel — so a frame's cost follows the
//! pixels its passes draw, not their count. `gpud: gpu work (…)` beside `gpu frame us` names
//! the slot's pixels, the glass scenes' and the blurs', the pixels by program (TASK-0335 P4: the
//! rounded and the shadow programs cost 6–8 times the opaque one) and the GPU's time per pixel;
//! every cut of the frame-cost package is judged by it. Split out of `desktop.rs` (structure
//! ratchet).
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `Board::work_px` on the host (`tests/frame_board.rs`), the write causes in
//!   `tests/frame_motion.rs`; the lines on the board

use nexus_gfx::frame::{Target, Writes, PROGRAM_KINDS};

use super::contexts::Holdings;
use super::emit_fmt;
use super::series::LAST;

/// Textured pixels by pass kind: the slot's, the glass scenes' (scratch), the blurs' (half and
/// cache rows); the glass writes by cause (TASK-0336 P0: band, backdrop, moved, new, full); and
/// the pixels by program (TASK-0335 P4: opaque, straight, rounded, shadow, blur); and the layers
/// the frames composed (TASK-0351 P0).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Work([u64; 3], [u64; 5], [u64; PROGRAM_KINDS], u64, [u64; 8]);

impl Work {
    /// The slot pass's blended pixels by layer kind (`Board::blended_by_kind`, TASK-0351 P14);
    /// a pass into any other target counts nothing and `px` is never asked.
    pub(super) fn note_kinds(&mut self, target: Target, px: impl FnOnce() -> [u64; 8]) {
        if target != Target::Slot {
            return;
        }
        for (acc, n) in self.4.iter_mut().zip(px()) {
            *acc += n;
        }
    }

    /// A pass into `target` drew `px` textured pixels by program (`Board::work_by_program`).
    pub(super) fn note(&mut self, target: Target, px: [u64; PROGRAM_KINDS]) {
        let k = match target {
            Target::Slot => 0,
            Target::Scratch => 1,
            Target::Half | Target::Cache => 2,
        };
        self.0[k] += px.iter().sum::<u64>();
        for (acc, n) in self.2.iter_mut().zip(px) {
            *acc += n;
        }
    }

    /// A frame's plan: its glass writes by cause, and the layers it composes.
    pub(super) fn note_plan(&mut self, w: Writes, layers: usize) {
        for (acc, n) in self.1.iter_mut().zip([w.band, w.backdrop, w.moved, w.new, w.full]) {
            *acc += u64::from(n);
        }
        self.3 += layers as u64;
    }

    /// Folds a frame's work into the window's.
    pub(super) fn add(&mut self, frame: Work) {
        for (acc, w) in self.0.iter_mut().zip(frame.0) {
            *acc += w;
        }
        for (acc, w) in self.1.iter_mut().zip(frame.1) {
            *acc += w;
        }
        for (acc, w) in self.2.iter_mut().zip(frame.2) {
            *acc += w;
        }
        self.3 += frame.3;
        for (acc, w) in self.4.iter_mut().zip(frame.4) {
            *acc += w;
        }
    }

    /// The window's lines: pixels per frame by kind and by program, the GPU's wait per pixel and
    /// the layers per frame; then the bindings the live contexts hold (TASK-0351 P0: the board's
    /// context binds every app surface — `of` is the room their tables have before growing, P2).
    pub(super) fn emit(&self, frames: u64, wait_ns: u64, held: Holdings) {
        let total = self.0.iter().sum::<u64>().max(1);
        let p = self.2;
        emit_fmt(format_args!(
            "gpud: gpu work (slot_px={} scene_px={} blur_px={} ns_per_px={} opaque_px={} straight_px={} rounded_px={} shadow_px={} layers={} last={})",
            self.0[0] / frames,
            self.0[1] / frames,
            self.0[2] / frames,
            wait_ns / total,
            p[0] / frames,
            p[1] / frames,
            p[2] / frames,
            p[3] / frames,
            self.3 / frames,
            LAST.map_or(0, |l| l.ns_per_px)
        ));
        // TASK-0351 P14: which layers the slot pass blends — the pixels the cut aims at.
        let k = self.4;
        emit_fmt(format_args!(
            "gpud: gpu blend kinds (keyless={} body={} region={} slice={} wash={} solid={} bar={} caret={})",
            k[0] / frames,
            k[1] / frames,
            k[2] / frames,
            k[3] / frames,
            k[4] / frames,
            k[5] / frames,
            k[6] / frames,
            k[7] / frames
        ));
        emit_fmt(format_args!(
            "gpud: bindings (in_use={} of={} contexts={})",
            held.bindings, held.binding_room, held.live
        ));
        // TASK-0336 P0: why the glasses were blurred again — the cause each cut aims at.
        let w = self.1;
        emit_fmt(format_args!(
            "gpud: glass writes (band={} backdrop={} moved={} new={} full={})",
            w[0], w[1], w[2], w[3], w[4]
        ));
    }
}

/// RFC-0108: the window judged — frames past the budget are dropped frames; `last=` the slowest
/// frame of the last recorded board cycle (TASK-0351 P8).
pub(super) fn emit_ux(ux: &nexus_ux_budgets::FrameWindow) {
    emit_fmt(format_args!(
        "gpud: ux frame {} (dropped={} of={} max_us={} budget_us={} last={})",
        ux.verdict().as_str(),
        ux.dropped,
        ux.frames,
        ux.max_us,
        nexus_ux_budgets::FRAME_US,
        LAST.map_or(0, |l| l.ux_max_us)
    ));
}
