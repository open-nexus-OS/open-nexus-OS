// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the run queues of ONE hart and the rule a thief follows across harts (TASK-0363
//! P2 S1) — the scheduler's pure core. Storage: one bounded queue per QoS class and the task the
//! hart runs. Order (TASK-0363 P3, RFC-0120; `run_queue_order.rs`): a reservation with budget
//! left runs first, earliest deadline first; everything else shares the hart fairly by weight —
//! earliest eligible virtual deadline. Fixed storage: a queue operation never allocates, so a
//! hart's queues sit behind a lock that is a leaf — nothing is taken while it is held but, by a
//! try-lock, another hart's queues for a steal's park.
//! NOT target-gated (like `ready_stats`, `waitset`): the hart is a parameter, never read from
//! `tp`, and the harts of a machine are the [`Harts`] the caller hands in — the kernel's
//! locked table (`sched::Scheduler`) or a test's cells — so every rule runs on the host.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `run_queue_tests.rs` and `run_queue_order_tests.rs` (host): the order, the
//!   weights' ratio, the reservations' budget, the bounds and every `test_reject_*`, the wrapped
//!   ring, set-qos, purge, steal by class and affinity, the park, the meter
//! INVARIANTS: a queue holds at most [`CLASS_CAPACITY`] tasks and rejects the next one; no
//!   operation allocates; a task a thief may not run is never lost — it is moved to its home
//!   hart's queue or left where it is; a hart's queues are never waited for while another
//!   hart's are held; a task's virtual time is its hart's — it crosses harts as its lag
//! ADR: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md

use crate::ready_stats::{behind, Behind};

#[path = "run_queue_order.rs"]
pub mod order;
pub use order::weight_of;

/// Quality-of-service hints used for prioritisation.
///
/// **ABI STABILITY (TASK-0013)**: This enum's discriminant values are part of the stable
/// userspace ABI once `sys_task_set_qos(qos: u8)` is exposed. Reordering or renumbering
/// variants is a breaking change. New variants must be appended.
///
/// Current discriminants (repr(u8)):
/// - Idle = 0
/// - Normal = 1
/// - Interactive = 2
/// - PerfBurst = 3
///
/// Since TASK-0363 P3 a class is a weight in the fair class (`weight_of`), not a priority: a
/// guarantee is a reservation of a time right (RFC-0120).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QosClass {
    Idle = 0,
    Normal = 1,
    Interactive = 2,
    PerfBurst = 3,
}

impl QosClass {
    /// Every class, the heaviest in front (a tie of the order goes to it).
    pub const DESCENDING: [Self; CLASSES] =
        [Self::PerfBurst, Self::Interactive, Self::Normal, Self::Idle];

    /// Decodes a stable QoS wire value into a typed class.
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Normal),
            2 => Some(Self::Interactive),
            3 => Some(Self::PerfBurst),
            _ => None,
        }
    }
}

/// QoS classes, one queue each.
pub const CLASSES: usize = 4;
/// Tasks one class's queue of one hart holds (the v1b hardening contract: an explicit bound,
/// a deterministic reject past it).
pub const CLASS_CAPACITY: usize = 64;

/// A task's reservation as its hart schedules it (RFC-0120): this period's deadline and what is
/// left of its budget, the period and the full budget — in the caller's clock. The kernel's
/// time-right table holds the same numbers for admission and its books.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Res {
    pub deadline: u64,
    pub left: u64,
    pub period: u64,
    pub budget: u64,
}

impl Res {
    /// Whether it runs from its reservation: budget left this period.
    pub const fn runs(&self) -> bool {
        self.left > 0
    }

    /// At or after its deadline: the next period after `now`, a full budget. Returns the periods
    /// that ended with budget unspent — for a task that waited in a queue all the while, the
    /// periods it missed.
    pub fn refresh(&mut self, now: u64) -> u64 {
        if now < self.deadline || self.period == 0 {
            return 0;
        }
        let periods = (now - self.deadline) / self.period + 1;
        let unspent = periods - u64::from(self.left == 0);
        self.deadline = self.deadline.saturating_add(periods.saturating_mul(self.period));
        self.left = self.budget;
        unspent
    }
}

