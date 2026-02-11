// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal scheduler used during NEURON bring-up
//! OWNERS: @kernel-sched-team
//! PUBLIC API: Scheduler (new/enqueue/try_enqueue/schedule_next), QosClass, TaskId, EnqueueOutcome
//! DEPENDS_ON: uart (boot logs), determinism (timeslice)
//! INVARIANTS: Bounded queue capacity + deterministic reject on saturation; timeslice deterministic; RR per QoS
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::collections::VecDeque;
use core::{array, marker::PhantomData};

// use crate::determinism; // not needed after inlined guarded access
use crate::types::{CpuId, Pid};

/// Task identifier handed out by the scheduler.
pub type TaskId = Pid;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct QueueCapacity(usize);

impl QueueCapacity {
    const fn new(raw: usize) -> Self {
        Self(raw)
    }

    const fn raw(self) -> usize {
        self.0
    }
}

const RUNTIME_QOS_QUEUE_CAPACITY: QueueCapacity = QueueCapacity::new(64);
const SELFTEST_QOS_QUEUE_CAPACITY: QueueCapacity = QueueCapacity::new(64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueRejectReason {
    QueueFull { qos: QosClass, capacity: usize },
    InvalidCpu { cpu: CpuId },
}

#[must_use = "enqueue outcomes must be handled"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    Rejected(EnqueueRejectReason),
}

struct CpuRunQueues {
    queues: [VecDeque<Task>; 4],
    current: Option<Task>,
}

impl CpuRunQueues {
    fn new() -> Self {
        Self {
            queues: [
                VecDeque::with_capacity(SELFTEST_QOS_QUEUE_CAPACITY.raw()),
                VecDeque::with_capacity(SELFTEST_QOS_QUEUE_CAPACITY.raw()),
                VecDeque::with_capacity(SELFTEST_QOS_QUEUE_CAPACITY.raw()),
                VecDeque::with_capacity(SELFTEST_QOS_QUEUE_CAPACITY.raw()),
            ],
            current: None,
        }
    }
}

/// Per-CPU round-robin scheduler with simple QoS based ordering.
///
/// ## Send/Sync Safety (TASK-0011B, TASK-0012)
///
/// **SMP v1**:
/// - `Scheduler` is `!Send` and `!Sync` (contains `VecDeque`, not thread-safe)
/// - Runtime path stays single-CPU stable for deterministic bring-up
/// - Per-CPU runqueues are available via selftest-only APIs
///
pub struct Scheduler {
    queues: [VecDeque<Task>; 4],
    current: Option<Task>,
    selftest_cpus: [CpuRunQueues; crate::smp::MAX_CPUS],
    timeslice_ns: u64,
    // Pre-SMP contract: scheduler is CPU-local and must not cross thread boundaries.
    _not_send_sync: PhantomData<*mut ()>,
}
static_assertions::assert_not_impl_any!(Scheduler: Send, Sync);

impl Scheduler {
    const SELFTEST_STEAL_MAX_PER_TICK: usize = 1;

    #[inline]
    const fn runtime_queue_capacity_for(_qos: QosClass) -> QueueCapacity {
        // v1b hardening contract: explicit bounded queue behavior in hot path.
        RUNTIME_QOS_QUEUE_CAPACITY
    }

    #[inline]
    const fn selftest_queue_capacity_for(_qos: QosClass) -> QueueCapacity {
        SELFTEST_QOS_QUEUE_CAPACITY
    }

