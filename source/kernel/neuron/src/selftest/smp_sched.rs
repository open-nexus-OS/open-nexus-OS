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

pub(super) fn run_steal_selftests(ctx: &mut Context<'_>, boot: CpuId, target: CpuId) {
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
    let stolen =
        ctx.scheduler.steal_into_current(QosClass::PerfBurst, |_| false, |_| target.as_index());
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