/// A task in a run queue, or the one its hart runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    /// The task's id (the kernel's `Pid`, raw).
    pub id: u32,
    pub qos: QosClass,
    /// The tick it entered a run queue at and what its hart ran then (`ready_stats`).
    pub ready_at: u64,
    pub behind: Behind,
    /// Its share of the fair class (`weight_of`).
    pub weight: u32,
    /// Its virtual time on this hart: the time it ran, scaled down by its weight.
    pub vt: u64,
    /// Its lag — the hart's virtual time minus its own (positive: owed) — when it arrives on a
    /// hart or as it left one: how its virtual time crosses a block, a steal, a park.
    pub lag: i64,
    /// Its reservation, if it holds one.
    pub res: Option<Res>,
}

impl Task {
    const NONE: Self = Self {
        id: 0,
        qos: QosClass::Idle,
        ready_at: 0,
        behind: Behind::Idle,
        weight: 1,
        vt: 0,
        lag: 0,
        res: None,
    };

    /// Task `id` of class `qos` at the default share, owed nothing, without a reservation.
    pub const fn new(id: u32, qos: QosClass) -> Self {
        Self { id, qos, weight: weight_of(qos, 100), ..Self::NONE }
    }
}

/// The queue of the class is at its bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

/// A bounded FIFO of tasks in fixed storage.
struct Ring {
    slots: [Task; CLASS_CAPACITY],
    head: usize,
    len: usize,
}

impl Ring {
    const fn new() -> Self {
        Self { slots: [Task::NONE; CLASS_CAPACITY], head: 0, len: 0 }
    }

    /// The slot of the `i`-th task from the front.
    const fn slot(&self, i: usize) -> usize {
        (self.head + i) % CLASS_CAPACITY
    }

    fn get(&self, i: usize) -> Option<Task> {
        self.peek(i).copied()
    }

    /// The `i`-th task in place: the order's walks read it where it lies.
    fn peek(&self, i: usize) -> Option<&Task> {
        (i < self.len).then(|| &self.slots[self.slot(i)])
    }

    fn get_mut(&mut self, i: usize) -> Option<&mut Task> {
        let at = self.slot(i);
        (i < self.len).then(|| &mut self.slots[at])
    }

    fn push_back(&mut self, task: Task) -> Result<(), Full> {
        if self.len == CLASS_CAPACITY {
            return Err(Full);
        }
        let at = self.slot(self.len);
        self.slots[at] = task;
        self.len += 1;
        Ok(())
    }

    fn push_front(&mut self, task: Task) -> Result<(), Full> {
        if self.len == CLASS_CAPACITY {
            return Err(Full);
        }
        self.head = (self.head + CLASS_CAPACITY - 1) % CLASS_CAPACITY;
        self.slots[self.head] = task;
        self.len += 1;
        Ok(())
    }

    fn position(&self, id: u32) -> Option<usize> {
        (0..self.len).find(|&i| self.slots[self.slot(i)].id == id)
    }

    /// Takes the `i`-th task out; the tasks behind it keep their order.
    fn remove(&mut self, i: usize) -> Option<Task> {
        let task = self.get(i)?;
        if i == 0 {
            self.head = self.slot(1);
        } else {
            for k in i..self.len - 1 {
                self.slots[self.slot(k)] = self.slots[self.slot(k + 1)];
            }
        }
        self.len -= 1;
        Some(task)
    }

    fn clear(&mut self) {
        (self.head, self.len) = (0, 0);
    }
}

/// One hart's run queues and the task it runs.
pub struct HartQueues {
    /// Indexed by the class's discriminant.
    queues: [Ring; CLASSES],
    current: Option<Task>,
    /// When the running task was last charged (the caller's clock) — the meter's mark, and the
    /// time the order replenishes reservations at.
    since: u64,
    /// The hart's virtual time when it last held no task (the base a lone arrival is placed at).
    vbase: u64,
    /// The time a fair task asks for at a time (its virtual deadline's distance at weight
    /// `WEIGHT_UNIT`), in the caller's clock.
    slice: u64,
    /// Periods a runnable holder's budget went unspent in, not yet taken (`take_missed`).
    missed: u64,
    /// A task came into the order since it was last asked (`take_arrived`) or picked.
    arrived: bool,
}

