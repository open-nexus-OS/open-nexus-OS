// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the running task as the trap exit sees it: whether a queued task goes ahead of it
//! now — the question a wake's trap exit asks before it returns to the interrupted task
//! (ADR-0096, since TASK-0363 P3 by the order of ADR-0101: a reservation with budget left, a
//! heavier earlier virtual deadline, an equal one past parity; asked only when a task came into
//! the order), the switch it leads to as one queue operation — and the running task's class set
//! in place.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `sched/run_queue_order_tests.rs`, `run_queue_tests.rs` (host) + QEMU lanes +
//!   the board's `KINIT: ready latency` lines
//! ADR: docs/adr/0101-the-fair-class-orders-by-virtual-deadline-a-reservation-runs-first.md

use super::*;

impl Scheduler {
    /// Whether a queued task goes ahead of the running task on this hart now: the wake that
    /// queued it preempts at the trap exit, so the display and input chain runs within interrupt
    /// latency, not within a tick. The running task is charged to now first (its virtual time,
    /// its budget). `false` when no task runs here (the idle loop rescans its queues itself).
    pub fn ready_before_current(&self) -> bool {
        let cpu = Self::current_cpu_index();
        // Asked at a trap's exit: only a task that came into the order since can go ahead
        // (`HartQueues::take_arrived`), and the meter is run for the question only then.
        with(cpu, HartQueues::take_arrived) == Some(true)
            && meter::metered(cpu, |q| q.ready_before_current()) == Some(true)
    }

    /// The running task gives way and the next runs, in one queue operation: one charge, one
    /// round of the hart's lock, the pick made on the queue the requeue left. `behind_all`: a
    /// yield (behind every task queued here); else a preemption (as it stands in the order).
    /// The next task — the same one when it is still first; `None` when the hart runs nothing or
    /// its queue is at its bound (the running task stays current). The caller has made the
    /// running task's saved frame complete: once queued, another hart may take it.
    pub fn give_way(&mut self, behind_all: bool) -> Option<TaskId> {
        Self::give_way_if(behind_all, false)
    }

    /// The syscall exit's switch (ADR-0096, ADR-0101): [`Self::give_way`] as a preemption, if a
    /// queued task goes ahead of the running one ([`Self::ready_before_current`]) — question and
    /// switch under one lock. `None`: the caller runs on.
    pub fn switch_if_ready(&mut self) -> Option<TaskId> {
        let cpu = Self::current_cpu_index();
        let arrived = with(cpu, HartQueues::take_arrived);
        (arrived == Some(true)).then(|| Self::give_way_if(false, true)).flatten()
    }

    /// The switch of a syscall that ran off the kernel lock (ADR-0102): if a queued task goes
    /// ahead of the running one, the running one is requeued as it stands in the order — without
    /// a pick, which is the hart's scheduler loop's. `true`: requeued; the caller made its saved
    /// frame complete before and touches it no more — `left` ran in the hold that queued it,
    /// before any hart can take it (where the caller's hart leaves its address space).
    /// `false`: it runs on (nothing goes ahead, or the queue is at its bound).
    pub fn preempt_if_ready(&mut self, left: impl FnOnce()) -> bool {
        let cpu = Self::current_cpu_index();
        with(cpu, HartQueues::take_arrived) == Some(true) && Self::give_up(cpu, false, left)
    }

    /// The `yield` of a syscall that ran off the kernel lock (TASK-0330 P4): the running task is
    /// queued behind every task waiting on this hart — without a pick, as `preempt_if_ready`,
    /// and with its `left`. `false`: it runs on (nothing waits here: the pick would hand it
    /// straight back; or the queue is at its bound).
    pub fn yield_if_waiting(&mut self, left: impl FnOnce()) -> bool {
        crate::liveness::bump();
        Self::give_up(Self::current_cpu_index(), true, left)
    }

    /// Requeues the running task of hart `cpu` if another goes ahead of it — any queued one for
    /// a yield (behind them all), else by the order; `left` runs in the hold that queued it.
    fn give_up(cpu: usize, yields: bool, left: impl FnOnce()) -> bool {
        let now = ready::now_ticks();
        meter::metered(cpu, |q| {
            let ahead = match yields {
                true => QosClass::DESCENDING.iter().any(|&class| q.len(class) > 0),
                false => q.ready_before_current(),
            };
            let Some(old) = q.current().filter(|_| ahead) else {
                return false;
            };
            let queued = if yields { q.yield_current(now) } else { q.requeue_current(now) };
            if queued {
                hand(old.id, Owner::Queued(cpu as u16));
                left();
            }
            queued
        }) == Some(true)
    }

    fn give_way_if(behind_all: bool, only_if_ready: bool) -> Option<TaskId> {
        crate::liveness::bump();
        let now = ready::now_ticks();
        let cur = Self::current_cpu_index();
        let picked = meter::metered(cur, |q| {
            if only_if_ready && !q.ready_before_current() {
                return None;
            }
            let old = q.current()?;
            let queued = if behind_all { q.yield_current(now) } else { q.requeue_current(now) };
            if !queued {
                return None;
            }
            hand(old.id, Owner::Queued(cur as u16));
            let new = q.pick()?;
            hand(new.id, Owner::Running(cur as u16));
            Some(new)
        })
        .flatten()?;
        ready::dispatched(&picked);
        Some(Pid::from_raw(picked.id))
    }

    /// Updates the currently running task QoS (and its weight) in place.
    pub fn set_current_qos(&mut self, qos: QosClass) -> bool {
        with(Self::current_cpu_index(), |q| {
            let weight = q.current().map_or(1, |task| weight_for(task.id, qos));
            q.set_current_qos(qos, weight)
        }) == Some(true)
    }
}
