// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal scheduler used during NEURON bring-up.

use alloc::collections::VecDeque;

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
#[derive(Default)]
pub struct Scheduler {
    queues: [VecDeque<Task>; 4],
    current: Option<Task>,
}

impl Scheduler {
    /// Creates an empty scheduler.
    pub fn new() -> Self {
        Self {
            queues: Default::default(),
            current: None,
        }
    }

    /// Enqueues a task with the provided QoS class.
    pub fn enqueue(&mut self, id: TaskId, qos: QosClass) {
        let task = Task { id, qos };
        self.queue_for(qos).push_back(task);
    }

    /// Picks the next runnable task.
    pub fn schedule_next(&mut self) -> Option<TaskId> {
        if let Some(task) = self.current.take() {
            self.enqueue(task.id, task.qos);
        }
        for class in [QosClass::PerfBurst, QosClass::Interactive, QosClass::Normal, QosClass::Idle] {
            if let Some(task) = self.queue_for(class).pop_front() {
                self.current = Some(task.clone());
                return Some(task.id);
            }
        }
        None
    }

    fn queue_for(&mut self, qos: QosClass) -> &mut VecDeque<Task> {
        match qos {
            QosClass::Idle => &mut self.queues[0],
            QosClass::Normal => &mut self.queues[1],
            QosClass::Interactive => &mut self.queues[2],
            QosClass::PerfBurst => &mut self.queues[3],
        }
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
}