impl HartQueues {
    /// A hart with nothing queued and nothing running.
    pub const fn new() -> Self {
        Self {
            queues: [const { Ring::new() }; CLASSES],
            current: None,
            since: 0,
            vbase: 0,
            slice: order::DEFAULT_SLICE,
            missed: 0,
            arrived: false,
        }
    }

    /// Whether a task came into the order — a wake, a thief's park, a queued task's new class
    /// or reservation — since this was last asked or the hart last picked; asking takes the
    /// mark. The order is asked whether a queued task goes ahead of the running one only then
    /// (and at the running task's timer): asked at every syscall's end, two peers that make
    /// syscalls would trade the hart at each one, and every exit would pay the meter.
    pub fn take_arrived(&mut self) -> bool {
        core::mem::take(&mut self.arrived)
    }

    /// The time a fair task asks for at a time (the kernel's tick, in its clock).
    pub fn set_slice(&mut self, ticks: u64) {
        self.slice = ticks.max(1);
    }

    /// Queues `task` at time `now`: placed on this hart's virtual time by its lag, stamped with
    /// what the hart runs (`ready_stats`).
    pub fn push(&mut self, task: Task, now: u64) -> Result<(), Full> {
        let waits = behind(self.current.map(|t| t.qos as usize), task.qos as usize);
        let placed = self.place(task);
        self.arrived = true;
        self.queues[task.qos as usize].push_back(Task { ready_at: now, behind: waits, ..placed })
    }

    /// Queues task `id` of class `qos` at the default share (a test's and a selftest's task).
    pub fn push_back(&mut self, id: u32, qos: QosClass, now: u64) -> Result<(), Full> {
        self.push(Task::new(id, qos), now)
    }

    /// Puts `task`, arriving from another hart, in front of its class's tasks (a thief's park),
    /// placed by its lag.
    pub fn push_front(&mut self, task: Task) -> Result<(), Full> {
        let placed = self.place(task);
        self.arrived = true;
        self.queues[task.qos as usize].push_front(placed)
    }

    /// The task of class `qos` at the front of its queue.
    pub fn front(&self, qos: QosClass) -> Option<Task> {
        self.queues[qos as usize].get(0)
    }

    /// Takes the task at the front of class `qos`'s queue, its lag stamped — to leave this hart
    /// (a steal) or to run here.
    pub fn pop_front(&mut self, qos: QosClass) -> Option<Task> {
        let mut task = self.queues[qos as usize].get(0)?;
        task.lag = self.lag_of(&task);
        self.queues[qos as usize].remove(0);
        Some(task)
    }

    /// The task the hart runs.
    pub fn current(&self) -> Option<Task> {
        self.current
    }

    /// The hart runs `task` as it is (taken from its own queues), or nothing.
    pub fn set_current(&mut self, task: Option<Task>) {
        self.current = task;
    }

    /// The hart runs `task`, arriving from another hart (a steal): placed by its lag.
    pub fn adopt(&mut self, task: Task) {
        self.current = Some(self.place(task));
    }

    /// The running task leaves the hart without a requeue (it blocked, it exited), its lag
    /// stamped — what its next wake is placed by.
    pub fn take_current(&mut self) -> Option<Task> {
        let mut task = self.current?;
        task.lag = self.lag_of(&task);
        self.current = None;
        Some(task)
    }

    /// The running task gives the hart up (a `yield`): queued behind every task waiting here.
    /// A queue at its bound keeps it current — it is never dropped. `false`: nothing ran, or it
    /// stayed.
    pub fn yield_current(&mut self, now: u64) -> bool {
        let last = self.queued().map(|t| t.vt).max();
        if let (Some(task), Some(last)) = (self.current.as_mut(), last) {
            task.vt = task.vt.max(last);
        }
        self.requeue_current(now)
    }

    /// The running task is preempted (its tick, a wake ahead of it): queued with its virtual
    /// time as it is. A queue at its bound keeps it current. `false`: nothing ran, or it stayed.
    pub fn requeue_current(&mut self, now: u64) -> bool {
        let Some(task) = self.current else {
            return false;
        };
        let waits = behind(Some(task.qos as usize), task.qos as usize);
        let queued = self.queues[task.qos as usize]
            .push_back(Task { ready_at: now, behind: waits, ..task })
            .is_ok();
        if queued {
            self.current = None;
        }
        queued
    }

