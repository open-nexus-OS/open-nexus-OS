// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host tests of one hart's order (TASK-0363 P3, RFC-0120, ADR-0101): two weights share
//! a hart in their ratio, a reservation gets its budget first every period and runs fair after
//! it (the overrun counted), the earlier deadline first, the wake rule, and a task's lag across
//! harts — with every `test_reject_*` of the order.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use std::collections::BTreeMap;

use super::super::Res;
use super::*;

const SLICE: u64 = 1_000;
const TICK: u64 = 100;

/// A hart that runs `tasks` as hogs (always runnable) for `steps` ticks of `TICK`: each tick
/// charges the running task, requeues it and picks. Returns each task's run time and the
/// overruns counted.
fn run_hogs(q: &mut HartQueues, now: &mut u64, steps: usize) -> (BTreeMap<u32, u64>, usize) {
    let (mut ran, mut overruns) = (BTreeMap::new(), 0);
    for _ in 0..steps {
        *now += TICK;
        if let Some(c) = q.charge(*now) {
            *ran.entry(c.id).or_insert(0) += c.ran;
            overruns += usize::from(c.exhausted);
        }
        q.requeue_current(*now);
        q.pick();
    }
    (ran, overruns)
}

fn hart() -> HartQueues {
    let mut q = HartQueues::new();
    q.set_slice(SLICE);
    q
}

fn task(id: u32, qos: QosClass, shares: u16) -> Task {
    Task { weight: weight_of(qos, shares), ..Task::new(id, qos) }
}

fn ratio(ran: &BTreeMap<u32, u64>, a: u32, b: u32) -> f64 {
    ran[&a] as f64 / ran[&b] as f64
}

#[test]
fn the_classes_weigh_in_order_and_shares_scale_them() {
    let w = |qos| weight_of(qos, 100);
    assert!(w(QosClass::Idle) < w(QosClass::Normal));
    assert!(w(QosClass::Normal) < w(QosClass::Interactive));
    assert!(w(QosClass::Interactive) < w(QosClass::PerfBurst));
    assert_eq!(weight_of(QosClass::Normal, 100), WEIGHT_UNIT);
    assert_eq!(weight_of(QosClass::Normal, 300), 3 * WEIGHT_UNIT);
}

#[test]
fn test_reject_a_weight_of_zero_or_past_the_share_bound() {
    assert_eq!(weight_of(QosClass::Idle, 0), weight_of(QosClass::Idle, 1).max(1));
    assert!(weight_of(QosClass::Idle, 1) >= 1, "never zero: the virtual time divides by it");
    assert_eq!(weight_of(QosClass::PerfBurst, u16::MAX), weight_of(QosClass::PerfBurst, 1000));
}

#[test]
fn two_weights_share_a_hart_in_their_ratio() {
    // The gate's number (`KSELFTEST: sched fair ok`): within 5 % of the weights' ratio.
    for (a, b, expect) in [
        (task(1, QosClass::Interactive, 100), task(2, QosClass::Normal, 100), 4.0),
        (task(1, QosClass::Normal, 300), task(2, QosClass::Normal, 100), 3.0),
        (task(1, QosClass::PerfBurst, 100), task(2, QosClass::Interactive, 100), 2.0),
    ] {
        let (mut q, mut now) = (hart(), 0);
        q.push(a, 0).unwrap();
        q.push(b, 0).unwrap();
        q.pick();
        let (ran, _) = run_hogs(&mut q, &mut now, 20_000);
        let got = ratio(&ran, 1, 2);
        assert!((got / expect - 1.0).abs() < 0.05, "ratio {got} for {expect}");
    }
}

#[test]
fn equal_weights_alternate_by_the_slice() {
    let (mut q, mut now) = (hart(), 0);
    for id in 1..=3 {
        q.push(task(id, QosClass::Normal, 100), 0).unwrap();
    }
    q.pick();
    let (ran, _) = run_hogs(&mut q, &mut now, 3_000);
    for id in 1..=3 {
        assert!((ran[&id] as i64 - 100_000).abs() <= SLICE as i64 * 2, "{ran:?}");
    }
}

