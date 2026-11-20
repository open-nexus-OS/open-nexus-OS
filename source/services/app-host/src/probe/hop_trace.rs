// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's frame timeline at its stamp sites (RFC-0108 §hops amended, TASK-0373 P1):
//! the record (`crate::hop_record`, host-tested) fed from the paint path — a dispatch that
//! changed the model and its relayout (`damage.rs`, `interaction.rs`), the rows painted
//! (`paint.rs`, `damage.rs`, `paint/split.rs`), the raster and the present (`band.rs`), the
//! present's ack (the loop in `mod.rs`), the bytes copied into the slot (`surface_mem.rs`) —
//! and its lines printed off the frame path: at the input window's line (`band.rs`, beside
//! `apphost: hops input (`) and when the loop's one-shot finds app-host idle (`overlay.rs`).
//! Each stamp is one `nsec()` read; a record is one fixed write into the ring.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `crate::hop_record` host tests; the lanes' `apphost: hops hist (`

use super::boot_env::nsec_now;
use super::state::DslApp;
use super::surface_mem::SurfaceMem;

/// A hop line, to the console (never folded or cut below the line's own cap).
fn print(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// The present in flight was acked (the loop): the work waiting for it waited for its credit.
pub(super) fn acked(app: Option<&mut DslApp>) {
    if let Some(dsl) = app {
        dsl.hops.acked(nsec_now(), dsl.input_rx_ns);
    }
}

impl DslApp {
    /// A dispatch that changed the model ended; `relaid`: a relayout ran in it (it stamped its
    /// own layout).
    pub(super) fn hop_dispatched(&mut self, relaid: bool) {
        self.hops.dispatched(nsec_now(), !relaid);
    }

    /// A relayout starts (the dispatch before it is done) or ends.
    pub(super) fn hop_relayout(&mut self, done: bool) {
        if done {
            self.hops.laid_out(nsec_now());
        } else {
            self.hops.dispatched(nsec_now(), false);
        }
    }

    /// `rows` rows (`px` pixels) were painted in `us`: the motion episode's raster cost and the
    /// frame's counts.
    pub(super) fn note_painted(&mut self, us: u64, rows: u32, px: u64) {
        self.motion_hops.painted(us, rows);
        self.hops.painted(rows, px);
    }

    /// The present's raster begins (`Some(false)`), is done (`Some(true)`), or showed nothing
    /// (`None`).
    pub(super) fn hop_raster(&mut self, done: Option<bool>) {
        match done {
            Some(false) => self.hops.frame_begin(nsec_now()),
            Some(true) => self.hops.rastered(nsec_now()),
            None => self.hops.nothing_shown(),
        }
    }

    /// The present `seq` of `surface_id` left: its record, with what `mem` copied for it and the
    /// input it answers (taken by `note_input_answered` right after).
    pub(super) fn hop_sent(&mut self, (surface_id, seq): (u32, u32), mem: &mut SurfaceMem) {
        self.hops.copied(mem.take_copied());
        self.hops.sent((surface_id, seq), nsec_now(), self.input_rx_ns);
    }

    /// The input window's line: the histogram, the slow frames, the cost once.
    pub(super) fn hop_window(&mut self) {
        self.hops.window(&nsec_now, &mut print);
    }

    /// When the loop's one-shot should wake for the slow frames (`None`: nothing owed).
    pub(super) fn hop_idle_deadline(&self) -> Option<u64> {
        self.hops.idle_deadline()
    }

    /// The loop's one-shot fired at `now_ns`: idle, the slow frames print.
    pub(super) fn hop_idle_fired(&mut self, now_ns: u64) {
        self.hops.idle_fired(now_ns, &mut print);
    }
}
