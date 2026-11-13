// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host tests of the run-queue core (TASK-0363 P2 S1) — class order, round-robin, the
//! yield requeue, the bounds and every `test_reject_*`, the ring past its wrap, set-qos, purge,
//! and the steal across harts: by class, by affinity, the park on the home hart, and a hart
//! whose queues are held. The harts of a test machine are cells: a borrow that is taken is a
//! lock that is held.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use core::cell::RefCell;

const NORMAL: QosClass = QosClass::Normal;

fn push(q: &mut HartQueues, id: u32, qos: QosClass) -> Result<(), Full> {
    q.push_back(id, qos, 0)
}

fn pick(q: &mut HartQueues) -> Option<u32> {
    q.pick().map(|task| task.id)
}

/// A machine of harts whose queues are cells; `offline` harts hold nothing to steal.
struct Cells {
    harts: Vec<RefCell<HartQueues>>,
    offline: Vec<usize>,
}

impl Cells {
    fn new(harts: usize) -> Self {
        Self {
            harts: (0..harts).map(|_| RefCell::new(HartQueues::new())).collect(),
            offline: vec![],
        }
    }

    fn push(&self, cpu: usize, id: u32, qos: QosClass) {
        push(&mut self.harts[cpu].borrow_mut(), id, qos).expect("room");
    }

    fn len(&self, cpu: usize, qos: QosClass) -> usize {
        self.harts[cpu].borrow().len(qos)
    }
}

impl Harts for Cells {
    fn count(&self) -> usize {
        self.harts.len()
    }

    fn online(&self, cpu: usize) -> bool {
        !self.offline.contains(&cpu)
    }

    fn try_with<R>(&self, cpu: usize, f: impl FnOnce(&mut HartQueues) -> R) -> Option<R> {
        self.harts.get(cpu)?.try_borrow_mut().ok().map(|mut queues| f(&mut queues))
    }
}

fn taken(stolen: Option<Stolen>) -> Option<u32> {
    match stolen {
        Some(Stolen::Taken(task)) => Some(task.id),
        _ => None,
    }
}

#[test]
fn qos_ordering() {
    let mut q = HartQueues::new();
    push(&mut q, 1, NORMAL).unwrap();
    push(&mut q, 2, QosClass::Interactive).unwrap();
    push(&mut q, 3, QosClass::PerfBurst).unwrap();
    push(&mut q, 4, QosClass::Idle).unwrap();
    assert_eq!(
        [pick(&mut q), pick(&mut q), pick(&mut q), pick(&mut q)],
        [Some(3), Some(2), Some(1), Some(4)]
    );
    assert_eq!(pick(&mut q), None);
    assert_eq!(q.current(), None, "nothing queued: the hart runs nothing");
}

#[test]
fn test_reject_a_qos_wire_value_beyond_the_classes() {
    for class in QosClass::DESCENDING {
        assert_eq!(QosClass::from_u8(class as u8), Some(class), "the wire value is the class");
    }
    for raw in CLASSES as u8..=u8::MAX {
        assert_eq!(QosClass::from_u8(raw), None);
    }
}

#[test]
fn a_class_is_served_round_robin() {
    let mut q = HartQueues::new();
    for id in 1..=3 {
        push(&mut q, id, NORMAL).unwrap();
    }
    let mut order = Vec::new();
    for _ in 0..7 {
        order.push(pick(&mut q).unwrap());
        assert!(q.yield_current(0));
    }
    assert_eq!(order, [1, 2, 3, 1, 2, 3, 1]);
}

#[test]
fn a_wake_of_a_class_above_the_running_task_is_ready_above() {
    // ADR-0096: with a Normal task running, an Interactive wake is "above" — the trap exit
    // preempts for it; the next pick is the woken task, after which nothing is above.
    let mut q = HartQueues::new();
    assert!(!q.ready_above_current(), "nothing runs: nothing is above");
    push(&mut q, 1, NORMAL).unwrap();
    assert_eq!(pick(&mut q), Some(1));
    assert!(!q.ready_above_current());
    push(&mut q, 4, QosClass::Interactive).unwrap();
    assert!(q.ready_above_current());
    assert!(q.yield_current(0));
    assert_eq!(pick(&mut q), Some(4));
    assert!(!q.ready_above_current(), "the Normal task behind it is not above");
}

