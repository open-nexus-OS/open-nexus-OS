// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd main-loop cadence telemetry (SMP-flicker triage) — the 1s
//! `windowd: loop hz=…` window plus the pacer-slip histogram and NACK counters,
//! and the `OP_TIMER_FIRED` payload decode both recv sites share.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `pacer_slip_bucket` unit-tested in `crate::telemetry`; window
//!   emission proven via QEMU uart.log (`windowd: loop hz=` lines under drag).

use super::runtime::DisplayServerRuntime;
use nexus_abi::debug_println;

/// Kernel `OP_TIMER_FIRED` opcode (payload byte 0).
const OP_TIMER_FIRED: u8 = 0x30;

/// Decode a kernel `OP_TIMER_FIRED` frame into `(now_ns, deadline_ns)` — the
/// kernel stamps both (payload [13..21] = armed deadline, [21..29] = now at
/// delivery), so `now - deadline` measures how late the timer IRQ actually
/// fired without any kernel change (pacer-slip histogram).
pub(super) fn decode_timer_fired(frame: &[u8]) -> Option<(u64, u64)> {
    if frame.len() < 29 || frame[0] != OP_TIMER_FIRED {
        return None;
    }
    let deadline_ns = u64::from_le_bytes([
        frame[13], frame[14], frame[15], frame[16], frame[17], frame[18], frame[19], frame[20],
    ]);
    let now_ns = u64::from_le_bytes([
        frame[21], frame[22], frame[23], frame[24], frame[25], frame[26], frame[27], frame[28],
    ]);
    Some((now_ns, deadline_ns))
}

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
    slip: [u32; 4],
}

impl LoopTelemetry {
    pub(super) const fn new() -> Self {
        Self {
            window_start_ns: 0,
            iters: 0,
            applies: 0,
            seq_base: 0,
            nack_base: 0,
            fullrq_base: 0,
            slip: [0; 4],
        }
    }

    /// Count one staged-input application (at most one per loop iteration).
    pub(super) fn note_apply(&mut self, applied: bool) {
        self.applies += applied as u32;
    }

    /// Record one `OP_TIMER_FIRED` delivery into the slip histogram.
    pub(super) fn note_timer_fired(&mut self, now_ns: u64, deadline_ns: u64) {
        self.slip[crate::telemetry::pacer_slip_bucket(now_ns.saturating_sub(deadline_ns))] += 1;
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
            let _ = debug_println(&alloc::format!(
                "windowd: loop hz={} apply={} present={} nack={} fullrq={} slip={}/{}/{}/{}",
                self.iters,
                self.applies,
                presents,
                nacks,
                fullrq,
                self.slip[0],
                self.slip[1],
                self.slip[2],
                self.slip[3],
            ));
        }
        self.rebase(now_ns, runtime);
        self.iters = 0;
        self.applies = 0;
        self.slip = [0; 4];
    }

    fn rebase(&mut self, now_ns: u64, runtime: &DisplayServerRuntime) {
        self.window_start_ns = now_ns;
        self.seq_base = runtime.present_seq_value();
        self.nack_base = runtime.nack_total();
        self.fullrq_base = runtime.nack_full_recompose_total();
    }
}
