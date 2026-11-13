// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the run queues of ONE hart and the rule a thief follows across harts (TASK-0363
//! P2 S1) — the scheduler's pure core. One bounded queue per QoS class, served highest class
//! first and round-robin inside a class, and the task the hart runs. Fixed storage: a queue
//! operation never allocates, so a hart's queues sit behind a lock that is a leaf — nothing is
//! taken while it is held but, by a try-lock, another hart's queues for a steal's park.
//! NOT target-gated (like `ready_stats`, `waitset`): the hart is a parameter, never read from
//! `tp`, and the harts of a machine are the [`Harts`] the caller hands in — the kernel's
//! locked table (`sched::Scheduler`) or a test's cells — so every rule runs on the host.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `run_queue_tests.rs` (host): class order, round-robin, the bounds and every
//!   `test_reject_*`, the wrapped ring, set-qos, purge, steal by class and affinity, the park
//! INVARIANTS: a queue holds at most [`CLASS_CAPACITY`] tasks and rejects the next one; no
//!   operation allocates; a task a thief may not run is never lost — it is moved to its home
//!   hart's queue or left where it is; a hart's queues are never waited for while another
//!   hart's are held
//! ADR: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md

use crate::ready_stats::{behind, Behind};

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
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QosClass {
    Idle = 0,
    Normal = 1,
    Interactive = 2,
    PerfBurst = 3,
}

impl QosClass {
    /// Every class, the one served first in front.
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

/// A task in a run queue, or the one its hart runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    /// The task's id (the kernel's `Pid`, raw).
    pub id: u32,
    pub qos: QosClass,
    /// The tick it entered a run queue at and what its hart ran then (`ready_stats`).
    pub ready_at: u64,
    pub behind: Behind,
}

impl Task {
    const NONE: Self = Self { id: 0, qos: QosClass::Idle, ready_at: 0, behind: Behind::Idle };
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
        (i < self.len).then(|| self.slots[self.slot(i)])
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

    fn pop_front(&mut self) -> Option<Task> {
        let task = self.get(0)?;
        self.head = self.slot(1);
        self.len -= 1;
        Some(task)
    }

    fn position(&self, id: u32) -> Option<usize> {
        (0..self.len).find(|&i| self.slots[self.slot(i)].id == id)
    }

    /// Takes the `i`-th task out; the tasks behind it keep their order.
    fn remove(&mut self, i: usize) -> Option<Task> {
        let task = self.get(i)?;
        for k in i..self.len - 1 {
            self.slots[self.slot(k)] = self.slots[self.slot(k + 1)];
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
}

impl HartQueues {
    /// A hart with nothing queued and nothing running.
    pub const fn new() -> Self {
        Self { queues: [const { Ring::new() }; CLASSES], current: None }
    }

    /// Queues task `id` behind its class's tasks at tick `now`.
    pub fn push_back(&mut self, id: u32, qos: QosClass, now: u64) -> Result<(), Full> {
        let waits = behind(self.current.map(|task| task.qos as usize), qos as usize);
        self.queues[qos as usize].push_back(Task { id, qos, ready_at: now, behind: waits })
    }

    /// Puts `task` in front of its class's tasks, as it is (a thief's park).
    pub fn push_front(&mut self, task: Task) -> Result<(), Full> {
        self.queues[task.qos as usize].push_front(task)
    }

    /// The task of class `qos` that is served next.
    pub fn front(&self, qos: QosClass) -> Option<Task> {
        self.queues[qos as usize].get(0)
    }

    /// Takes the task of class `qos` that is served next.
    pub fn pop_front(&mut self, qos: QosClass) -> Option<Task> {
        self.queues[qos as usize].pop_front()
    }

    /// The next task to run — the highest class's first — becomes the hart's current task;
    /// with nothing queued the hart runs nothing.
    pub fn pick(&mut self) -> Option<Task> {
        self.current = QosClass::DESCENDING.iter().find_map(|&class| self.pop_front(class));
        self.current
    }

    /// The task the hart runs.
    pub fn current(&self) -> Option<Task> {
        self.current
    }

    /// The hart runs `task` (a stolen one), or nothing.
    pub fn set_current(&mut self, task: Option<Task>) {
        self.current = task;
    }

    /// Queues the running task behind its peers (a yield, the end of its slice). A queue at
    /// its bound keeps it current — it is never dropped. `false`: nothing ran, or it stayed.
    pub fn yield_current(&mut self, now: u64) -> bool {
        let Some(task) = self.current else {
            return false;
        };
        let queued = self.push_back(task.id, task.qos, now).is_ok();
        if queued {
            self.current = None;
        }
        queued
    }

    /// Whether a task of a class strictly above the running task's waits here (ADR-0096).
    /// `false` when the hart runs nothing.
    pub fn ready_above_current(&self) -> bool {
        self.current.is_some_and(|current| {
            QosClass::DESCENDING
                .iter()
                .any(|&class| class as u8 > current.qos as u8 && self.len(class) != 0)
        })
    }

    /// Sets the running task's class; `false`: the hart runs nothing.
    pub fn set_current_qos(&mut self, qos: QosClass) -> bool {
        self.current.as_mut().map(|task| task.qos = qos).is_some()
    }

    /// Sets the class of task `id` if it is the one running here.
    pub fn set_qos_if_current(&mut self, id: u32, qos: QosClass) -> bool {
        self.current.as_mut().filter(|task| task.id == id).map(|task| task.qos = qos).is_some()
    }

    /// Moves queued task `id` to the back of class `qos`. `None`: not queued here.
    /// `Some(Err(Full))`: the class is at its bound and the task stays where it was.
    pub fn requeue(&mut self, id: u32, qos: QosClass) -> Option<Result<(), Full>> {
        let (from, at) = (0..CLASSES).find_map(|c| Some((c, self.queues[c].position(id)?)))?;
        if from != qos as usize && self.queues[qos as usize].len == CLASS_CAPACITY {
            return Some(Err(Full));
        }
        let task = self.queues[from].remove(at)?;
        Some(self.queues[qos as usize].push_back(Task { qos, ..task }))
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
        self.current = None;
    }

    /// Tasks queued in class `qos`.
    pub fn len(&self, qos: QosClass) -> usize {
        self.queues[qos as usize].len
    }
}

impl Default for HartQueues {
    fn default() -> Self {
        Self::new()
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
    /// The thief runs this task; it left its queue.
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
                let task = queues.front(class)?;
                if allow(task.id) {
                    queues.pop_front(class);
                    return Some(Stolen::Taken(task));
                }
                let home = home_of(task.id).min(count.saturating_sub(1));
                let parked = home != victim
                    && harts.try_with(home, |h| h.push_front(task).is_ok()) == Some(true);
                if parked {
                    queues.pop_front(class);
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