    #[inline]
    fn bounded_push(
        queue: &mut VecDeque<Task>,
        task: Task,
        qos: QosClass,
        capacity: QueueCapacity,
    ) -> EnqueueOutcome {
        if queue.len() >= capacity.raw() {
            return EnqueueOutcome::Rejected(EnqueueRejectReason::QueueFull {
                qos,
                capacity: capacity.raw(),
            });
        }
        queue.push_back(task);
        EnqueueOutcome::Enqueued
    }

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
            // Keep runtime scheduler behavior unchanged for SMP=1 determinism.
            queues: [
                VecDeque::with_capacity(RUNTIME_QOS_QUEUE_CAPACITY.raw()),
                VecDeque::with_capacity(RUNTIME_QOS_QUEUE_CAPACITY.raw()),
                VecDeque::with_capacity(RUNTIME_QOS_QUEUE_CAPACITY.raw()),
                VecDeque::with_capacity(RUNTIME_QOS_QUEUE_CAPACITY.raw()),
            ],
            current: None,
            // Per-CPU queues are used only by SMP selftests.
            selftest_cpus: array::from_fn(|_| CpuRunQueues::new()),
            timeslice_ns: ts,
            _not_send_sync: PhantomData,
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
    pub fn enqueue(&mut self, id: TaskId, qos: QosClass) -> EnqueueOutcome {
        self.try_enqueue(id, qos)
    }