#[test]
fn test_reject_a_wake_of_the_same_or_a_lower_class_as_above() {
    // ADR-0096: a Normal or Idle wake never preempts a running Normal task — it waits its turn
    // in the round-robin, as before; only a strictly higher class is above.
    let mut q = HartQueues::new();
    push(&mut q, 1, NORMAL).unwrap();
    assert_eq!(pick(&mut q), Some(1));
    push(&mut q, 2, NORMAL).unwrap();
    push(&mut q, 3, QosClass::Idle).unwrap();
    assert!(!q.ready_above_current());
    // An Interactive task running: a second Interactive wake is its equal, PerfBurst is above.
    push(&mut q, 5, QosClass::Interactive).unwrap();
    assert!(q.yield_current(0));
    assert_eq!(pick(&mut q), Some(5));
    push(&mut q, 6, QosClass::Interactive).unwrap();
    assert!(!q.ready_above_current());
    push(&mut q, 7, QosClass::PerfBurst).unwrap();
    assert!(q.ready_above_current());
}

#[test]
fn yield_requeues_current_task() {
    let mut q = HartQueues::new();
    assert!(!q.yield_current(0), "nothing runs");
    push(&mut q, 1, NORMAL).unwrap();
    assert_eq!(pick(&mut q), Some(1));
    assert!(q.yield_current(0));
    assert_eq!(q.current(), None);
    assert_eq!(pick(&mut q), Some(1));
}

#[test]
fn a_task_is_stamped_with_its_tick_and_what_its_hart_ran() {
    use crate::ready_stats::Behind;
    let mut q = HartQueues::new();
    q.push_back(1, NORMAL, 100).unwrap();
    let first = q.pick().unwrap();
    assert_eq!((first.ready_at, first.behind), (100, Behind::Idle));
    q.push_back(2, QosClass::Interactive, 250).unwrap();
    q.push_back(3, QosClass::Idle, 260).unwrap();
    assert_eq!(q.front(QosClass::Interactive).unwrap().behind, Behind::Lower);
    assert_eq!(q.front(QosClass::Idle).unwrap().behind, Behind::Peer);
    assert_eq!(q.front(QosClass::Interactive).unwrap().ready_at, 250);
}

#[test]
fn test_reject_enqueue_above_the_bound() {
    let mut q = HartQueues::new();
    for id in 1..=CLASS_CAPACITY as u32 {
        push(&mut q, id, NORMAL).unwrap();
    }
    assert_eq!(push(&mut q, 9_999, NORMAL), Err(Full));
    assert_eq!(q.len(NORMAL), CLASS_CAPACITY);
    // A class's bound is its own: another class still takes a task.
    push(&mut q, 9_999, QosClass::Idle).unwrap();
    // The order held: the first in is the first out.
    assert_eq!(pick(&mut q), Some(1));
}

#[test]
fn test_reject_a_yield_into_a_full_class_drops_nothing() {
    let mut q = HartQueues::new();
    push(&mut q, 500, NORMAL).unwrap();
    assert_eq!(pick(&mut q), Some(500));
    for id in 1..=CLASS_CAPACITY as u32 {
        push(&mut q, id, NORMAL).unwrap();
    }
    assert!(!q.yield_current(0), "the class is at its bound");
    assert_eq!(q.current().map(|task| task.id), Some(500), "it stays the running task");
}

