// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's time rights (TASK-0363 P3, RFC-0120) — one table of rights and
//! reservations (`nexus_sched::TimeRights`, the host-tested contract) behind its own leaf lock,
//! reset for the machine's harts at boot. A task's reservation lives in its scheduling record;
//! the run queues carry a copy in their clock (`run_queue::Res`) taken as the task arrives on a
//! hart (the constant-bandwidth rule: a sleep brings back no time), and the meter's charges are
//! booked here after the queue lock is dropped. The running holder's budget end is kept per hart
//! so the timer fires there (`trap::timer_arm`, `arm_wakeup`): a budget is enforced to the
//! clock, not to the tick. Lock order: this table, then a hart's queues — never the other way.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the contract in `nexus-sched/tests/time_right.rs`, the order in
//!   `run_queue_order_tests.rs` (host); `KSELFTEST: sched time right ok`, the syscall's
//!   `test_reject_*` (`selftest/smp_sched/time_right.rs`)
//! ADR: docs/adr/0101-the-fair-class-orders-by-virtual-deadline-a-reservation-runs-first.md

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};

use nexus_sched::{ReservationId, RightId, TimeRightError, TimeRights};

use super::*;
use crate::hal::platform::{ns_to_ticks, ticks_to_ns};
use crate::run_queue::{order::Charge, Res};

/// Rights the table holds (the share floor bounds what one holder can make: RFC-0120).
pub const RIGHTS: usize = 64;
/// Reservations the table holds.
pub const RESERVATIONS: usize = 64;
/// Harts the table can admit on: a CPU set's bits.
const HARTS: usize = 64;
/// What a hart's reservations may sum to: four fifths — the fair class keeps the rest of every
/// hart (RFC-0120's open question, decided here: the board's busiest hart ran 0.3–0.6 of its
/// time under use, `KINIT: hart busy pm`, so a fifth floor leaves every fair task room).
pub const CAPACITY_PPM: u32 = 800_000;

type Table = TimeRights<RIGHTS, RESERVATIONS, HARTS>;

static TABLE: SpinIrqLock<Table> = SpinIrqLock::new(Table::EMPTY);
/// The root is handed out once, to init (`claim_root`).
static ROOT_CLAIMED: AtomicBool = AtomicBool::new(false);
/// Per hart: when the running holder's budget ends (ns; 0: it runs no reservation).
static BUDGET_END: HartTable<AtomicU64> = HartTable::new(AtomicU64::new(0));
/// Budgets that ran out while their holder ran (it went on in the fair class).
static OVERRUNS: AtomicUsize = AtomicUsize::new(0);
/// Periods in which a runnable holder did not get its budget (`HartQueues::take_missed`).
static MISSES: AtomicUsize = AtomicUsize::new(0);

/// Resets the table for a machine of `harts` CPUs: the root holds their capacity.
pub(super) fn size(harts: usize) -> Result<(), SizeError> {
    TABLE.lock().reset(harts, CAPACITY_PPM);
    BUDGET_END.size(harts, || AtomicU64::new(0))
}

fn now_ns() -> u64 {
    ticks_to_ns(ready::now_ticks())
}

/// The reservation task `id` runs with as it arrives on a hart at `now` (queue clock): the
/// constant-bandwidth rule applied. `None`: none, or one whose right was revoked (cleared).
pub(super) fn arrive(id: u32, now: u64) -> Option<Res> {
    let record = record::get(Pid::from_raw(id))?;
    let raw = record.reservation();
    if raw == 0 {
        return None;
    }
    let res = ReservationId::from_raw(raw);
    let mut table = TABLE.lock();
    let Ok((deadline, left)) = table.wake(res, ticks_to_ns(now)) else {
        record.set_reservation(0);
        return None;
    };
    let (_, _, _, budget) = table.state(res)?;
    let period = table.period_of(res)?;
    Some(Res {
        deadline: ns_to_ticks(deadline),
        left: ns_to_ticks(left),
        period: ns_to_ticks(period),
        budget: ns_to_ticks(budget),
    })
}

/// Books a charge of the meter (after the queue lock): an overrun is counted, a holder's
/// reservation is charged — and refreshed past its deadline — in the table; a revoked one is
/// cleared from the record (its copy in the queue runs out with its period).
pub(super) fn charged(charge: &Charge, now: u64) {
    if charge.exhausted {
        OVERRUNS.fetch_add(1, Ordering::Relaxed);
    }
    let Some(record) = record::get(Pid::from_raw(charge.id)) else { return };
    let raw = record.reservation();
    if raw == 0 {
        return;
    }
    let (res, now) = (ReservationId::from_raw(raw), ticks_to_ns(now));
    let mut table = TABLE.lock();
    if table.charge(res, ticks_to_ns(charge.ran), now) == Err(TimeRightError::Revoked) {
        record.set_reservation(0);
        return;
    }
    if table.state(res).is_some_and(|(_, deadline, _, _)| now >= deadline) {
        let _ = table.replenish(res, now);
    }
}

