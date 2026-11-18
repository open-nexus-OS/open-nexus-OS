// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the scheduler's order on the real clock (TASK-0363 P3, RFC-0120, ADR-0101) — the
//! boot hart's queues driven as its timer drives them (pick, hold the hart for a quantum or to
//! the picked holder's budget end, preempt), through the scheduler's own paths: the meter, the
//! time-right table, the budget end the timer is capped at. Three proofs: two weights share the
//! hart in their ratio; a reservation's budget is run in each of a hundred periods, every
//! overrun counted, no period missed, the books balanced after its end; and the time-right
//! syscall refuses what it must (`test_reject_*`, through the syscall table). The order itself
//! is host-tested (`run_queue_order_tests.rs`); these prove the kernel's wiring of it.
//! Bounds are exact on the same clock: a host that stalls the hart (a multi-hart QEMU lane
//! under load) widens the bound by what its stall measurably was, never by a tolerance.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the `KSELFTEST: sched fair`, `sched time right` and
//!   `test_reject_time_right_*` lines, on the multi-hart lanes and the board
//! ADR: docs/adr/0101-the-fair-class-orders-by-virtual-deadline-a-reservation-runs-first.md

use super::super::Context;
use crate::cap::{CapError, Capability, CapabilityKind, Held, Rights};
use crate::hal::platform::{ns_to_ticks, ticks_per_us, ticks_to_ns};
use crate::mm::AddressSpaceError;
use crate::sched::{meter, reserve, EnqueueOutcome, QosClass};
use crate::syscall::{api, Args, Error as SysError, SyscallTable, SYSCALL_SCHED};
use crate::task::Pid;
use crate::types::CpuId;
use crate::{log_error, log_info};
use nexus_sched::{TimeRightOp, TIME_RIGHT_OP};

/// The longest the drive holds the hart for one pick.
const QUANTUM_US: u64 = 250;
/// The reservation of the proof: a fifth of the hart every two milliseconds (400 µs).
const SHARE_PPM: u32 = 200_000;
const PERIOD_NS: u64 = 2_000_000;
const PERIODS: usize = 100;

fn clock() -> u64 {
    riscv::register::time::read() as u64
}

/// What a drive of the boot hart saw (clock ticks).
#[derive(Default)]
struct Drive {
    span: u64,
    /// The longest one pick held the hart — a quantum at most, unless the host stalled it.
    max_hold: u64,
    /// Periods covered by holds longer than a period less the budget: only a host's stall
    /// holds the hart that long (four times the longest the drive asks for).
    stalled: u64,
}

/// Runs the boot hart's queues as its timer does until `done` or `limit` ticks: pick, hold the
/// hart for a quantum — or to the picked holder's budget end, where the timer is capped — and
/// preempt.
fn drive(ctx: &mut Context<'_>, boot: CpuId, limit: u64, mut done: impl FnMut() -> bool) -> Drive {
    let quantum = QUANTUM_US * ticks_per_us();
    let (period, budget) = (ns_to_ticks(PERIOD_NS), ns_to_ticks(PERIOD_NS / 5));
    let begin = clock();
    let mut seen = Drive::default();
    while !done() && clock().wrapping_sub(begin) < limit {
        if ctx.scheduler.schedule_next().is_none() {
            break;
        }
        let start = clock();
        let hold = reserve::budget_end_ns(boot.as_index())
            .map_or(quantum, |end| (ns_to_ticks(end).saturating_sub(start) + 1).min(quantum));
        while clock().wrapping_sub(start) < hold {
            core::hint::spin_loop();
        }
        ctx.scheduler.preempt_current();
        let held = clock().wrapping_sub(start);
        seen.max_hold = seen.max_hold.max(held);
        if held > period - budget {
            seen.stalled += held / period + 1;
        }
    }
    seen.span = clock().wrapping_sub(begin);
    seen
}

fn queued(ctx: &mut Context<'_>, boot: CpuId, pid: Pid, qos: QosClass) -> bool {
    matches!(ctx.scheduler.enqueue_on_cpu(boot, pid, qos), EnqueueOutcome::Enqueued)
}

