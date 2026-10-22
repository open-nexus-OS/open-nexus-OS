// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd main-loop cadence telemetry (SMP-flicker triage) — the 1s
//! `windowd: loop hz=…` window plus the NACK counters, why its frames were sent
//! (`frame_causes`), the frame chain's hops and the frame pace (RFC-0110: the presents shown
//! when windowd predicted), and the `OP_TIMER_FIRED` payload decode both recv sites share.
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
    /// RFC-0110: the judged landings (on time, late, early) at the window's start.
    pace_base: [u32; 3],
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
            pace_base: [0; 3],
        }
    }

    /// Count one staged-input application (at most one per loop iteration).
    pub(super) fn note_apply(&mut self, applied: bool) {
        self.applies += applied as u32;
    }

    /// Per-iteration window bookkeeping: counts the iteration and emits/rolls
    /// the 1s window. Every window with traffic (a present) reports — the old
    /// gate of eight presents hid the board, which presents fewer (cycle 51);
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
        if presents >= 1 {
            let nacks = runtime.nack_total().wrapping_sub(self.nack_base);
            let fullrq = runtime.nack_full_recompose_total().wrapping_sub(self.fullrq_base);
            // TASK-0054C P2-g: where the replies of this window went. `shared`
            // is the fallback onto windowd's own response endpoint — the route
            // that can wedge the compositor, so it belongs next to the cadence.
            let (ch, cap, shared) = super::reply_route::counts();
            let (content, blit, pointer, moved, keepalive, shape) = super::frame_causes::take();
            let (rects, queued_px, flushed_px) = super::frame_causes::take_damage();
            let (flush_us, copy_rows, copy_us, base_rows, base_us) =
                super::frame_causes::take_work();
            let refused = super::frame_causes::take_refused();
            let hops = super::frame_causes::take_frame_hops();
            // ADR-0075: the clients windowd did not wait on — behind, their deepest outbox,
            // the intent refused because a client was far behind.
            let (behind, queued_max, dropped) = runtime.client_census();
            let _ = debug_println(&alloc::format!(
                "windowd: loop hz={} apply={} present={} nack={} fullrq={} reply(ch={} cap={} shared={}) frames(content={} blit={} pointer={} moved={} keepalive={} shape={}) damage(rects={rects} queued_px={queued_px} flushed_px={flushed_px}) clients(behind={behind} queued_max={queued_max} dropped={dropped}) work(flush_us_max={flush_us} copy_rows={copy_rows} copy_us={copy_us} base_rows={base_rows} base_us={base_us}) layers(refused={refused})",
                self.iters,
                self.applies,
                presents,
                nacks,
                fullrq,
                ch,
                cap,
                shared,
                content,
                blit,
                pointer,
                moved,
                keepalive,
                shape,
            ));
            // RFC-0108 §hops: the frame chain's hops in windowd, in the shape every service
            // prints — owed → sent, sent → its credit back.
            let [(os_avg, os_max, os_n), (sa_avg, sa_max, sa_n)] = hops;
            if os_n + sa_n > 0 {
                let _ = debug_println(&alloc::format!(
                    "windowd: hops frame (owed>sent={os_avg}/{os_max}/{os_n} \
                     sent>acked={sa_avg}/{sa_max}/{sa_n})"
                ));
            }
            // RFC-0110: the presents shown when windowd predicted — the time the clients'
            // frame pulses named — more than half a period late, or early; the cadence gpud holds.
            let (judged, cadence, period_ns) = runtime.frame_pace();
            let [on_time, late, early] =
                core::array::from_fn(|k| judged[k].wrapping_sub(self.pace_base[k]));
            if on_time + late + early > 0 {
                let _ = debug_println(&alloc::format!(
                    "windowd: frame pace (on_time={on_time} late={late} early={early} \
                     cadence={cadence} period_us={})",
                    period_ns / 1000
                ));
            }
        } else {
            // A quiet window's counters are not carried into the next one.
            let _ = super::frame_causes::take();
            let _ = super::frame_causes::take_damage();
            let _ = super::frame_causes::take_work();
            let _ = super::frame_causes::take_refused();
            let _ = super::frame_causes::take_frame_hops();
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
        self.pace_base = runtime.frame_pace().0;
    }
}