/// Hart `cpu` runs `task` from `now` (queue clock), just charged: a holder with budget left
/// arms the hart's timer at its budget's end — on its own hart only (another hart's queue
/// operation, a purge, notes the end and leaves the timer to that hart's next arm).
pub(super) fn runs(cpu: usize, task: Option<Task>, now: u64) {
    let left = task.and_then(|t| t.res).filter(Res::runs).map(|r| r.left);
    let end = left.map_or(0, |left| ticks_to_ns(now.saturating_add(left)));
    if let Some(slot) = BUDGET_END.get(cpu) {
        slot.store(end, Ordering::Relaxed);
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
    if let Some(left) = left.filter(|_| cpu == crate::smp::cpu_current_id().as_index()) {
        crate::trap::timer_arm(left.min(crate::trap::default_tick_cycles()));
    }
}

/// When the holder hart `cpu` runs has spent its budget (ns), if it runs one — where a timer
/// arm caps the preemption tick.
pub fn budget_end_ns(cpu: usize) -> Option<u64> {
    BUDGET_END.get(cpu).map(|slot| slot.load(Ordering::Relaxed)).filter(|&end| end != 0)
}

/// Books the periods a hart's queues found missed (after the queue lock).
pub(super) fn missed(periods: u64) {
    if periods != 0 {
        MISSES.fetch_add(periods as usize, Ordering::Relaxed);
    }
}

/// Since boot: the budgets that ran out while their holder ran, and the periods in which a
/// runnable holder did not get its budget — what a guarantee is judged by (zero).
pub fn counts() -> (usize, usize) {
    (OVERRUNS.load(Ordering::Relaxed), MISSES.load(Ordering::Relaxed))
}

/// The most any hart has reserved (ppm of it) and the hart.
pub fn load() -> (u32, usize) {
    let (table, now) = (TABLE.lock(), now_ns());
    let (harts, _) = table.machine();
    (0..harts)
        .map(|hart| (table.load(hart, now), hart))
        .max_by_key(|&(ppm, _)| ppm)
        .unwrap_or((0, 0))
}

/// The root right for the selftest that runs before init claims it (`selftest/smp_sched`): what
/// it derives it revokes before it returns.
pub fn selftest_root() -> RightId {
    TABLE.lock().root()
}

/// Whether task `pid` runs with a reservation.
pub fn holds(pid: TaskId) -> bool {
    record::get(pid).is_some_and(|record| record.reservation() != 0)
}

/// The hart task `pid`'s reservation is admitted on — where it runs.
pub fn hart_of(pid: TaskId) -> Option<usize> {
    let raw = record::get(pid)?.reservation();
    (raw != 0).then(|| TABLE.lock().state(ReservationId::from_raw(raw)).map(|s| s.0)).flatten()
}

/// The root right, once: to init, the one task that divides the machine's time (RFC-0120).
pub fn claim_root() -> Option<RightId> {
    (!ROOT_CLAIMED.swap(true, Ordering::AcqRel)).then(|| TABLE.lock().root())
}

/// A right of `share` ppm derived from `parent`, nothing finer than `grain` ppm ever taken from
/// it (0: the parent's grain).
pub fn derive(parent: RightId, share: u32, grain: u32) -> Result<RightId, TimeRightError> {
    TABLE.lock().derive(parent, share, grain)
}

/// What `right` was given and what of it it still holds: `(total, own)`.
pub fn held(right: RightId) -> Option<(u32, u32)> {
    TABLE.lock().held(right)
}

/// Revokes `right` and everything derived from it, in one step.
pub fn revoke(right: RightId) -> Result<(), TimeRightError> {
    TABLE.lock().revoke(right)
}

/// Binds `share` ppm of `right` with `period_ns` to task `pid`, on the first of `harts` that
/// admits it; a reservation the task held is ended first. Takes effect in its queue entry at
/// once, wherever it is. Returns the hart.
pub fn bind(
    pid: TaskId,
    right: RightId,
    share: u32,
    period_ns: u64,
    harts: impl Iterator<Item = usize>,
) -> Result<usize, TimeRightError> {
    let record = record::get(pid).ok_or(TimeRightError::Revoked)?;
    let now = now_ns();
    let bound = {
        let mut table = TABLE.lock();
        let old = record.reservation();
        if old != 0 {
            let _ = table.unbind(ReservationId::from_raw(old), now);
            record.set_reservation(0);
        }
        let mut refusal = TimeRightError::Hart;
        let mut bound = None;
        for hart in harts {
            match table.bind(right, share, period_ns, hart, now) {
                Ok(res) => {
                    bound = Some((res, hart));
                    break;
                }
                Err(e @ (TimeRightError::BeyondCapacity | TimeRightError::Hart)) => refusal = e,
                Err(e) => return Err(e),
            }
        }
        let (res, hart) = bound.ok_or(refusal)?;
        record.set_reservation(res.to_raw());
        hart
    };
    let snapshot = arrive(pid.as_raw(), ready::now_ticks());
    for cpu in 0..RUN_QUEUES.len() {
        let _ = with(cpu, |q| q.set_res(pid.as_raw(), snapshot));
    }
    Ok(bound)
}

/// Ends task `pid`'s reservation if it was bound from `right` (the right that bound it decides
/// its end). `Err(Revoked)` also when it holds none or another right's.
pub fn unbind(pid: TaskId, right: RightId) -> Result<(), TimeRightError> {
    let record = record::get(pid).ok_or(TimeRightError::Revoked)?;
    let raw = record.reservation();
    let res = ReservationId::from_raw(raw);
    {
        let mut table = TABLE.lock();
        if raw == 0 || table.right_of(res) != Some(right) {
            return Err(TimeRightError::Revoked);
        }
        table.unbind(res, now_ns())?;
    }
    record.set_reservation(0);
    for cpu in 0..RUN_QUEUES.len() {
        let _ = with(cpu, |q| q.set_res(pid.as_raw(), None));
    }
    Ok(())
}

/// Task `pid` is gone: its reservation's share is its right's again.
pub fn release(pid: TaskId) {
    let Some(record) = record::get(pid) else { return };
    let raw = record.reservation();
    if raw != 0 {
        let _ = TABLE.lock().unbind(ReservationId::from_raw(raw), now_ns());
        record.set_reservation(0);
    }
}

/// The books balance (`TimeRights::audit`) — for the selftest.
pub fn audit() -> bool {
    TABLE.lock().audit(now_ns())
}