/// Two hogs, one four times the other's weight, share the boot hart for a tenth of a second:
/// the lighter one's run time is a fifth of the sum, off by at most two of the longest holds
/// (the fair class's bound: a task is never further from its share than the turns in flight) —
/// within 5 % when no hold was stalled (two quanta of a fifth of 100 ms are 2.5 %).
fn run_fair(ctx: &mut Context<'_>, boot: CpuId) {
    const RUN_US: u64 = 100_000;
    let light = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let heavy = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    ctx.scheduler.selftest_reset_cpu(boot);
    let ready = queued(ctx, boot, light, QosClass::Normal)
        && queued(ctx, boot, heavy, QosClass::Interactive);
    let seen = drive(ctx, boot, RUN_US * ticks_per_us(), || false);
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.purge(light);
    ctx.scheduler.purge(heavy);
    let (light_ns, heavy_ns) = (meter::runtime_ns(light), meter::runtime_ns(heavy));
    let fifth = (light_ns + heavy_ns) / 5;
    let off = light_ns.abs_diff(fifth);
    let off_pm = off * 1_000 / fifth.max(1);
    let max_hold_ns = ticks_to_ns(seen.max_hold);
    if ready && light_ns > 0 && heavy_ns > 0 && off <= 2 * max_hold_ns {
        log_info!(
            target: "selftest",
            "KSELFTEST: sched fair ok (weights=4:1 heavy_us={} light_us={} off_pm={} max_hold_us={})",
            heavy_ns / 1_000,
            light_ns / 1_000,
            off_pm,
            max_hold_ns / 1_000
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: sched fair FAIL (ready={} heavy_us={} light_us={} off_pm={} max_hold_us={})",
            ready,
            heavy_ns / 1_000,
            light_ns / 1_000,
            off_pm,
            max_hold_ns / 1_000
        );
    }
}

/// A holder of a fifth of the boot hart every 2 ms against a hog of eight times its weight (its
/// fair share is a ninth): driven until its budget ran out a hundred times. Each overrun is a
/// budget run in full, so it ran at least a hundred budgets; no period of a runnable holder
/// ended with budget unspent, but those a measured stall of the host covered; and after the
/// unbind and the revoke the root holds what it held and the books balance.
fn run_reservation(ctx: &mut Context<'_>, boot: CpuId) {
    let budget_us = PERIOD_NS / 5 / 1_000;
    let holder = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let crowd = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    ctx.scheduler.selftest_reset_cpu(boot);
    let root = reserve::selftest_root();
    let before = reserve::held(root);
    let Ok(right) = reserve::derive(root, SHARE_PPM) else {
        log_error!(target: "selftest", "KSELFTEST: sched time right FAIL (derive)");
        return;
    };
    let hart = core::iter::once(boot.as_index());
    let bound = reserve::bind(holder, right, SHARE_PPM, PERIOD_NS, hart) == Ok(boot.as_index());
    let (overruns0, misses0) = reserve::counts();
    let ready = bound
        && queued(ctx, boot, holder, QosClass::Normal)
        && queued(ctx, boot, crowd, QosClass::PerfBurst);
    let limit = 4 * PERIODS as u64 * ns_to_ticks(PERIOD_NS);
    let seen = drive(ctx, boot, limit, || reserve::counts().0 - overruns0 >= PERIODS);
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.purge(holder);
    ctx.scheduler.purge(crowd);
    let (overruns, misses) = reserve::counts();
    let (overruns, misses) = (overruns - overruns0, misses - misses0);
    let ran_us = meter::runtime_ns(holder) / 1_000;
    let periods = ticks_to_ns(seen.span).div_ceil(PERIOD_NS);
    let released = reserve::unbind(holder, right).is_ok() && reserve::revoke(right).is_ok();
    let balanced = reserve::audit() && reserve::held(root) == before;
    // A microsecond for the meter's rounding of each charge to nanoseconds.
    let ran_budgets = ran_us + 1 >= overruns as u64 * budget_us;
    if ready
        && overruns >= PERIODS
        && ran_budgets
        && misses as u64 <= seen.stalled
        && released
        && balanced
    {
        log_info!(
            target: "selftest",
            "KSELFTEST: sched time right ok (budgets={} of_us={} ran_us={} periods={} missed={} stalled={})",
            overruns,
            budget_us,
            ran_us,
            periods,
            misses,
            seen.stalled
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: sched time right FAIL (ready={} budgets={} ran_us={} periods={} missed={} stalled={} released={} balanced={})",
            ready,
            overruns,
            ran_us,
            periods,
            misses,
            seen.stalled,
            released,
            balanced
        );
    }
}

/// The caller's slots of the syscall proof.
const RIGHT_SLOT: usize = 60;
const VIEW_SLOT: usize = 61;
const KIND_SLOT: usize = 62;
const EMPTY_SLOT: usize = 63;

fn verdict(name: &str, ok: bool) {
    if ok {
        log_info!(target: "selftest", "KSELFTEST: test_reject_time_right_{} ok", name);
    } else {
        log_error!(target: "selftest", "KSELFTEST: test_reject_time_right_{} FAIL", name);
    }
}

