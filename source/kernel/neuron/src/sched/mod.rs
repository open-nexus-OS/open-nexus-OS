// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal scheduler used during NEURON bring-up
//! OWNERS: @kernel-sched-team
//! PUBLIC API: Scheduler (new/enqueue/next), QosClass, TaskId
//! DEPENDS_ON: uart (boot logs), determinism (timeslice)
//! INVARIANTS: No heap growth in hot paths; timeslice deterministic; RR per QoS
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::collections::VecDeque;

// use crate::determinism; // not needed after inlined guarded access

/// Task identifier handed out by the scheduler.
pub type TaskId = u32;

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

/// Metadata maintained per task.
#[derive(Debug, Clone)]
struct Task {
    id: TaskId,
    qos: QosClass,
}

/// Round-robin scheduler with simple QoS based ordering.
///
/// ## Send/Sync Safety (TASK-0011B, TASK-0012)
///
/// **Single-CPU (current)**:
/// - `Scheduler` is `!Send` and `!Sync` (contains `VecDeque`, not thread-safe)
/// - Lives in `KERNEL_STATE` (single global instance)
/// - Only accessed from trap handler (single-threaded execution)
///
/// **SMP (TASK-0012)**:
/// - Each CPU will have its own `PerCpuScheduler` (no sharing)
/// - `PerCpuScheduler` will be `!Send` (CPU-local, never migrates)
/// - Work stealing will use atomic operations (no direct queue access)
///
/// See `docs/architecture/16-rust-concurrency-model.md` for SMP design.
pub struct Scheduler {
    queues: [VecDeque<Task>; 4],
    current: Option<Task>,
    timeslice_ns: u64,
}

impl Scheduler {
    /// Creates an empty scheduler.
    pub fn new() -> Self {
        #[cfg(all(target_arch = "riscv64", target_os = "none", debug_assertions))]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "SCHED: new enter\n");
        }
        // Read deterministic timeslice guarded and fall back to constant if needed
        let ts = crate::determinism::fixed_tick_ns();
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
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
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "SCHED: new exit\n");
        }
        s
    }

    /// Enqueues a task with the provided QoS class.
    pub fn enqueue(&mut self, id: TaskId, qos: QosClass) {
        // Debug: log only the first few enqueues to avoid UART flood
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            const LOG_LIMIT: usize = 32;
            static ENQ_COUNT: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            let count = ENQ_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if count < LOG_LIMIT {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "[DEBUG sched] enqueue: pid={} qos={:?}", id, qos);
            }
        }

        let task = Task { id, qos };
        self.queue_for(qos).push_back(task);
    }

    /// Picks the next runnable task.
    pub fn schedule_next(&mut self) -> Option<TaskId> {
        crate::liveness::bump();

        // Debug: log queue sizes for the first few iterations only
        let mut u: Option<crate::uart::RawUart> = None;
        #[cfg(all(target_arch = "riscv64", target_os = "none", debug_assertions))]
        {
            const LOG_LIMIT: usize = 256;
            static NEXT_COUNT: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            let count = NEXT_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if count < LOG_LIMIT {
                u = Some(crate::uart::raw_writer());
            }
        }
        if let Some(ref mut w) = u {
            use core::fmt::Write as _;
            let _ = writeln!(
                w,
                "[DEBUG sched] schedule_next: queues=[PerfBurst:{}, Interactive:{}, Normal:{}, Idle:{}]",
                self.queues[3].len(),
                self.queues[2].len(),
                self.queues[1].len(),
                self.queues[0].len()
            );
        }

        for class in [QosClass::PerfBurst, QosClass::Interactive, QosClass::Normal, QosClass::Idle]
        {
            if let Some(task) = self.queue_for(class).pop_front() {
                if let Some(ref mut w) = u {
                    use core::fmt::Write as _;
                    let _ = writeln!(w, "[DEBUG sched] picked: pid={} qos={:?}", task.id, task.qos);
                }
                self.current = Some(task.clone());
                return Some(task.id);
            }
        }
        self.current = None;
        if let Some(mut w) = u {
            use core::fmt::Write as _;
            let _ = writeln!(w, "[DEBUG sched] no task to schedule");
        }
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

    /// Removes all queued references to `id` and clears it if currently running.
    pub fn purge(&mut self, id: TaskId) {
        for q in &mut self.queues {
            q.retain(|t| t.id != id);
        }
        if self.current.as_ref().is_some_and(|t| t.id == id) {
            self.current = None;
        }
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
