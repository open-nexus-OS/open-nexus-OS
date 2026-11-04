// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host unit tests of the scheduler — QoS ordering, the deterministic timeslice, the
//! yield requeue, bounded work stealing and every `test_reject_*` of the queue bounds. Split
//! out of `sched/mod.rs` (structure gate: one responsibility per file).
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;

#[test]
fn qos_ordering() {
    let mut sched = Scheduler::new();
    assert!(matches!(sched.enqueue(Pid::from_raw(1), QosClass::Normal), EnqueueOutcome::Enqueued));
    assert!(matches!(
        sched.enqueue(Pid::from_raw(2), QosClass::Interactive),
        EnqueueOutcome::Enqueued
    ));
    assert!(matches!(
        sched.enqueue(Pid::from_raw(3), QosClass::PerfBurst),
        EnqueueOutcome::Enqueued
    ));
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(3)));
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(2)));
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
}

#[test]
fn a_wake_of_a_class_above_the_running_task_is_ready_above() {
    // ADR-0096: with a Normal task running, an Interactive wake is "above" — the trap exit
    // preempts for it; the next pick is the woken task, after which nothing is above.
    let mut sched = Scheduler::new();
    assert!(!sched.ready_above_current(), "nothing runs: nothing is above");
    assert!(matches!(sched.enqueue(Pid::from_raw(1), QosClass::Normal), EnqueueOutcome::Enqueued));
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
    assert!(!sched.ready_above_current());
    assert!(matches!(
        sched.enqueue(Pid::from_raw(4), QosClass::Interactive),
        EnqueueOutcome::Enqueued
    ));
    assert!(sched.ready_above_current());
    sched.yield_current();
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(4)));
    assert!(!sched.ready_above_current(), "the Normal task behind it is not above");
}

#[test]
fn test_reject_a_wake_of_the_same_or_a_lower_class_as_above() {
    // ADR-0096: a Normal or Idle wake never preempts a running Normal task — it waits its turn
    // in the round-robin, as before; only a strictly higher class is above.
    let mut sched = Scheduler::new();
    assert!(matches!(sched.enqueue(Pid::from_raw(1), QosClass::Normal), EnqueueOutcome::Enqueued));
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
    assert!(matches!(sched.enqueue(Pid::from_raw(2), QosClass::Normal), EnqueueOutcome::Enqueued));
    assert!(matches!(sched.enqueue(Pid::from_raw(3), QosClass::Idle), EnqueueOutcome::Enqueued));
    assert!(!sched.ready_above_current());
    // An Interactive task running: a second Interactive wake is its equal, PerfBurst is above.
    assert!(matches!(
        sched.enqueue(Pid::from_raw(5), QosClass::Interactive),
        EnqueueOutcome::Enqueued
    ));
    sched.yield_current();
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(5)));
    assert!(matches!(
        sched.enqueue(Pid::from_raw(6), QosClass::Interactive),
        EnqueueOutcome::Enqueued
    ));
    assert!(!sched.ready_above_current());
    assert!(matches!(
        sched.enqueue(Pid::from_raw(7), QosClass::PerfBurst),
        EnqueueOutcome::Enqueued
    ));
    assert!(sched.ready_above_current());
}

#[test]
fn deterministic_timeslice() {
    let sched = Scheduler::new();
    assert_eq!(sched.timeslice_ns(), crate::determinism::fixed_tick_ns());
}

#[test]
fn yield_requeues_current_task() {
    let mut sched = Scheduler::new();
    assert!(matches!(sched.enqueue(Pid::from_raw(1), QosClass::Normal), EnqueueOutcome::Enqueued));
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
    sched.yield_current();
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
}

#[test]
fn running_on_names_the_hart_of_a_dispatched_task() {
    // ADR-0082: an order for a task running on another hart is left as a pending mark; the
    // scheduler answers which hart that is, and `None` for a task that is only queued.
    let mut sched = Scheduler::new();
    assert!(matches!(sched.enqueue(Pid::from_raw(7), QosClass::Normal), EnqueueOutcome::Enqueued));
    assert_eq!(sched.running_on(Pid::from_raw(7)), None);
    assert_eq!(sched.schedule_next(), Some(Pid::from_raw(7)));
    assert_eq!(sched.running_on(Pid::from_raw(7)), Some(CpuId::from_raw(0)));
    sched.finish_current();
    assert_eq!(sched.running_on(Pid::from_raw(7)), None);
}