fn reserved(id: u32, budget: u64, period: u64, now: u64) -> Task {
    let res = Res { deadline: now + period, left: budget, period, budget };
    Task { res: Some(res), ..task(id, QosClass::Normal, 100) }
}

#[test]
fn a_reservation_runs_its_budget_first_every_period_and_fair_after_it() {
    // A holder of 3 of every 10 (a hog too) against a fair hog of the same weight: it runs its
    // budget at the start of each period, then runs fair; every period's overrun is counted
    // once. Its reserved time counts against its fair share (ADR-0101): a reservation buys
    // when it runs, not more than the larger of its budget and its fair share.
    let (mut q, mut now) = (hart(), 0);
    q.push(reserved(1, 3_000, 10_000, 0), 0).unwrap();
    q.push(task(2, QosClass::Normal, 100), 0).unwrap();
    q.pick();
    let (mut holder, mut overruns) = (0, 0);
    for period in 0..100u64 {
        // The first 3 000 of each period are the holder's, whatever ran before.
        let (first, over) = run_hogs(&mut q, &mut now, 30);
        assert_eq!(first.get(&1).copied(), Some(3_000), "period {period}: {first:?}");
        let (rest, more) = run_hogs(&mut q, &mut now, 70);
        overruns += over + more;
        holder += 3_000 + rest.get(&1).copied().unwrap_or(0);
        assert!(rest.get(&2).copied().unwrap_or(0) > 0, "the fair task runs every period");
    }
    assert_eq!(overruns, 100, "the budget runs out once a period");
    // Same weight, so its fair share (half) is larger than its budget: it gets half, not more.
    let mean = holder / 100;
    assert!((4_500..=5_500).contains(&mean), "holder mean {mean} per period");
}

#[test]
fn a_holder_crowded_out_of_the_fair_class_still_gets_its_budget() {
    // Nine fair hogs of the same weight: its fair share (a tenth) is below its budget (3 of 10).
    let (mut q, mut now) = (hart(), 0);
    q.push(reserved(1, 3_000, 10_000, 0), 0).unwrap();
    for id in 2..=10 {
        q.push(task(id, QosClass::Normal, 100), 0).unwrap();
    }
    q.pick();
    let (ran, overruns) = run_hogs(&mut q, &mut now, 10_000);
    let holder = ran[&1] / 100;
    assert!((3_000..=3_300).contains(&holder), "holder {holder} per period");
    assert_eq!(overruns, 100);
}

#[test]
fn test_reject_a_fair_task_ahead_of_a_reservation_with_budget() {
    let mut q = hart();
    q.push(task(1, QosClass::PerfBurst, 1000), 0).unwrap();
    let holder = Task { qos: QosClass::Idle, weight: 1, ..reserved(2, 500, 10_000, 0) };
    q.push(holder, 0).unwrap();
    assert_eq!(q.pick().map(|t| t.id), Some(2), "the budget runs first, whatever the weights");
    // Running from its budget, nothing fair goes ahead of it.
    q.push(task(3, QosClass::PerfBurst, 1000), 0).unwrap();
    assert!(!q.ready_before_current());
}

#[test]
fn the_earlier_deadline_runs_first_and_a_wake_with_one_goes_ahead() {
    let mut q = hart();
    q.push(reserved(1, 1_000, 20_000, 0), 0).unwrap();
    q.push(reserved(2, 1_000, 5_000, 0), 0).unwrap();
    assert_eq!(q.pick().map(|t| t.id), Some(2));
    q.push(reserved(3, 1_000, 2_000, 0), 0).unwrap();
    assert!(q.ready_before_current(), "an earlier deadline goes ahead at its wake");
    q.push(task(4, QosClass::Normal, 100), 0).unwrap();
    let mut fair = hart();
    fair.push(task(5, QosClass::Normal, 100), 0).unwrap();
    fair.pick();
    fair.push(reserved(6, 1_000, 50_000, 0), 0).unwrap();
    assert!(fair.ready_before_current(), "a reservation with budget goes ahead of a fair task");
}

