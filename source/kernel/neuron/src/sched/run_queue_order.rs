// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the order of one hart (TASK-0363 P3, RFC-0120; ADR-0101) — which queued task runs
//! next, whether a wake goes ahead of the running task, and the charge of the time it ran.
//! Two lanes. A task whose reservation has budget left this period runs first, earliest
//! deadline first; the budget is charged as it runs and refreshed at the deadline. Everything
//! else — and a holder whose budget is spent, until its deadline — shares the hart fairly by
//! weight: its virtual time grows by the time it ran over its weight, the hart's virtual time is
//! the weighted mean of its tasks', and among the tasks not ahead of it (eligible) the earliest
//! virtual deadline runs (its virtual time plus a slice over its weight). A woken task is placed
//! by its lag, so a sleep neither earns nor costs share; a heavier wake goes ahead at once, an
//! equal one when the running task has run past the hart's virtual time.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `run_queue_order_tests.rs` (host): the ratio of two weights, the budget per
//!   period, the overrun counted, the lanes, the wake rule, the lag across harts, `test_reject_*`
//! ADR: docs/adr/0101-the-fair-class-orders-by-virtual-deadline-a-reservation-runs-first.md

use super::{HartQueues, QosClass, Task, CLASSES};

/// The weight of `Normal` at 100 shares: virtual time runs at the clock's pace for it.
pub const WEIGHT_UNIT: u32 = 1024;
/// The classes' weights at 100 shares (Idle, Normal, Interactive, PerfBurst).
const CLASS_WEIGHT: [u32; CLASSES] = [16, 1024, 4096, 8192];
/// A slice before the kernel names its tick (`HartQueues::set_slice`).
pub(super) const DEFAULT_SLICE: u64 = 1_000;

/// The weight of a task of class `qos` holding `shares` (100 = the default; TASK-0042's shares,
/// clamped to 1–1000): the class's weight scaled by its shares, at least 1.
pub const fn weight_of(qos: QosClass, shares: u16) -> u32 {
    let shares = if shares == 0 {
        1
    } else if shares > 1000 {
        1000
    } else {
        shares
    };
    let weight = CLASS_WEIGHT[qos as usize] as u64 * shares as u64 / 100;
    if weight == 0 {
        1
    } else {
        weight as u32
    }
}

/// What the running task was charged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charge {
    pub id: u32,
    /// The time it ran since it was last charged.
    pub ran: u64,
    /// Its reservation's budget ran out in that time (an overrun: it runs fair until its
    /// deadline).
    pub exhausted: bool,
}

/// How far from its hart's base a task's virtual time counts in the mean: far above any slice
/// (2^28 ticks for the lightest task at a 10 ms tick), and small enough that a hart's 256 tasks
/// at the heaviest weight (2^17) sum in 64 bits.
const SPAN: u64 = 1 << 36;

/// `ran` of the clock as virtual time at `weight`. 64-bit on purpose, like everything in the
/// order: this runs at every switch, and the kernel's target divides 128 bits in software (the
/// IPC round trip paid a third more for it — `KSELFTEST: ipc call budget`).
fn virtual_of(ran: u64, weight: u32) -> u64 {
    ran.saturating_mul(u64::from(WEIGHT_UNIT)) / u64::from(weight.max(1))
}

/// A weighted mean of virtual times around a base, summed in 64 bits.
#[derive(Clone, Copy, Default)]
struct Mean {
    sum: i64,
    weights: i64,
}

impl Mean {
    fn add(&mut self, task: &Task, base: u64) {
        let weight = i64::from(task.weight);
        self.sum = self.sum.saturating_add(weight.saturating_mul(diff(task.vt, base, SPAN)));
        self.weights += weight;
    }

    /// The mean on `base`; with no task, `base`.
    fn at(self, base: u64) -> u64 {
        if self.weights == 0 {
            return base;
        }
        base.saturating_add_signed(self.sum / self.weights)
    }
}

/// What one walk over a hart's queued tasks finds (`HartQueues::survey`).
#[derive(Default)]
struct Survey {
    /// The queued tasks' virtual time.
    mean: Mean,
    /// The reservation that runs next: `(deadline, class, index)`.
    reserved: Option<(u64, usize, usize)>,
}

/// `a - b`, at most `cap` either way.
fn diff(a: u64, b: u64, cap: u64) -> i64 {
    let cap = cap.min(i64::MAX as u64);
    if a >= b {
        (a - b).min(cap) as i64
    } else {
        -((b - a).min(cap) as i64)
    }
}

impl HartQueues {
    /// The time a task of `weight` asks for at a time, as virtual time.
    fn slice_of(&self, weight: u32) -> u64 {
        virtual_of(self.slice, weight)
    }

    /// The hart's virtual time: the weighted mean of its queued tasks' (and of the running one's
    /// with `running`); with none, its base.
    pub(super) fn vnow(&self, running: bool) -> u64 {
        let mut mean = Mean::default();
        self.queued().for_each(|task| mean.add(task, self.vbase));
        if let Some(task) = self.current.as_ref().filter(|_| running) {
            mean.add(task, self.vbase);
        }
        mean.at(self.vbase)
    }

    /// The lag of `task` on this hart: the hart's virtual time minus its own (positive: owed).
    pub(super) fn lag_of(&self, task: &Task) -> i64 {
        diff(self.vnow(true), task.vt, u64::MAX)
    }

