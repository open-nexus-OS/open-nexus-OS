// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic scheduler steal/EDT selftests (KSELFTEST markers):
//! work-stealing probe, ADR-0052 earliest-deadline arming + home-CPU steal
//! park, and the steal-policy negative checks. Split out of `selftest/mod.rs`
//! (structure-gate); runs from `run_smp_selftests`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md

use super::Context;
use crate::sched::{EnqueueOutcome, QosClass};
use crate::task::Pid;
use crate::types::CpuId;
use crate::{log_error, log_info};

mod loan;
mod time_right;

/// TASK-0363 P5: the placement rule (`placement::place`) on this machine's topology — as many
/// tasks as there are harts, woken in a row by the boot hart and homed there, are placed on as
/// many harts at their wake (the first waits for the handoff at home, each next one takes a free
/// hart, its home's cluster first): no task waits for a steal's next look.
fn run_spread_selftest(boot: CpuId) {
    use nexus_sched::CpuSet;
    let harts = crate::topology::hart_count();
    let all = crate::topology::runs();
    let me = CpuSet::single(boot.as_index()).unwrap_or(CpuSet::from_bits(0));
    let (mut free, mut placed) = (all.without(me), CpuSet::from_bits(0));
    for n in 0..harts {
        let wake = crate::placement::Wake {
            home: boot.as_index(),
            waker: boot.as_index(),
            allowed: all,
            free,
            near: crate::topology::cluster_of(boot.as_index()),
            backlog: n > 0,
            home_runs_lower: false,
        };
        let one = CpuSet::single(crate::placement::place(&wake)).unwrap_or(me);
        placed = placed.union(one);
        free = free.without(one);
    }
    if placed.count() == harts {
        log_info!(target: "selftest", "KSELFTEST: sched spread ok (harts={} placed={})", harts, harts);
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: sched spread FAIL harts={} placed={}",
            harts,
            placed.count()
        );
    }
}

/// TASK-0363 P3: the meter on the scheduler's own switch path — a task picked on the boot hart,
/// held there for a known time and switched away from, is charged at least that time and at
/// most the time from before its pick to after its switch away, read on the same clock. Exact
/// bounds, no tolerance: a host that stalls the hart between the steps widens the outer bound
/// with the charge (a multi-hart QEMU lane under load stalled one by 0.7 ms).
fn run_meter_selftest(ctx: &mut Context<'_>, boot: CpuId) {
    use crate::hal::platform::{ticks_per_us, ticks_to_ns};
    const RUN_US: u64 = 2_000;
    let clock = || riscv::register::time::read() as u64;
    let pid = ctx.tasks.selftest_create_dummy_task(Pid::KERNEL, ctx.scheduler);
    ctx.scheduler.selftest_reset_cpu(boot);
    let queued = matches!(
        ctx.scheduler.selftest_enqueue_on_cpu(boot, pid, QosClass::Normal),
        EnqueueOutcome::Enqueued
    );
    let before = clock();
    let picked = queued && ctx.scheduler.selftest_schedule_on_cpu(boot) == Some(pid);
    let (start, run) = (clock(), RUN_US * ticks_per_us());
    while clock().wrapping_sub(start) < run {
        core::hint::spin_loop();
    }
    ctx.scheduler.selftest_reset_cpu(boot);
    let (span_us, ran_ns) =
        (ticks_to_ns(clock().wrapping_sub(before)) / 1_000, crate::sched::meter::runtime_ns(pid));
    let ran_us = ran_ns / 1_000;
    if picked && ran_ns >= ticks_to_ns(run) && ran_us <= span_us {
        log_info!(
            target: "selftest",
            "KSELFTEST: sched runtime metered ok (ran_us={} of={} span_us={})",
            ran_us,
            RUN_US,
            span_us
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: sched runtime metered FAIL (picked={} ran_us={} of={} span_us={})",
            picked,
            ran_us,
            RUN_US,
            span_us
        );
    }
}

