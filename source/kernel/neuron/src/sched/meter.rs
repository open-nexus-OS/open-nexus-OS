// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the meter (TASK-0363 P3; the time track's gate G2, RFC-0120): every operation that
//! changes the task a hart runs is metered — the task that leaves is charged the time it ran,
//! into its scheduling record (what a reservation's `charge` and the fair class's virtual time
//! are fed from) and the hart's window sum (the `hart busy pm` line's `metered=`). The rule is
//! `HartQueues::charge`, host-tested; this file names the clock and the record. A task's run time
//! includes the kernel's work on its behalf (its traps, its syscalls): what its hart did while
//! it was the hart's task.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `run_queue_tests.rs` (host: the charge, every tick charged once);
//!   `KSELFTEST: sched runtime metered ok` (a known run on the boot hart, through this path)
//! RFC: docs/rfcs/RFC-0120-time-is-a-right-a-share-derived-as-a-subset-a-reservation-admitted-per-hart-revoked-in-one-step.md
//! ADR: docs/adr/0101-the-fair-class-orders-by-virtual-deadline-a-reservation-runs-first.md

use core::sync::atomic::AtomicU64;

use super::*;

/// The run time metered on each hart since its window began (ticks).
static WINDOW: HartTable<AtomicU64> = HartTable::new(AtomicU64::new(0));

/// Sizes the window sums for a machine of `harts` CPUs (with the run queues).
pub(super) fn size(harts: usize) -> Result<(), SizeError> {
    WINDOW.size(harts, || AtomicU64::new(0))
}

/// Runs `f` on hart `cpu`'s queues under their lock, the running task charged to now first
/// (its virtual time, its budget — so `f` decides on the time it ran); the charge is booked and
/// the hart's new running task noted after the lock is dropped. `None`: no such hart.
pub(super) fn metered<R>(cpu: usize, f: impl FnOnce(&mut HartQueues) -> R) -> Option<R> {
    let now = ready::now_ticks();
    let (out, charge, runs, missed) = with(cpu, |q| {
        let charge = q.charge(now);
        let out = f(q);
        (out, charge, q.current(), q.take_missed())
    })?;
    if let Some(charge) = charge {
        book(cpu, &charge);
        reserve::charged(&charge, now);
    }
    reserve::missed(missed);
    reserve::runs(cpu, runs, now);
    Some(out)
}

/// Task `charge.id` ran `charge.ran` ticks on hart `cpu` (a probe id without a record counts
/// for the hart only).
fn book(cpu: usize, charge: &crate::run_queue::order::Charge) {
    if let Some(sum) = WINDOW.get(cpu) {
        sum.fetch_add(charge.ran, Ordering::Relaxed);
    }
    if let Some(record) = record::get(Pid::from_raw(charge.id)) {
        record.add_runtime(crate::hal::platform::ticks_to_ns(charge.ran));
    }
}

/// The metered share of a window of `window_ns`, the harts' mean in permille; the sums start
/// again.
pub fn take_metered_pm(window_ns: u64) -> u64 {
    let harts = WINDOW.len().max(1) as u64;
    let sum: u64 = (0..WINDOW.len())
        .filter_map(|cpu| WINDOW.get(cpu))
        .map(|ticks| crate::hal::platform::ticks_to_ns(ticks.swap(0, Ordering::Relaxed)))
        .map(|ns| ns.min(window_ns))
        .sum();
    sum.saturating_mul(1000) / window_ns.max(1).saturating_mul(harts)
}

/// The run time metered for task `pid` so far (ns; 0 without a record).
pub fn runtime_ns(pid: TaskId) -> u64 {
    record::get(pid).map_or(0, |record| record.runtime_ns())
}