    /// Sets the running task's class and weight; `false`: the hart runs nothing.
    pub fn set_current_qos(&mut self, qos: QosClass, weight: u32) -> bool {
        self.current.as_mut().map(|task| (task.qos, task.weight) = (qos, weight.max(1))).is_some()
    }

    /// Sets the class and weight of task `id` if it is the one running here.
    pub fn set_qos_if_current(&mut self, id: u32, qos: QosClass, weight: u32) -> bool {
        self.current
            .as_mut()
            .filter(|task| task.id == id)
            .map(|task| (task.qos, task.weight) = (qos, weight.max(1)))
            .is_some()
    }

    /// Moves queued task `id` to the back of class `qos` with `weight`; its virtual time stays.
    /// `None`: not queued here. `Some(Err(Full))`: the class is at its bound and the task stays
    /// where it was.
    pub fn requeue(&mut self, id: u32, qos: QosClass, weight: u32) -> Option<Result<(), Full>> {
        let (from, at) = (0..CLASSES).find_map(|c| Some((c, self.queues[c].position(id)?)))?;
        if from != qos as usize && self.queues[qos as usize].len == CLASS_CAPACITY {
            return Some(Err(Full));
        }
        let task = self.queues[from].remove(at)?;
        self.arrived = true;
        Some(self.queues[qos as usize].push_back(Task { qos, weight: weight.max(1), ..task }))
    }

    /// Task `id` runs with reservation `res` from now (a bind, an unbind): its entry here —
    /// running or queued — takes it. `false`: not on this hart.
    pub fn set_res(&mut self, id: u32, res: Option<Res>) -> bool {
        let mut found = false;
        if let Some(task) = self.current.as_mut().filter(|t| t.id == id) {
            (task.res, found) = (res, true);
        }
        for ring in &mut self.queues {
            for i in 0..ring.len {
                if let Some(task) = ring.get_mut(i).filter(|t| t.id == id) {
                    (task.res, found) = (res, true);
                }
            }
        }
        self.arrived |= found;
        found
    }

    /// Takes task `id` off this hart's queues — unless it is the task this hart runs, which only
    /// its own hart may take off (TASK-0363 P2 S3: a kill or a freeze decides under this lock
    /// whether the task is queued here or running here, so a dispatch that takes no kernel lock
    /// never starts a task another hart is reaping).
    pub fn withdraw(&mut self, id: u32) -> Withdraw {
        if self.current.is_some_and(|task| task.id == id) {
            return Withdraw::Running;
        }
        let mut removed = false;
        for queue in &mut self.queues {
            while let Some(at) = queue.position(id) {
                queue.remove(at);
                removed = true;
            }
        }
        if removed {
            Withdraw::Removed
        } else {
            Withdraw::Absent
        }
    }

    /// Forgets task `id`: out of every queue, and no longer the running one.
    pub fn purge(&mut self, id: u32) {
        for queue in &mut self.queues {
            while let Some(at) = queue.position(id) {
                queue.remove(at);
            }
        }
        if self.current.is_some_and(|task| task.id == id) {
            self.current = None;
        }
    }

    /// Nothing queued, nothing running.
    pub fn reset(&mut self) {
        self.queues.iter_mut().for_each(Ring::clear);
        (self.current, self.arrived) = (None, false);
    }

    /// Tasks queued in class `qos`.
    pub fn len(&self, qos: QosClass) -> usize {
        self.queues[qos as usize].len
    }

    /// Every queued task, the heaviest class first, each class front to back.
    fn queued(&self) -> impl Iterator<Item = &Task> + '_ {
        QosClass::DESCENDING.into_iter().flat_map(move |class| {
            let ring = &self.queues[class as usize];
            (0..ring.len).filter_map(move |i| ring.peek(i))
        })
    }
}

impl Default for HartQueues {
    fn default() -> Self {
        Self::new()
    }
}