#[test]
fn the_ring_keeps_its_order_past_its_wrap() {
    let mut q = HartQueues::new();
    let mut next = 0u32;
    let mut expect = 0u32;
    // Fill, drain half, fill again — many times round the fixed storage.
    for round in 0..40 {
        while q.len(NORMAL) < CLASS_CAPACITY - (round % 3) {
            push(&mut q, next, NORMAL).unwrap();
            next += 1;
        }
        for _ in 0..CLASS_CAPACITY / 2 + round % 5 {
            assert_eq!(q.pop_front(NORMAL).map(|task| task.id), Some(expect));
            expect += 1;
        }
    }
    assert!(next > 10 * CLASS_CAPACITY as u32);
}

#[test]
fn set_qos_moves_a_queued_task_to_the_back_of_its_new_class() {
    let mut q = HartQueues::new();
    for id in [1, 2, 3] {
        push(&mut q, id, NORMAL).unwrap();
    }
    push(&mut q, 9, QosClass::Interactive).unwrap();
    assert_eq!(q.requeue(2, QosClass::Interactive), Some(Ok(())));
    assert_eq!(q.requeue(77, QosClass::Interactive), None, "not queued here");
    assert_eq!(
        [pick(&mut q), pick(&mut q), pick(&mut q), pick(&mut q)],
        [Some(9), Some(2), Some(1), Some(3)]
    );
    assert_eq!(q.current().map(|task| task.qos), Some(NORMAL));
    // The running task's class is set in place.
    assert!(q.set_qos_if_current(3, QosClass::PerfBurst));
    assert!(!q.set_qos_if_current(1, QosClass::PerfBurst), "not the running task");
    assert_eq!(q.current().map(|task| task.qos), Some(QosClass::PerfBurst));
    assert!(q.set_current_qos(QosClass::Idle));
    q.set_current(None);
    assert!(!q.set_current_qos(QosClass::Idle), "the hart runs nothing");
}

#[test]
fn test_reject_set_task_qos_when_target_queue_full() {
    let mut q = HartQueues::new();
    push(&mut q, 0x1234, NORMAL).unwrap();
    push(&mut q, 0x1235, NORMAL).unwrap();
    for i in 0..CLASS_CAPACITY as u32 {
        push(&mut q, 0x4000 + i, QosClass::Interactive).unwrap();
    }
    assert_eq!(q.requeue(0x1234, QosClass::Interactive), Some(Err(Full)));
    assert_eq!(q.len(QosClass::Interactive), CLASS_CAPACITY);
    // It kept its class AND its place: still in front of the task queued after it.
    let front = q.front(NORMAL).unwrap();
    assert_eq!((front.id, front.qos), (0x1234, NORMAL));
    // Within its own class a full queue is no obstacle: it goes to the back.
    assert_eq!(q.requeue(0x4000, QosClass::Interactive), Some(Ok(())));
    assert_eq!(q.front(QosClass::Interactive).map(|task| task.id), Some(0x4001));
}

#[test]
fn purge_forgets_a_task_queued_or_running() {
    let mut q = HartQueues::new();
    for id in [1, 2, 3, 2] {
        push(&mut q, id, NORMAL).unwrap();
    }
    push(&mut q, 2, QosClass::Idle).unwrap();
    q.purge(2);
    assert_eq!((q.len(NORMAL), q.len(QosClass::Idle)), (2, 0));
    assert_eq!(pick(&mut q), Some(1));
    q.purge(1);
    assert_eq!(q.current(), None);
    assert_eq!(pick(&mut q), Some(3));
    q.reset();
    assert_eq!((q.current(), q.len(NORMAL)), (None, 0));
}

#[test]
fn bounded_single_task_work_steal() {
    let m = Cells::new(2);
    m.push(1, 42, NORMAL);
    m.push(1, 43, NORMAL);
    assert_eq!(taken(steal(&m, 0, QosClass::PerfBurst, |_| true, |_| 1)), Some(42));
    assert_eq!(m.len(1, NORMAL), 1, "one task a steal");
    assert_eq!(taken(steal(&m, 0, QosClass::PerfBurst, |_| true, |_| 1)), Some(43));
    assert_eq!(steal(&m, 0, QosClass::PerfBurst, |_| true, |_| 1), None);
}