    /// `task` arriving on this hart: its virtual time from its lag — bounded by one slice either
    /// way, so neither a long sleep nor a long run carries over more than one turn.
    pub(super) fn place(&self, task: Task) -> Task {
        let bound = self.slice_of(task.weight).min(i64::MAX as u64) as i64;
        let lag = task.lag.clamp(-bound, bound);
        let vt = self.vnow(true).saturating_add_signed(lag.saturating_neg());
        Task { vt, lag: 0, ..task }
    }

    /// Charges the running task the time since it was last charged, at `now`: its virtual time
    /// by its weight, its reservation's budget (refreshed past the deadline). The mark moves to
    /// `now` with or without a running task — so whatever the hart runs next starts there.
    pub fn charge(&mut self, now: u64) -> Option<Charge> {
        let since = core::mem::replace(&mut self.since, now);
        let ran = now.saturating_sub(since);
        let task = self.current.as_mut()?;
        task.vt = task.vt.saturating_add(virtual_of(ran, task.weight));
        let mut exhausted = false;
        if let Some(res) = task.res.as_mut() {
            exhausted = res.left > 0 && ran >= res.left;
            res.left = res.left.saturating_sub(ran);
            // Still short of its budget at its deadline: it got the hart too late for it.
            self.missed += u64::from(res.left > 0 && now >= res.deadline);
            res.refresh(now);
        }
        Some(Charge { id: task.id, ran, exhausted })
    }

    /// The periods in which a runnable holder did not get its budget (it waited in a queue, or
    /// ran too late for it) since they were last taken — the reservations' misses.
    pub fn take_missed(&mut self) -> u64 {
        core::mem::take(&mut self.missed)
    }

    /// One walk over the queued tasks at the mark, each read where it lies: a reservation past
    /// its deadline is refreshed (a period that ended with budget unspent is a miss — its holder
    /// was runnable all the while), the virtual times are summed, and the reservation that runs
    /// next is found — budget left, earliest deadline.
    fn survey(&mut self) -> Survey {
        let (now, base) = (self.since, self.vbase);
        let (mut found, mut missed) = (Survey::default(), 0);
        for class in QosClass::DESCENDING {
            let ring = &mut self.queues[class as usize];
            for i in 0..ring.len {
                let Some(task) = ring.get_mut(i) else { continue };
                found.mean.add(task, base);
                let Some(res) = task.res.as_mut() else { continue };
                missed += res.refresh(now);
                if res.runs()
                    && found.reserved.is_none_or(|(deadline, _, _)| res.deadline < deadline)
                {
                    found.reserved = Some((res.deadline, class as usize, i));
                }
            }
        }
        self.missed += missed;
        found
    }

    /// The queued task that runs next after a survey, at the hart's virtual time `now` (the
    /// survey's mean): its reservation, else the earliest virtual deadline among the tasks not
    /// ahead of `now` — `(class, index)`.
    fn next_after(&self, found: &Survey, now: u64) -> Option<(usize, usize)> {
        if let Some((_, class, i)) = found.reserved {
            return Some((class, i));
        }
        let mut fair: Option<(u64, usize, usize)> = None;
        for class in QosClass::DESCENDING {
            let ring = &self.queues[class as usize];
            for (i, task) in (0..ring.len).filter_map(|i| Some((i, ring.peek(i)?))) {
                if task.vt > now {
                    continue;
                }
                let deadline = task.vt.saturating_add(self.slice_of(task.weight));
                if fair.is_none_or(|(best, _, _)| deadline < best) {
                    fair = Some((deadline, class as usize, i));
                }
            }
        }
        fair.map(|(_, class, i)| (class, i))
    }

    /// The next task to run becomes the hart's current task — a reservation with budget left,
    /// earliest deadline first, else the earliest eligible virtual deadline; with nothing queued
    /// the hart runs nothing. The task that ran is not requeued here (its caller did, or it left).
    pub fn pick(&mut self) -> Option<Task> {
        self.arrived = false;
        let found = self.survey();
        self.vbase = found.mean.at(self.vbase);
        let next = self.next_after(&found, self.vbase);
        self.current = next.and_then(|(class, i)| self.queues[class].remove(i));
        self.current
    }

    /// Whether a queued task goes ahead of the running one now (a wake's trap exit): a
    /// reservation with budget left before a fair task or a later deadline; between fair tasks,
    /// an earlier virtual deadline that is heavier, or any earlier one once the running task has
    /// run past the hart's virtual time. `false` when the hart runs nothing (the idle loop picks
    /// itself). The running task is charged to the mark first by the caller.
    pub fn ready_before_current(&mut self) -> bool {
        let Some(current) = self.current else {
            return false;
        };
        let found = self.survey();
        let base = self.vbase;
        let next = self.next_after(&found, found.mean.at(base));
        let Some(next) = next.and_then(|(class, i)| self.queues[class].peek(i)) else {
            return false;
        };
        match (next.res.filter(|r| r.runs()), current.res.filter(|r| r.runs())) {
            (Some(next), Some(current)) => next.deadline < current.deadline,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => {
                let deadline = |t: &Task| t.vt.saturating_add(self.slice_of(t.weight));
                // The hart's virtual time with the running task: the survey's sum and its own.
                let mut with_current = found.mean;
                with_current.add(&current, base);
                deadline(next) < deadline(&current)
                    && (next.weight > current.weight || current.vt > with_current.at(base))
            }
        }
    }
}

#[cfg(test)]
#[path = "run_queue_order_tests.rs"]
mod tests;