/// The time-right syscall's refusals, through the syscall table, by a task that is not init and
/// holds a tenth of a hart: the root is not its to claim; a slot of another kind, one without
/// `MANAGE` and an empty one name no right; it derives no more than it holds; it binds no other
/// task; and a right derived from one that was revoked binds nothing — its holder's reservation
/// ended with it.
fn run_syscall_rejects(ctx: &mut Context<'_>) {
    const HELD_PPM: u32 = 100_000;
    let caller = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let other = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    let root = reserve::selftest_root();
    let before = reserve::held(root);
    let Ok(right) = reserve::derive(root, HELD_PPM) else {
        log_error!(target: "selftest", "KSELFTEST: test_reject_time_right FAIL (derive)");
        return;
    };
    let cap = |kind, rights| Held::plain(Capability { kind, rights });
    let time = CapabilityKind::TimeRight(right.to_raw());
    let staged = ctx.tasks.caps_of_mut(caller).is_some_and(|caps| {
        [
            (RIGHT_SLOT, cap(time, Rights::MANAGE)),
            (VIEW_SLOT, cap(time, Rights::empty())),
            (KIND_SLOT, cap(CapabilityKind::Timer(u32::MAX), Rights::MANAGE)),
        ]
        .into_iter()
        .all(|(slot, held)| held.is_ok_and(|held| caps.set(slot, held).is_ok()))
    });
    if !staged {
        log_error!(target: "selftest", "KSELFTEST: test_reject_time_right FAIL (stage)");
        return;
    }

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    sys.tasks.set_current(caller);
    let mut call = |op: TimeRightOp, a: usize, b: usize, c: usize, d: usize| {
        table.dispatch(
            SYSCALL_SCHED,
            &mut sys,
            &Args::new([TIME_RIGHT_OP, op as usize, a, b, c, d]),
        )
    };
    let denied = |r: Result<usize, SysError>| {
        matches!(r, Err(SysError::Capability(CapError::PermissionDenied)))
    };
    let (share, period) = (HELD_PPM as usize / 2, PERIOD_NS as usize);

    verdict("claim_by_stranger", denied(call(TimeRightOp::ClaimRoot, 0, 0, 0, 0)));
    verdict(
        "forged_slot",
        matches!(
            call(TimeRightOp::Held, KIND_SLOT, 0, 0, 0),
            Err(SysError::Capability(CapError::InvalidSlot))
        ) && denied(call(TimeRightOp::Derive, VIEW_SLOT, share, 0, 0))
            && matches!(
                call(TimeRightOp::Bind, EMPTY_SLOT, 0, share, period),
                Err(SysError::Capability(_))
            ),
    );
    let held = ((HELD_PPM as usize) << 32) | HELD_PPM as usize;
    verdict(
        "beyond_held",
        denied(call(TimeRightOp::Derive, RIGHT_SLOT, HELD_PPM as usize + 1, 0, 0))
            && denied(call(TimeRightOp::Bind, RIGHT_SLOT, 0, HELD_PPM as usize + 1, period))
            && matches!(
                call(TimeRightOp::Bind, RIGHT_SLOT, 0, share, 10),
                Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs))
            )
            && call(TimeRightOp::Held, RIGHT_SLOT, 0, 0, 0) == Ok(held),
    );
    verdict(
        "bind_other_task",
        denied(call(TimeRightOp::Bind, RIGHT_SLOT, other.as_index(), share, period))
            && !reserve::holds(other),
    );
    // What it may: derive half into a new slot and bind itself from it. Then the right it
    // derived from is revoked — the derived slot names nothing, its reservation is gone.
    let child = call(TimeRightOp::Derive, RIGHT_SLOT, share, 0, 0);
    let bound = child.is_ok_and(|slot| call(TimeRightOp::Bind, slot, 0, share, period).is_ok())
        && reserve::hart_of(caller).is_some();
    let revoked = call(TimeRightOp::Revoke, RIGHT_SLOT, 0, 0, 0) == Ok(0);
    let dead = child.is_ok_and(|slot| {
        denied(call(TimeRightOp::Bind, slot, 0, share, period))
            && denied(call(TimeRightOp::Held, slot, 0, 0, 0))
            && denied(call(TimeRightOp::Unbind, slot, 0, 0, 0))
    });
    reserve::release(caller);
    verdict(
        "after_revoke",
        bound
            && revoked
            && dead
            && reserve::hart_of(caller).is_none()
            && reserve::audit()
            && reserve::held(root) == before,
    );
    sys.tasks.set_current(Pid::KERNEL);
}

pub(super) fn run(ctx: &mut Context<'_>, boot: CpuId) {
    run_fair(ctx, boot);
    run_reservation(ctx, boot);
    run_syscall_rejects(ctx);
    ctx.tasks.set_current(Pid::KERNEL);
}