    /// Attempts to enqueue a task and returns explicit bounded-backpressure status.
    pub fn try_enqueue(&mut self, id: TaskId, qos: QosClass) -> EnqueueOutcome {
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
                let _ = writeln!(u, "[DEBUG sched] enqueue: pid={} qos={:?}", id.as_raw(), qos);
            }
        }

        let task = Task { id, qos };
        Self::bounded_push(
            self.queue_for(qos),
            task,
            qos,
            Self::runtime_queue_capacity_for(qos),
        )
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
                    let _ = writeln!(
                        w,
                        "[DEBUG sched] picked: pid={} qos={:?}",
                        task.id.as_raw(),
                        task.qos
                    );
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
            if matches!(
                self.try_enqueue(task.id, task.qos),
                EnqueueOutcome::Rejected(_)
            ) {
                // Deterministic fail-closed behavior for queue saturation:
                // keep the task as current so it is not silently dropped.
                self.current = Some(task);
            }
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

    fn selftest_queue_for(&mut self, cpu_idx: usize, qos: QosClass) -> &mut VecDeque<Task> {
        match qos {
            QosClass::Idle => &mut self.selftest_cpus[cpu_idx].queues[0],
            QosClass::Normal => &mut self.selftest_cpus[cpu_idx].queues[1],
            QosClass::Interactive => &mut self.selftest_cpus[cpu_idx].queues[2],
            QosClass::PerfBurst => &mut self.selftest_cpus[cpu_idx].queues[3],
        }
    }

    fn selftest_try_steal(&mut self, local_cpu: usize, max_qos: QosClass) -> Option<Task> {
        const STEAL_CLASSES_PERF: [QosClass; 4] =
            [QosClass::PerfBurst, QosClass::Interactive, QosClass::Normal, QosClass::Idle];
        const STEAL_CLASSES_INTERACTIVE: [QosClass; 3] =
            [QosClass::Interactive, QosClass::Normal, QosClass::Idle];
        const STEAL_CLASSES_NORMAL: [QosClass; 2] = [QosClass::Normal, QosClass::Idle];
        const STEAL_CLASSES_IDLE: [QosClass; 1] = [QosClass::Idle];

        let steal_classes: &[QosClass] = match max_qos {
            QosClass::PerfBurst => &STEAL_CLASSES_PERF,
            QosClass::Interactive => &STEAL_CLASSES_INTERACTIVE,
            QosClass::Normal => &STEAL_CLASSES_NORMAL,
            QosClass::Idle => &STEAL_CLASSES_IDLE,
        };
        let online_mask = crate::smp::cpu_online_mask();
        for class in steal_classes {
            for victim in 0..crate::smp::MAX_CPUS {
                if victim == local_cpu {
                    continue;
                }
                // Unit tests run without a real CPU-online mask contract.
                if !cfg!(test) && online_mask != 0 && (online_mask & (1usize << victim)) == 0 {
                    continue;
                }
                if let Some(task) = self.selftest_queue_for(victim, *class).pop_front() {
                    crate::smp::record_work_steal();
                    return Some(task);
                }
            }
        }
        None
    }

    pub fn selftest_reset_cpu(&mut self, cpu: CpuId) {
        let cpu_idx = cpu.as_index();
        if cpu_idx >= crate::smp::MAX_CPUS {
            return;
        }
        for queue in &mut self.selftest_cpus[cpu_idx].queues {
            queue.clear();
        }
        self.selftest_cpus[cpu_idx].current = None;
    }

    pub fn selftest_enqueue_on_cpu(
        &mut self,
        cpu: CpuId,
        id: TaskId,
        qos: QosClass,
    ) -> EnqueueOutcome {
        self.selftest_try_enqueue_on_cpu(cpu, id, qos)
    }

    fn selftest_try_enqueue_on_cpu(
        &mut self,
        cpu: CpuId,
        id: TaskId,
        qos: QosClass,
    ) -> EnqueueOutcome {
        let cpu_idx = cpu.as_index();
        if cpu_idx >= crate::smp::MAX_CPUS {
            return EnqueueOutcome::Rejected(EnqueueRejectReason::InvalidCpu { cpu });
        }
        let task = Task { id, qos };
        Self::bounded_push(
            self.selftest_queue_for(cpu_idx, qos),
            task,
            qos,
            Self::selftest_queue_capacity_for(qos),
        )
    }

    pub fn selftest_schedule_on_cpu(&mut self, cpu: CpuId) -> Option<TaskId> {
        let cpu_idx = cpu.as_index();
        if cpu_idx >= crate::smp::MAX_CPUS {
            return None;
        }

        for class in [QosClass::PerfBurst, QosClass::Interactive, QosClass::Normal, QosClass::Idle]
        {
            if let Some(task) = self.selftest_queue_for(cpu_idx, class).pop_front() {
                self.selftest_cpus[cpu_idx].current = Some(task.clone());
                return Some(task.id);
            }
            for _ in 0..Self::SELFTEST_STEAL_MAX_PER_TICK {
                if let Some(task) = self.selftest_try_steal(cpu_idx, class) {
                    self.selftest_cpus[cpu_idx].current = Some(task.clone());
                    return Some(task.id);
                }
            }
        }

        self.selftest_cpus[cpu_idx].current = None;
        None
    }

    pub fn selftest_try_steal_for_class(
        &mut self,
        local_cpu: CpuId,
        max_qos: QosClass,
    ) -> Option<TaskId> {
        let cpu_idx = local_cpu.as_index();
        if cpu_idx >= crate::smp::MAX_CPUS {
            return None;
        }
        self.selftest_try_steal(cpu_idx, max_qos).map(|task| task.id)
    }

    pub fn selftest_queue_len(&self, cpu: CpuId, qos: QosClass) -> usize {
        let cpu_idx = cpu.as_index();
        if cpu_idx >= crate::smp::MAX_CPUS {
            return 0;
        }
        let queue_idx = match qos {
            QosClass::Idle => 0,
            QosClass::Normal => 1,
            QosClass::Interactive => 2,
            QosClass::PerfBurst => 3,
        };
        self.selftest_cpus[cpu_idx].queues[queue_idx].len()
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
        assert!(matches!(
            sched.enqueue(Pid::from_raw(1), QosClass::Normal),
            EnqueueOutcome::Enqueued
        ));
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
    fn deterministic_timeslice() {
        let sched = Scheduler::new();
        assert_eq!(sched.timeslice_ns(), crate::determinism::fixed_tick_ns());
    }

    #[test]
    fn yield_requeues_current_task() {
        let mut sched = Scheduler::new();
        assert!(matches!(
            sched.enqueue(Pid::from_raw(1), QosClass::Normal),
            EnqueueOutcome::Enqueued
        ));
        assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
        sched.yield_current();
        assert_eq!(sched.schedule_next(), Some(Pid::from_raw(1)));
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
            assert!(matches!(
                sched.try_enqueue(Pid::from_raw(pid), qos),
                EnqueueOutcome::Enqueued
            ));
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
}