#[test]
fn test_reject_an_exhausted_reservation_ahead_of_a_fair_task() {
    let (mut q, mut now) = (hart(), 0);
    q.push(reserved(1, 300, 10_000, 0), 0).unwrap();
    q.pick();
    now += 400;
    let charged = q.charge(now).unwrap();
    assert!(charged.exhausted && charged.ran == 400, "{charged:?}");
    assert_eq!(q.current().and_then(|t| t.res).map(|r| r.left), Some(0));
    q.push(task(2, QosClass::Normal, 100), now).unwrap();
    q.requeue_current(now);
    // Spent until its deadline: fair, and behind the task that has not run.
    assert_eq!(q.pick().map(|t| t.id), Some(2));
    // At its deadline its budget is back, and it runs first again.
    q.charge(10_000);
    q.requeue_current(10_000);
    assert_eq!(q.pick().map(|t| t.id), Some(1));
}

#[test]
fn an_equal_wake_waits_until_the_running_task_passed_the_harts_virtual_time() {
    let (mut q, mut now) = (hart(), 0);
    q.push(task(1, QosClass::Normal, 100), 0).unwrap();
    q.push(task(2, QosClass::Normal, 100), 0).unwrap();
    q.pick();
    q.push(task(3, QosClass::Normal, 100), 0).unwrap();
    assert!(!q.ready_before_current(), "at parity the running task keeps the hart");
    now += SLICE / 2;
    q.charge(now);
    assert!(q.ready_before_current(), "past the hart's virtual time an equal wake goes ahead");
}

#[test]
fn the_order_is_asked_when_a_task_arrived_not_at_every_exit() {
    let mut q = hart();
    assert!(!q.take_arrived(), "nothing arrived on a new hart");
    q.push(task(1, QosClass::Normal, 100), 0).unwrap();
    q.push(task(2, QosClass::Normal, 100), 0).unwrap();
    q.pick();
    assert!(!q.take_arrived(), "a pick decided on everything that had arrived");
    q.push(task(3, QosClass::Normal, 100), 0).unwrap();
    assert!(q.take_arrived(), "a wake is asked about");
    assert!(!q.take_arrived(), "once");
    // The running task requeued is no arrival: two peers do not trade the hart at each exit.
    assert!(q.requeue_current(1));
    assert!(!q.take_arrived());
    q.pick();
    // A queued task raised to a heavier class, or bound a reservation, is asked about.
    assert_eq!(
        q.requeue(3, QosClass::Interactive, weight_of(QosClass::Interactive, 100)),
        Some(Ok(()))
    );
    assert!(q.take_arrived());
    assert!(q.set_res(3, Some(Res { deadline: 100, left: 10, period: 100, budget: 10 })));
    assert!(q.take_arrived());
}

#[test]
fn a_task_keeps_its_lag_across_harts_bounded_by_a_slice() {
    let (mut a, mut now) = (hart(), 0);
    a.push(task(1, QosClass::Normal, 100), 0).unwrap();
    a.push(task(2, QosClass::Normal, 100), 0).unwrap();
    a.pick();
    now += 300;
    a.charge(now);
    let left = a.take_current().unwrap();
    assert!(left.lag < 0, "it ran ahead of the hart: lag {}", left.lag);
    // Another hart, its virtual time far on: the task arrives behind by what it ran ahead.
    let mut b = far_on(50_000);
    b.push(left, now).unwrap();
    let placed = b.queued().find(|t| t.id == 1).unwrap();
    assert_eq!(i128::from(placed.vt) - 50_000, -i128::from(left.lag));
}

