// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the scheduler as the kernel calls it — every hart's run queues behind that hart's
//! own lock (TASK-0363 P2 S1). The rules live in `run_queue` (pure, host-tested); this file
//! names the executing hart, takes the lock of the hart an operation touches, stamps the
//! clock and feeds the wait statistics.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: `sched/run_queue_tests.rs` (host) + QEMU selftests + boot markers
//! PUBLIC API: Scheduler (new/enqueue/try_enqueue/schedule_next), QosClass, TaskId, EnqueueOutcome
//! DEPENDS_ON: run_queue (the rules), sync (HartTable, SpinIrqLock), determinism (timeslice)
//! INVARIANTS: Bounded queue capacity + deterministic reject on saturation; timeslice
//!   deterministic; RR per QoS; a hart's queue lock is a leaf — held for one queue operation,
//!   never while the kernel lock or a second hart's queues are waited for
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use core::marker::PhantomData;

use crate::run_queue::{self, HartQueues, Harts, Stolen, Task, CLASS_CAPACITY};
use crate::sync::hart_table::{HartTable, SizeError};
use crate::sync::spin_irq::{IrqOffGuard, SpinIrqLock};
use crate::types::{CpuId, Pid};

pub use crate::run_queue::QosClass;

/// Task identifier handed out by the scheduler.
pub type TaskId = Pid;

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

#[must_use = "set-qos outcomes must be handled"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetQosOutcome {
    Updated,
    NotFound,
    QueueFull,
}

/// Every hart's run queues, each behind its own lock: the boot hart's from the first
/// instruction, the others' sized with the machine (`smp::size_for_machine`).
static RUN_QUEUES: HartTable<SpinIrqLock<HartQueues>> =
    HartTable::new(SpinIrqLock::new(HartQueues::new()));

/// Sizes the run queues for a machine of `harts` CPUs (`smp::size_for_machine`).
pub fn size(harts: usize) -> Result<(), SizeError> {
    RUN_QUEUES.size(harts, || SpinIrqLock::new(HartQueues::new()))
}

/// Runs `f` on hart `cpu`'s queues under their lock. `None`: the machine has no such hart.
fn with<R>(cpu: usize, f: impl FnOnce(&mut HartQueues) -> R) -> Option<R> {
    RUN_QUEUES.get(cpu).map(|queues| f(&mut queues.lock()))
}

/// The machine's harts as a thief sees them: a hart's queues only if they are free.
struct Machine;

impl Harts for Machine {
    fn count(&self) -> usize {
        RUN_QUEUES.len()
    }

    fn online(&self, cpu: usize) -> bool {
        let online = crate::smp::cpu_online();
        online.is_empty() || online.contains(cpu)
    }

    fn try_with<R>(&self, cpu: usize, f: impl FnOnce(&mut HartQueues) -> R) -> Option<R> {
        RUN_QUEUES.get(cpu)?.try_lock().map(|mut queues| f(&mut queues))
    }
}

fn rejected_full(qos: QosClass) -> EnqueueOutcome {
    EnqueueOutcome::Rejected(EnqueueRejectReason::QueueFull { qos, capacity: CLASS_CAPACITY })
}

/// Per-CPU round-robin scheduler with simple QoS based ordering.
///
/// ## SMP model (TASK-0363 P2 S1)
///
/// One [`HartQueues`] per CPU is THE runtime state, each behind its own lock in a table the
/// machine sizes. Every method without a CPU argument addresses the executing CPU (tp-derived
/// identity), so each hart schedules from its own queues with unchanged call sites; an
/// operation on another hart's queues (a wake to the home CPU, a steal, a purge) takes that
/// hart's lock for the one operation. The value itself still lives in the kernel lock's
/// aggregate — its callers are the kernel lock's — and stays `!Send`/`!Sync`: what it
/// guards is no longer the queues but the order of the callers' own steps (S2 and S3 take
/// those off the kernel lock). The `selftest_*` APIs are thin wrappers over the same state so
/// the RFC-0021 markers keep their meaning. SMP=1: everything lands on CPU 0 with
/// byte-identical scheduling order to the pre-SMP scheduler.
pub struct Scheduler {
    timeslice_ns: u64,
    _not_send_sync: PhantomData<*mut ()>,
}
static_assertions::assert_not_impl_any!(Scheduler: Send, Sync);

