// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd main-loop cadence telemetry (SMP-flicker triage) — the 1s
//! `windowd: loop hz=…` window plus the NACK counters,
//! and the `OP_TIMER_FIRED` payload decode both recv sites share.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `pacer_slip_bucket` unit-tested in `crate::telemetry`; window
//!   emission proven via QEMU uart.log (`windowd: loop hz=` lines under drag).

use super::runtime::DisplayServerRuntime;
use nexus_abi::debug_println;

/// Kernel `OP_TIMER_FIRED` opcode (payload byte 0).
/// Loop-cadence telemetry window (~1s), emitted only while input/present
/// traffic is flowing (idle stays silent). Measures the ACTUAL frame cadence
/// the pointer/scroll pipeline gets — independent of gpud's own present stats —
/// plus the SMP-flicker triage numbers: present NACKs, NACK-driven full-frame
/// recomposes, and the pacer-slip histogram (how late each `OP_TIMER_FIRED`
/// arrived vs its armed deadline; bucket 2/3 traffic during drag = the kernel
/// slipped the 8.33ms pacer deadline to a later tick — cadence jitter, not
/// render cost).
pub(super) struct LoopTelemetry {
    window_start_ns: u64,
    iters: u32,
    applies: u32,
    seq_base: u32,
    nack_base: u32,
    fullrq_base: u32,
}

impl LoopTelemetry {
    pub(super) const fn new() -> Self {
        Self { window_start_ns: 0, iters: 0, applies: 0, seq_base: 0, nack_base: 0, fullrq_base: 0 }
    }

    /// Count one staged-input application (at most one per loop iteration).
    pub(super) fn note_apply(&mut self, applied: bool) {
        self.applies += applied as u32;
    }

    /// Per-iteration window bookkeeping: counts the iteration and emits/rolls
    /// the 1s window. Only windows with real traffic (>=8 presents) report —
    /// boots and idle periods stay quiet.
    pub(super) fn tick(&mut self, now_ns: u64, runtime: &DisplayServerRuntime) {
        self.iters += 1;
        if self.window_start_ns == 0 {
            self.rebase(now_ns, runtime);
            return;
        }
        if now_ns.saturating_sub(self.window_start_ns) < 1_000_000_000 {
            return;
        }
        let presents = runtime.present_seq_value().wrapping_sub(self.seq_base);
        if presents >= 8 {
            let nacks = runtime.nack_total().wrapping_sub(self.nack_base);
            let fullrq = runtime.nack_full_recompose_total().wrapping_sub(self.fullrq_base);
            // TASK-0054C P2-g: where the replies of this window went. `shared`
            // is the fallback onto windowd's own response endpoint — the route
            // that can wedge the compositor, so it belongs next to the cadence.
            let (ch, cap, shared) = super::reply_route::counts();
            let _ = debug_println(&alloc::format!(
                "windowd: loop hz={} apply={} present={} nack={} fullrq={} reply(ch={} cap={} shared={})",
                self.iters,
                self.applies,
                presents,
                nacks,
                fullrq,
                ch,
                cap,
                shared,
            ));
        }
        self.rebase(now_ns, runtime);
        self.iters = 0;
        self.applies = 0;
    }

    fn rebase(&mut self, now_ns: u64, runtime: &DisplayServerRuntime) {
        self.window_start_ns = now_ns;
        self.seq_base = runtime.present_seq_value();
        self.nack_base = runtime.nack_total();
        self.fullrq_base = runtime.nack_full_recompose_total();
    }
}