pub(super) fn run_steal_selftests(ctx: &mut Context<'_>, boot: CpuId, target: CpuId) {
    run_meter_selftest(ctx, boot);
    time_right::run(ctx, boot);
    loan::run(ctx, boot, target);
    run_spread_selftest(boot);
    // Deterministic work-stealing probe:
    // - put one runnable task only on CPU1
    // - run CPU0 pick path and require a steal event + expected PID
    let probe_pid = Pid::from_raw(0x7FFF_FF00);
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.selftest_reset_cpu(target);
    if !matches!(
        ctx.scheduler.selftest_enqueue_on_cpu(target, probe_pid, QosClass::Normal),
        EnqueueOutcome::Enqueued
    ) {
        log_error!(target: "selftest", "KSELFTEST: work stealing FAIL enqueue");
        return;
    }
    let picked = ctx.scheduler.selftest_schedule_on_cpu(boot);
    let steal_count = crate::smp::work_steal_count();
    if picked == Some(probe_pid) && steal_count > 0 {
        log_info!(target: "selftest", "KSELFTEST: work stealing ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: work stealing FAIL picked={:?} steals={}",
            picked,
            steal_count
        );
    }

    // ADR-0052 §1: earliest-deadline arming — a later arm on the same hart
    // must NOT clobber an earlier armed deadline; an earlier one must win.
    // (The clobber slipped windowd's 8.33ms pacer to the 10ms fallback tick.)
    if crate::trap::selftest_edt_probe(ctx.hal.timer()) {
        log_info!(target: "selftest", "KSELFTEST: edt arm ok");
    } else {
        log_error!(target: "selftest", "KSELFTEST: edt arm FAIL");
    }

    // ADR-0052 §3: an affinity-REJECTED steal parks the task back on its HOME
    // CPU's queue. Previously it parked on cpu0, where the affinity-blind
    // `schedule_next` would RUN it — background work stealing display-chain
    // time on the soft-RT hart.
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.selftest_reset_cpu(target);
    let park_pid = Pid::from_raw(0x7FFF_FF30);
    if !matches!(
        ctx.scheduler.selftest_enqueue_on_cpu(target, park_pid, QosClass::Normal),
        EnqueueOutcome::Enqueued
    ) {
        log_error!(target: "selftest", "KSELFTEST: steal park FAIL enqueue");
        return;
    }
    let stolen = ctx.scheduler.selftest_steal_refused(target);
    // Queue-level truth (schedule_on_cpu would steal cross-queue and mask a
    // wrong park): the task must sit in its HOME queue, and nowhere on boot's.
    let on_home = ctx.scheduler.selftest_queue_len(target, QosClass::Normal) == 1;
    let on_boot = ctx.scheduler.selftest_queue_len(boot, QosClass::Normal) != 0;
    if stolen.is_none() && on_home && !on_boot {
        log_info!(target: "selftest", "KSELFTEST: steal park ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: steal park FAIL stolen={:?} home={} boot={}",
            stolen,
            on_home,
            on_boot
        );
    }
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.selftest_reset_cpu(target);

    // Deterministic negative checks for steal policies:
    // 1) reject stealing more than one task per scheduling tick.
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.selftest_reset_cpu(target);
    if !matches!(
        ctx.scheduler.selftest_enqueue_on_cpu(target, Pid::from_raw(0x7FFF_FF10), QosClass::Normal),
        EnqueueOutcome::Enqueued
    ) {
        log_error!(target: "selftest", "KSELFTEST: test_reject_steal_above_bound FAIL enqueue-0");
        return;
    }
    if !matches!(
        ctx.scheduler.selftest_enqueue_on_cpu(target, Pid::from_raw(0x7FFF_FF11), QosClass::Normal),
        EnqueueOutcome::Enqueued
    ) {
        log_error!(target: "selftest", "KSELFTEST: test_reject_steal_above_bound FAIL enqueue-1");
        return;
    }
    let _ = ctx.scheduler.selftest_schedule_on_cpu(boot);
    if ctx.scheduler.selftest_queue_len(target, QosClass::Normal) == 1 {
        log_info!(target: "selftest", "KSELFTEST: test_reject_steal_above_bound ok");
    } else {
        log_error!(target: "selftest", "KSELFTEST: test_reject_steal_above_bound FAIL");
    }

    // 2) reject stealing higher-QoS tasks when current class is lower.
    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.selftest_reset_cpu(target);
    if !matches!(
        ctx.scheduler.selftest_enqueue_on_cpu(
            target,
            Pid::from_raw(0x7FFF_FF20),
            QosClass::Interactive,
        ),
        EnqueueOutcome::Enqueued
    ) {
        log_error!(target: "selftest", "KSELFTEST: test_reject_steal_higher_qos FAIL enqueue");
        return;
    }
    if ctx.scheduler.selftest_try_steal_for_class(boot, QosClass::Normal).is_none() {
        log_info!(target: "selftest", "KSELFTEST: test_reject_steal_higher_qos ok");
    } else {
        log_error!(target: "selftest", "KSELFTEST: test_reject_steal_higher_qos FAIL");
    }

    ctx.scheduler.selftest_reset_cpu(boot);
    ctx.scheduler.selftest_reset_cpu(target);
    if matches!(ctx.scheduler.enqueue(Pid::KERNEL, QosClass::Normal), EnqueueOutcome::Rejected(_)) {
        panic!("scheduler selftest bootstrap enqueue rejected");
    }
}