#[test]
fn a_steal_takes_the_highest_class_first_and_harts_in_order() {
    let m = Cells::new(4);
    m.push(1, 10, NORMAL);
    m.push(2, 20, QosClass::Interactive);
    m.push(3, 30, QosClass::Interactive);
    m.push(0, 99, QosClass::PerfBurst); // the thief's own: never stolen
    let order: Vec<_> =
        (0..4).map(|_| taken(steal(&m, 0, QosClass::PerfBurst, |_| true, |_| 0))).collect();
    assert_eq!(order, [Some(20), Some(30), Some(10), None]);
    assert_eq!(m.len(0, QosClass::PerfBurst), 1);
}

#[test]
fn test_reject_steal_higher_qos() {
    let m = Cells::new(2);
    m.push(1, 200, QosClass::Interactive);
    assert_eq!(steal(&m, 0, NORMAL, |_| true, |_| 1), None);
    assert_eq!(m.len(1, QosClass::Interactive), 1);
}

#[test]
fn test_reject_steal_from_a_hart_that_is_not_online() {
    let mut m = Cells::new(3);
    m.offline.push(1);
    m.push(1, 7, NORMAL);
    m.push(2, 8, NORMAL);
    assert_eq!(taken(steal(&m, 0, NORMAL, |_| true, |_| 0)), Some(8));
    assert_eq!(steal(&m, 0, NORMAL, |_| true, |_| 0), None);
    assert_eq!(m.len(1, NORMAL), 1);
}

#[test]
fn test_reject_steal_against_affinity() {
    // The task's home is the hart it is queued on: refused, it stays there, in its place.
    let m = Cells::new(2);
    m.push(1, 5, NORMAL);
    m.push(1, 6, NORMAL);
    assert_eq!(steal(&m, 0, QosClass::PerfBurst, |_| false, |_| 1), Some(Stolen::Rejected));
    assert_eq!((m.len(0, NORMAL), m.len(1, NORMAL)), (0, 2));
    assert_eq!(m.harts[1].borrow().front(NORMAL).map(|task| task.id), Some(5));
}

#[test]
fn a_refused_task_parks_at_the_front_of_its_home_hart() {
    // ADR-0052: queued on hart 1, at home on hart 2, refused by the thief on hart 0 — it is
    // moved home, in front, and never onto the thief (whose pick has no affinity check).
    let m = Cells::new(3);
    m.push(2, 70, NORMAL);
    m.push(1, 71, NORMAL);
    assert_eq!(steal(&m, 0, QosClass::PerfBurst, |id| id != 71, |_| 2), Some(Stolen::Rejected));
    assert_eq!((m.len(0, NORMAL), m.len(1, NORMAL), m.len(2, NORMAL)), (0, 0, 2));
    assert_eq!(m.harts[2].borrow().front(NORMAL).map(|task| task.id), Some(71));
    // A home beyond the machine is its last hart.
    m.push(1, 72, NORMAL);
    assert_eq!(steal(&m, 0, QosClass::PerfBurst, |id| id != 72, |_| 99), Some(Stolen::Rejected));
    assert_eq!(m.harts[2].borrow().front(NORMAL).map(|task| task.id), Some(72));
}

#[test]
fn test_reject_a_park_that_would_lose_the_task() {
    // The home hart's class is at its bound: the refused task stays where it was queued.
    let m = Cells::new(3);
    for id in 0..CLASS_CAPACITY as u32 {
        m.push(2, 1_000 + id, NORMAL);
    }
    m.push(1, 71, NORMAL);
    assert_eq!(steal(&m, 0, QosClass::PerfBurst, |_| false, |_| 2), Some(Stolen::Rejected));
    assert_eq!((m.len(1, NORMAL), m.len(2, NORMAL)), (1, CLASS_CAPACITY));
    // The home hart's queues are held (another hart is in them): it stays, too.
    let m = Cells::new(3);
    m.push(1, 71, NORMAL);
    let held = m.harts[2].borrow_mut();
    assert_eq!(steal(&m, 0, QosClass::PerfBurst, |_| false, |_| 2), Some(Stolen::Rejected));
    drop(held);
    assert_eq!((m.len(1, NORMAL), m.len(2, NORMAL)), (1, 0));
}