impl Scheduler {
    const SELFTEST_STEAL_MAX_PER_TICK: usize = 1;

    /// Implicit scheduling domain: the executing CPU (tp-derived identity).
    #[inline]
    fn current_cpu_index() -> usize {
        let idx = crate::smp::cpu_current_id().as_index();
        idx * (idx < RUN_QUEUES.len()) as usize
    }

    /// Creates the scheduler: the machine's queues start empty.
    pub fn new() -> Self {
        let ts = crate::determinism::fixed_tick_ns();
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            use core::fmt::Write as _;
            if !crate::log::kfold(crate::log::KGroup::Sched, crate::log::Level::Info) {
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "SCHED: timeslice={}", ts);
            }
        }
        Self { timeslice_ns: ts, _not_send_sync: PhantomData }
    }

    /// Enqueues a task with the provided QoS class.
    pub fn enqueue(&mut self, id: TaskId, qos: QosClass) -> EnqueueOutcome {
        self.try_enqueue(id, qos)
    }

    /// Returns the currently running task id, if any.
    pub fn current_task_id(&self) -> Option<TaskId> {
        with(Self::current_cpu_index(), |q| q.current()).flatten().map(|t| Pid::from_raw(t.id))
    }

    /// Updates the scheduler-visible QoS for `id` in current/queued state.
    ///
    /// If the task is currently running, this updates the in-place current entry.
    /// If the task is queued, it moves the queue entry to the target QoS bucket.
    pub fn set_task_qos(&mut self, id: TaskId, qos: QosClass) -> SetQosOutcome {
        let harts = 0..RUN_QUEUES.len();
        // Running anywhere: update in place (any hart's current slot).
        if harts
            .clone()
            .any(|cpu| with(cpu, |q| q.set_qos_if_current(id.as_raw(), qos)) == Some(true))
        {
            return SetQosOutcome::Updated;
        }
        // Queued: move within the CPU it is queued on (no migration here).
        match harts.filter_map(|cpu| with(cpu, |q| q.requeue(id.as_raw(), qos)).flatten()).next() {
            Some(Ok(())) => SetQosOutcome::Updated,
            Some(Err(run_queue::Full)) => SetQosOutcome::QueueFull,
            None => SetQosOutcome::NotFound,
        }
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
            // Runtime-gated (off by default; MAX_LEVEL=Info): the raw write bypasses the diag
            // facade, so honour the same DEBUG/topic gate the facade uses. Re-enable via the
            // boot knob (`NEXUS_LOG=sched=debug`) without a rebuild.
            if count < LOG_LIMIT && crate::log::would_log(crate::log::Level::Debug, "sched") {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "[DEBUG sched] enqueue: pid={} qos={:?}", id.as_raw(), qos);
            }
        }
        Self::push(Self::current_cpu_index(), id, qos)
    }

    /// Queues `id` on hart `cpu`: one operation under that hart's lock.
    fn push(cpu: usize, id: TaskId, qos: QosClass) -> EnqueueOutcome {
        let now = ready::now_ticks();
        match with(cpu, |q| q.push_back(id.as_raw(), qos, now)) {
            Some(Ok(())) => EnqueueOutcome::Enqueued,
            Some(Err(run_queue::Full)) => rejected_full(qos),
            None => EnqueueOutcome::Rejected(EnqueueRejectReason::InvalidCpu {
                cpu: CpuId::from_raw(cpu as u16),
            }),
        }
    }

    /// Picks the next runnable task.
    pub fn schedule_next(&mut self) -> Option<TaskId> {
        crate::liveness::bump();
        let cur = Self::current_cpu_index();
        let picked = with(cur, HartQueues::pick).flatten();
        // Debug: the first few picks only (off by default; `NEXUS_LOG=sched=debug`), written
        // after the queue lock is dropped.
        #[cfg(all(target_arch = "riscv64", target_os = "none", debug_assertions))]
        {
            const LOG_LIMIT: usize = 256;
            static NEXT_COUNT: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            let count = NEXT_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if count < LOG_LIMIT && crate::log::would_log(crate::log::Level::Debug, "sched") {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = match picked {
                    Some(task) => {
                        writeln!(
                            u,
                            "[DEBUG sched] cpu{} picked: pid={} qos={:?}",
                            cur, task.id, task.qos
                        )
                    }
                    None => writeln!(u, "[DEBUG sched] cpu{} no task to schedule", cur),
                };
            }
        }
        let task = picked?;
        ready::dispatched(&task);
        Some(Pid::from_raw(task.id))
    }

    /// Re-enqueue the currently running task (call on timeslice/yield).
    pub fn yield_current(&mut self) {
        // Queued while still its hart's current: it waits behind its peers, not behind an
        // idle hart (`ready_stats`). Deterministic fail-closed behavior for queue
        // saturation: a rejected task stays current so it is not silently dropped.
        let now = ready::now_ticks();
        let _ = with(Self::current_cpu_index(), |q| q.yield_current(now));
    }

    /// Marks the current task as finished without re-enqueuing it.
    pub fn finish_current(&mut self) {
        let _ = with(Self::current_cpu_index(), |q| q.set_current(None));
    }

    /// The CPU `id` is the current task of, if any (ADR-0082: an order for a task running on
    /// another hart is left as a pending mark and delivered by a resched IPI).
    pub fn running_on(&self, id: TaskId) -> Option<CpuId> {
        (0..RUN_QUEUES.len())
            .find(|&cpu| with(cpu, |q| q.current()).flatten().is_some_and(|t| t.id == id.as_raw()))
            .map(|index| CpuId::from_raw(index as u16))
    }

    /// Removes all queued references to `id` and clears it if currently running.
    pub fn purge(&mut self, id: TaskId) {
        for cpu in 0..RUN_QUEUES.len() {
            let _ = with(cpu, |q| q.purge(id.as_raw()));
        }
    }

    /// One steal for hart `local` (`run_queue::steal`), counted as the pop it replaces was.
    fn steal_for(
        local: usize,
        max_qos: QosClass,
        allow: impl FnMut(u32) -> bool,
        home_of: impl FnMut(u32) -> usize,
    ) -> Option<Task> {
        match run_queue::steal(&Machine, local, max_qos, allow, home_of)? {
            Stolen::Taken(task) => {
                crate::smp::record_work_steal();
                Some(task)
            }
            Stolen::Rejected => {
                crate::smp::record_work_steal();
                None
            }
        }
    }

    pub fn selftest_reset_cpu(&mut self, cpu: CpuId) {
        let _ = with(cpu.as_index(), HartQueues::reset);
    }

    /// Enqueues a task on a specific CPU's per-CPU runqueue.
    ///
    /// This is the runtime (non-selftest) path for SMP-aware wakeups.
    /// The caller (typically `TaskTable::wake`) passes the task's `home_cpu`
    /// to ensure tasks stay on their home CPU and avoid cross-CPU migration
    /// during IPC wakeups.
    pub fn enqueue_on_cpu(&mut self, cpu: CpuId, id: TaskId, qos: QosClass) -> EnqueueOutcome {
        let cpu_idx = cpu.as_index();
        if cpu_idx >= RUN_QUEUES.len() {
            return EnqueueOutcome::Rejected(EnqueueRejectReason::InvalidCpu { cpu });
        }
        // Bounded bring-up diagnostic: the first few REMOTE enqueues prove
        // cross-CPU placement actually happens (A3/A4 evidence).
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        if cpu_idx != 0 {
            static REMOTE_ENQ_LOGGED: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            if REMOTE_ENQ_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 3 {
                log_info!(target: "smp", "KINIT: enqueue cpu{} pid={}", cpu_idx, id.as_raw());
            }
        }
        Self::push(cpu_idx, id, qos)
    }

    /// Runtime work stealing (A8): bounded single-task steal for the executing
    /// CPU when its own queues are empty. On success the stolen task becomes
    /// this CPU's current and its id is returned; the caller migrates the
    /// task's `home_cpu` (explicit migration). Victims are scanned in
    /// ascending CPU order; the QoS ceiling is respected.
    ///
    /// B (TASK-0042) + ADR-0052: a task the thief may not run (`allow`) parks — no loss, no
    /// reorder — at the FRONT of its HOME CPU's queue (`home_of`); `schedule_next` has no
    /// affinity check, so a park on the thief would run it there. The task is judged under
    /// its hart's queue lock and leaves that queue only for a place that took it.
    pub fn steal_into_current(
        &mut self,
        max_qos: QosClass,
        mut allow: impl FnMut(TaskId) -> bool,
        mut home_of: impl FnMut(TaskId) -> usize,
    ) -> Option<TaskId> {
        // No trap on this hart between the victim's queue and its own: a stolen task is in
        // neither while it changes hands.
        let _irq_off = IrqOffGuard::new();
        let cur = Self::current_cpu_index();
        let task = Self::steal_for(
            cur,
            max_qos,
            |id| allow(Pid::from_raw(id)),
            |id| home_of(Pid::from_raw(id)),
        )?;
        ready::dispatched(&task);
        let _ = with(cur, |q| q.set_current(Some(task)));
        Some(Pid::from_raw(task.id))
    }

    pub fn selftest_enqueue_on_cpu(
        &mut self,
        cpu: CpuId,
        id: TaskId,
        qos: QosClass,
    ) -> EnqueueOutcome {
        match with(cpu.as_index(), |q| q.push_back(id.as_raw(), qos, ready::now_ticks())) {
            Some(Ok(())) => EnqueueOutcome::Enqueued,
            Some(Err(run_queue::Full)) => rejected_full(qos),
            None => EnqueueOutcome::Rejected(EnqueueRejectReason::InvalidCpu { cpu }),
        }
    }

    pub fn selftest_schedule_on_cpu(&mut self, cpu: CpuId) -> Option<TaskId> {
        let cpu_idx = cpu.as_index();
        if cpu_idx >= RUN_QUEUES.len() {
            return None;
        }
        let _irq_off = IrqOffGuard::new();
        for class in QosClass::DESCENDING {
            let own = with(cpu_idx, |q| q.pop_front(class)).flatten();
            let task = own.or_else(|| {
                (0..Self::SELFTEST_STEAL_MAX_PER_TICK)
                    .find_map(|_| Self::steal_for(cpu_idx, class, |_| true, |_| cpu_idx))
            });
            if let Some(task) = task {
                let _ = with(cpu_idx, |q| q.set_current(Some(task)));
                return Some(Pid::from_raw(task.id));
            }
        }
        let _ = with(cpu_idx, |q| q.set_current(None));
        None
    }

    pub fn selftest_try_steal_for_class(
        &mut self,
        local_cpu: CpuId,
        max_qos: QosClass,
    ) -> Option<TaskId> {
        let cpu_idx = local_cpu.as_index();
        if cpu_idx >= RUN_QUEUES.len() {
            return None;
        }
        Self::steal_for(cpu_idx, max_qos, |_| true, |_| cpu_idx).map(|task| Pid::from_raw(task.id))
    }

    pub fn selftest_queue_len(&self, cpu: CpuId, qos: QosClass) -> usize {
        with(cpu.as_index(), |q| q.len(qos)).unwrap_or(0)
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

mod class;
mod ready;