#[test]
fn bounded_single_task_work_steal() {
    let mut sched = Scheduler::new();
    let boot = CpuId::from_raw(0);
    let secondary = CpuId::from_raw(1);
    sched.selftest_reset_cpu(boot);
    sched.selftest_reset_cpu(secondary);
    assert!(matches!(
        sched.selftest_enqueue_on_cpu(secondary, Pid::from_raw(42), QosClass::Normal),
        EnqueueOutcome::Enqueued
    ));
    assert_eq!(sched.selftest_schedule_on_cpu(boot), Some(Pid::from_raw(42)));
    assert_eq!(sched.selftest_schedule_on_cpu(boot), None);
}

#[test]
fn test_reject_steal_above_bound() {
    let mut sched = Scheduler::new();
    let boot = CpuId::from_raw(0);
    let secondary = CpuId::from_raw(1);
    sched.selftest_reset_cpu(boot);
    sched.selftest_reset_cpu(secondary);
    assert!(matches!(
        sched.selftest_enqueue_on_cpu(secondary, Pid::from_raw(100), QosClass::Normal),
        EnqueueOutcome::Enqueued
    ));
    assert!(matches!(
        sched.selftest_enqueue_on_cpu(secondary, Pid::from_raw(101), QosClass::Normal),
        EnqueueOutcome::Enqueued
    ));

    assert!(sched.selftest_schedule_on_cpu(boot).is_some());
    assert_eq!(sched.selftest_cpus[secondary.as_index()].queues[1].len(), 1);
}

#[test]
fn test_reject_steal_higher_qos() {
    let mut sched = Scheduler::new();
    let boot = CpuId::from_raw(0);
    let secondary = CpuId::from_raw(1);
    sched.selftest_reset_cpu(boot);
    sched.selftest_reset_cpu(secondary);
    assert!(matches!(
        sched.selftest_enqueue_on_cpu(secondary, Pid::from_raw(200), QosClass::Interactive),
        EnqueueOutcome::Enqueued
    ));

    let rejected = sched.selftest_try_steal(boot.as_index(), QosClass::Normal);
    assert!(rejected.is_none());
}

#[test]
fn test_reject_enqueue_above_runtime_bound() {
    let mut sched = Scheduler::new();
    let qos = QosClass::Normal;
    let capacity = Scheduler::runtime_queue_capacity_for(qos).raw();
    for pid in 1..=(capacity as u32) {
        assert!(matches!(sched.try_enqueue(Pid::from_raw(pid), qos), EnqueueOutcome::Enqueued));
    }

    let rejected = sched.try_enqueue(Pid::from_raw((capacity as u32) + 1), qos);
    assert!(matches!(
        rejected,
        EnqueueOutcome::Rejected(EnqueueRejectReason::QueueFull { qos: QosClass::Normal, capacity: cap })
            if cap == capacity
    ));
}

#[test]
fn test_reject_enqueue_above_selftest_bound() {
    let mut sched = Scheduler::new();
    let cpu = CpuId::from_raw(1);
    sched.selftest_reset_cpu(cpu);

    let qos = QosClass::Normal;
    let capacity = Scheduler::selftest_queue_capacity_for(qos).raw();
    for pid in 1..=(capacity as u32) {
        assert!(matches!(
            sched.selftest_enqueue_on_cpu(cpu, Pid::from_raw(pid), qos),
            EnqueueOutcome::Enqueued
        ));
    }

    let rejected = sched.selftest_enqueue_on_cpu(cpu, Pid::from_raw((capacity as u32) + 1), qos);
    assert!(matches!(
        rejected,
        EnqueueOutcome::Rejected(EnqueueRejectReason::QueueFull { qos: QosClass::Normal, capacity: cap })
            if cap == capacity
    ));
}

#[test]
fn test_reject_set_task_qos_when_target_queue_full() {
    let mut sched = Scheduler::new();
    let target = Pid::from_raw(0x1234);
    assert!(matches!(sched.enqueue(target, QosClass::Normal), EnqueueOutcome::Enqueued));

    let cap = Scheduler::runtime_queue_capacity_for(QosClass::Interactive).raw();
    for i in 0..cap {
        let pid = Pid::from_raw(0x4000u32.wrapping_add(i as u32));
        assert!(matches!(sched.enqueue(pid, QosClass::Interactive), EnqueueOutcome::Enqueued));
    }

    let outcome = sched.set_task_qos(target, QosClass::Interactive);
    assert_eq!(outcome, SetQosOutcome::QueueFull);
    assert_eq!(sched.queues[2].len(), cap);
    assert!(sched.queues[1].iter().any(|task| task.id == target && task.qos == QosClass::Normal));
}