#[test]
fn a_thief_never_waits_for_a_hart_whose_queues_are_held() {
    let m = Cells::new(3);
    m.push(1, 1, NORMAL);
    m.push(2, 2, NORMAL);
    let held = m.harts[1].borrow_mut();
    assert_eq!(taken(steal(&m, 0, NORMAL, |_| true, |_| 0)), Some(2), "the free hart's task");
    assert_eq!(steal(&m, 0, NORMAL, |_| true, |_| 0), None, "the held hart is passed over");
    drop(held);
    assert_eq!(taken(steal(&m, 0, NORMAL, |_| true, |_| 0)), Some(1));
}

/// Random pushes, picks, yields, requeues, purges and steals over four harts: no task is ever
/// in two places or lost — what went in is exactly what the queues and the running slots hold.
#[test]
fn no_task_is_lost_or_doubled_over_random_steps() {
    let m = Cells::new(4);
    let mut seed = 0x2545_f491u32;
    let mut rand = |below: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % below
    };
    let classes = [QosClass::Idle, NORMAL, QosClass::Interactive, QosClass::PerfBurst];
    let mut live: Vec<u32> = Vec::new();
    let mut next = 1u32;
    for _ in 0..20_000 {
        let cpu = rand(4) as usize;
        match rand(7) {
            0 | 1 => {
                let qos = classes[rand(4) as usize];
                if push(&mut m.harts[cpu].borrow_mut(), next, qos).is_ok() {
                    live.push(next);
                }
                next += 1;
            }
            2 => {
                // A pick replaces the running task: the one that ran has finished.
                let before = m.harts[cpu].borrow().current();
                if let Some(done) = before {
                    live.retain(|&id| id != done.id);
                }
                m.harts[cpu].borrow_mut().pick();
            }
            3 => {
                m.harts[cpu].borrow_mut().yield_current(0);
            }
            4 if !live.is_empty() => {
                let id = live[rand(live.len() as u32) as usize];
                let qos = classes[rand(4) as usize];
                let _ = m.harts[cpu].borrow_mut().requeue(id, qos);
            }
            5 if !live.is_empty() => {
                let id = live.swap_remove(rand(live.len() as u32) as usize);
                m.harts.iter().for_each(|h| h.borrow_mut().purge(id));
            }
            _ => {
                let home = rand(4) as usize;
                let stolen = steal(&m, cpu, classes[rand(4) as usize], |id| id % 3 != 0, |_| home);
                if let Some(Stolen::Taken(task)) = stolen {
                    // The thief runs it in place of what it ran.
                    let mut queues = m.harts[cpu].borrow_mut();
                    if let Some(done) = queues.current() {
                        live.retain(|&id| id != done.id);
                    }
                    queues.set_current(Some(task));
                }
            }
        }
        let mut held: Vec<u32> = Vec::new();
        for hart in &m.harts {
            let mut queues = hart.borrow_mut();
            held.extend(queues.current().map(|task| task.id));
            for class in classes {
                let n = queues.len(class);
                for _ in 0..n {
                    let task = queues.pop_front(class).unwrap();
                    assert_eq!(task.qos, class, "a task sits in its class's queue");
                    held.push(task.id);
                    queues.queues[class as usize].push_back(task).unwrap();
                }
            }
        }
        held.sort_unstable();
        let mut expect = live.clone();
        expect.sort_unstable();
        assert_eq!(held, expect);
    }
    assert!(next > 5_000);
}