/// A hart whose virtual time is `vt`: one task ran that long and runs on.
fn far_on(vt: u64) -> HartQueues {
    let mut q = hart();
    q.push(task(9, QosClass::Normal, 100), 0).unwrap();
    q.pick();
    q.charge(vt);
    assert_eq!(q.vnow(true), vt);
    q
}

#[test]
fn test_reject_a_long_sleep_bringing_back_more_than_a_slice() {
    let mut q = far_on(80_000);
    // Owed a million: it arrives at most one slice before the hart's virtual time.
    let sleeper = Task { lag: 1_000_000, ..task(1, QosClass::Normal, 100) };
    q.push(sleeper, 0).unwrap();
    let placed = q.queued().find(|t| t.id == 1).unwrap();
    assert!(placed.vt + SLICE >= 80_000 - SLICE, "placed at {}", placed.vt);
    // And a hog that ran far ahead arrives at most one slice behind.
    let hog = Task { lag: -1_000_000, ..task(2, QosClass::Normal, 100) };
    q.push(hog, 0).unwrap();
    let placed = q.queued().find(|t| t.id == 2).unwrap();
    assert!(placed.vt <= q.vnow(false) + SLICE, "placed at {}", placed.vt);
}

#[test]
fn a_yield_goes_behind_every_waiting_task() {
    let (mut q, mut now) = (hart(), 0);
    q.push(task(1, QosClass::Interactive, 100), 0).unwrap();
    q.push(task(2, QosClass::Normal, 100), 0).unwrap();
    q.pick();
    now += 10;
    q.charge(now);
    assert!(q.yield_current(now));
    // Heavier, barely run — and still behind the Normal task, because it gave the hart up.
    assert_eq!(q.pick().map(|t| t.id), Some(2));
}

#[test]
fn a_runnable_holder_kept_off_the_hart_past_its_deadline_is_a_miss() {
    // Holder 1 runs; holder 2 (3 000 of every 10 000) waits in the queue while the hart is held
    // for 25 000 without a pick — a stretch nothing preempts. Two of its periods ended with its
    // budget unspent; the one that ran all the while missed nothing.
    let mut q = hart();
    q.push(reserved(1, 1_000, 5_000, 0), 0).unwrap();
    q.pick();
    q.push(reserved(2, 3_000, 10_000, 0), 0).unwrap();
    q.charge(25_000);
    q.requeue_current(25_000);
    assert_eq!(q.pick().map(|t| t.id), Some(2), "the earlier deadline of the new periods");
    assert_eq!(q.take_missed(), 2);
    assert_eq!(q.take_missed(), 0, "taken once");
}

#[test]
fn a_holder_that_got_the_hart_too_late_for_its_budget_is_a_miss() {
    // 3 000 to run and 2 000 to its deadline when the hart is its: short at the deadline.
    let mut q = hart();
    q.charge(8_000);
    q.push(reserved(1, 3_000, 10_000, 0), 8_000).unwrap();
    q.pick();
    q.charge(10_500);
    assert_eq!(q.take_missed(), 1);
    assert_eq!(q.current().and_then(|t| t.res).map(|r| r.left), Some(3_000), "a new period");
}

#[test]
fn test_reject_a_miss_counted_for_a_budget_spent_in_time() {
    // The hogs of the gate, a hundred periods: every budget is spent before its deadline, and a
    // spent budget waiting for its next period is no miss however long it waits.
    let (mut q, mut now) = (hart(), 0);
    q.push(reserved(1, 3_000, 10_000, 0), 0).unwrap();
    for id in 2..=10 {
        q.push(task(id, QosClass::Normal, 100), 0).unwrap();
    }
    q.pick();
    let (_, overruns) = run_hogs(&mut q, &mut now, 10_000);
    assert_eq!((overruns, q.take_missed()), (100, 0));
}
