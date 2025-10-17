// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal scheduler used during NEURON bring-up.

extern crate alloc;

use alloc::collections::VecDeque;

// use crate::determinism; // not needed after inlined guarded access

/// Task identifier handed out by the scheduler.
pub type TaskId = u32;

/// Quality-of-service hints used for prioritisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QosClass {
    Idle,
    Normal,
    Interactive,
    PerfBurst,
}

/// Metadata maintained per task.
#[derive(Debug, Clone)]
struct Task {
    id: TaskId,
    qos: QosClass,
}

/// Round-robin scheduler with simple QoS based ordering.
pub struct Scheduler {
    queues: [VecDeque<Task>; 4],
    current: Option<Task>,
    timeslice_ns: u64,
}

impl Scheduler {
    /// Creates an empty scheduler.
    pub fn new() -> Self {
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "SCHED: new enter\n");
        }
        // Read deterministic timeslice guarded and fall back to constant if needed
        let ts = crate::determinism::fixed_tick_ns();
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "SCHED: timeslice={}\n", ts);
        }
        let s = Self {
            // Preallocate minimal capacity to avoid first enqueue allocation cost
            queues: [
                VecDeque::with_capacity(1),
                VecDeque::with_capacity(1),
                VecDeque::with_capacity(1),
                VecDeque::with_capacity(1),
            ],
            current: None,
            timeslice_ns: ts,
        };
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "SCHED: new exit\n");
        }
        s
    }

    /// Enqueues a task with the provided QoS class.
    pub fn enqueue(&mut self, id: TaskId, qos: QosClass) {
        let task = Task { id, qos };
        self.queue_for(qos).push_back(task);
    }

    /// Picks the next runnable task.
    pub fn schedule_next(&mut self) -> Option<TaskId> {
        crate::liveness::bump();
        for class in [QosClass::PerfBurst, QosClass::Interactive, QosClass::Normal, QosClass::Idle]
        {
            if let Some(task) = self.queue_for(class).pop_front() {
                self.current = Some(task.clone());
                return Some(task.id);
            }
        }
        self.current = None;
        None
    }

    /// Re-enqueue the currently running task (call on timeslice/yield).
    pub fn yield_current(&mut self) {
        if let Some(task) = self.current.take() {
            self.enqueue(task.id, task.qos);
        }
    }

    /// Marks the current task as finished without re-enqueuing it.
    pub fn finish_current(&mut self) {
        self.current = None;
    }

    fn queue_for(&mut self, qos: QosClass) -> &mut VecDeque<Task> {
        match qos {
            QosClass::Idle => &mut self.queues[0],
            QosClass::Normal => &mut self.queues[1],
            QosClass::Interactive => &mut self.queues[2],
            QosClass::PerfBurst => &mut self.queues[3],
        }
    }

    /// Returns the configured deterministic time slice for each task.
    pub fn timeslice_ns(&self) -> u64 {
        self.timeslice_ns
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qos_ordering() {
        let mut sched = Scheduler::new();
        sched.enqueue(1, QosClass::Normal);
        sched.enqueue(2, QosClass::Interactive);
        sched.enqueue(3, QosClass::PerfBurst);
        assert_eq!(sched.schedule_next(), Some(3));
        assert_eq!(sched.schedule_next(), Some(2));
        assert_eq!(sched.schedule_next(), Some(1));
    }

    #[test]
    fn deterministic_timeslice() {
        let sched = Scheduler::new();
        assert_eq!(sched.timeslice_ns(), crate::determinism::fixed_tick_ns());
    }

    #[test]
    fn yield_requeues_current_task() {
        let mut sched = Scheduler::new();
        sched.enqueue(1, QosClass::Normal);
        assert_eq!(sched.schedule_next(), Some(1));
        sched.yield_current();
        assert_eq!(sched.schedule_next(), Some(1));
    }
}