/// What [`HartQueues::withdraw`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withdraw {
    /// The hart runs it: it stays, its own hart takes it off (a pending order).
    Running,
    /// It was queued here and is not any more.
    Removed,
    /// Not on this hart.
    Absent,
}

/// Who holds a task's scheduling record — its saved frame (TASK-0363 P2 S2): nobody, a hart's
/// queue, or the hart that runs it. Changed only where the task's queue state changes, under that
/// hart's queue lock: an enqueue makes it `Queued(h)`, a pick or a steal `Running(h)`, a park
/// `Queued(home)`, a switch away without a requeue (a block, an exit) `Free`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Neither queued nor running: blocked, new, exited — its frame is the kernel lock's.
    Free,
    Queued(u16),
    Running(u16),
}

impl Owner {
    /// The record's word: the state in the high half, the hart in the low.
    pub const fn encode(self) -> u32 {
        match self {
            Self::Free => 0,
            Self::Queued(h) => (1 << 16) | h as u32,
            Self::Running(h) => (2 << 16) | h as u32,
        }
    }

    /// The state a record's word holds; `None` for a word no `encode` wrote.
    pub const fn decode(raw: u32) -> Option<Self> {
        let h = (raw & 0xffff) as u16;
        match raw >> 16 {
            0 if h == 0 => Some(Self::Free),
            1 => Some(Self::Queued(h)),
            2 => Some(Self::Running(h)),
            _ => None,
        }
    }

    /// Whether hart `hart` may read or write the frame: a free task's (no hart holds it), or the
    /// one this hart runs. A queued task's frame is its queue's hart's to take — written before
    /// the enqueue, read after the pick, by nobody in between.
    pub const fn may_touch(self, hart: u16) -> bool {
        match self {
            Self::Free => true,
            Self::Running(h) => h == hart,
            Self::Queued(_) => false,
        }
    }
}

/// The harts of a machine, each one's queues reached without waiting.
pub trait Harts {
    /// The machine's harts.
    fn count(&self) -> usize;
    /// Whether hart `cpu` schedules (a hart that is not online holds nothing to steal).
    fn online(&self, cpu: usize) -> bool;
    /// Runs `f` on hart `cpu`'s queues if they are free right now. `None`: no such hart, or
    /// its queues are held — a thief never waits for another hart.
    fn try_with<R>(&self, cpu: usize, f: impl FnOnce(&mut HartQueues) -> R) -> Option<R>;
}

/// What a steal found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stolen {
    /// The thief runs this task (its lag stamped: [`HartQueues::adopt`]); it left its queue.
    Taken(Task),
    /// The first task found may not run on the thief: moved to the front of its home hart's
    /// queue when that is another hart with room, left where it is otherwise. Never lost.
    Rejected,
}

/// One bounded steal for hart `thief`: the first task of class `ceiling` or below, highest
/// class first and harts in ascending order, in the queues of another online hart. The task is
/// looked at under its hart's queue lock: `allow(id)` says whether the thief may run it,
/// `home_of(id)` names the hart a refused task belongs on. `None`: nothing to steal.
pub fn steal(
    harts: &impl Harts,
    thief: usize,
    ceiling: QosClass,
    mut allow: impl FnMut(u32) -> bool,
    mut home_of: impl FnMut(u32) -> usize,
) -> Option<Stolen> {
    let count = harts.count();
    for class in QosClass::DESCENDING.into_iter().filter(|&c| c as u8 <= ceiling as u8) {
        for victim in (0..count).filter(|&v| v != thief && harts.online(v)) {
            let found = harts.try_with(victim, |queues| {
                let front = queues.front(class)?;
                if allow(front.id) {
                    return queues.pop_front(class).map(Stolen::Taken);
                }
                let home = home_of(front.id).min(count.saturating_sub(1));
                let mut leaving = front;
                leaving.lag = queues.lag_of(&front);
                let parked = home != victim
                    && harts.try_with(home, |h| h.push_front(leaving).is_ok()) == Some(true);
                if parked {
                    queues.queues[class as usize].remove(0);
                }
                Some(Stolen::Rejected)
            });
            if let Some(Some(stolen)) = found {
                return Some(stolen);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "run_queue_tests.rs"]
mod tests;
