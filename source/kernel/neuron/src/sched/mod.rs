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
//! DEPENDS_ON: run_queue (the rules), sync (HartTable, RankedLock), determinism (timeslice)
//! INVARIANTS: Bounded queue capacity + deterministic reject on saturation; timeslice
//!   deterministic; RR per QoS; a hart's queue lock is a leaf — held for one queue operation,
//!   never while the kernel lock or a second hart's queues are waited for
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::run_queue::{self, weight_of, HartQueues, Harts, Owner, Stolen, Task, CLASS_CAPACITY};
use crate::sync::hart_table::{HartTable, SizeError};
use crate::sync::lock_rank::Class;
use crate::sync::ranked::RankedLock;
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

/// What [`Scheduler::withdraw`] did.
#[must_use = "a running task is left to its own hart"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withdrawn {
    /// It runs on that hart: it stays there; its order goes as a pending mark.
    Running(CpuId),
    /// It was queued and is not any more.
    Removed,
    /// Neither queued nor running.
    Absent,
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
static RUN_QUEUES: HartTable<RankedLock<HartQueues>> =
    HartTable::new(RankedLock::new(Class::RunQueue, HartQueues::new()));

/// The task each hart runs (its trap context: a user task, or PID 0 while the hart has none) —
/// written by that hart only, at a dispatch that may hold no kernel lock (TASK-0363 P2 S3).
static RUNS: HartTable<AtomicU32> = HartTable::new(AtomicU32::new(0));

/// Sizes the run queues for a machine of `harts` CPUs (`smp::size_for_machine`).
pub fn size(harts: usize) -> Result<(), SizeError> {
    // Built at compile time: a hart's queues are 20 KiB, copied from the image — never built on
    // the boot stack (`HartTable::size`).
    RUN_QUEUES
        .size(harts, || const { RankedLock::new(Class::RunQueue, HartQueues::new()) })
        .and_then(|()| RUNS.size(harts, || AtomicU32::new(0)))
        .and_then(|()| meter::size(harts))
        .and_then(|()| reserve::size(harts))?;
    // RFC-0102: two run queues are held together in ascending CPU index only.
    for (cpu, queues) in RUN_QUEUES.iter().enumerate() {
        queues.set_key(cpu);
    }
    // TASK-0363 P3: a fair task asks for one preemption tick at a time.
    let slice = crate::hal::platform::ns_to_ticks(crate::hal::platform::PREEMPT_TICK_NS);
    for cpu in 0..RUN_QUEUES.len() {
        let _ = with(cpu, |q| q.set_slice(slice));
    }
    Ok(())
}

/// Task `id` of class `qos` as it arrives on a hart at `now`: its weight from its class and
/// shares, the lag it left its last hart with, its reservation (`reserve::arrive`).
fn arriving(id: TaskId, qos: QosClass, now: u64) -> Task {
    let record = record::get(id);
    let (shares, lag) = record.map_or((100, 0), |r| (r.shares(), r.lag()));
    Task {
        weight: weight_of(qos, shares),
        lag,
        res: reserve::arrive(id.as_raw(), now),
        ..Task::new(id.as_raw(), qos)
    }
}

/// The weight of task `id` at class `qos` (its record's shares).
fn weight_for(id: u32, qos: QosClass) -> u32 {
    weight_of(qos, record::get(Pid::from_raw(id)).map_or(100, |r| r.shares()))
}

/// The task this hart runs (PID 0 before its first dispatch).
pub fn runs_here() -> TaskId {
    runs_on(crate::smp::cpu_current_id().as_index())
}

/// The task hart `cpu` runs (PID 0: its scheduler loop) — for a line that names a breach.
pub fn runs_on(cpu: usize) -> TaskId {
    Pid::from_raw(RUNS.get(cpu).map_or(0, |slot| slot.load(Ordering::Acquire)))
}

/// The class of the task hart `cpu` runs off its queues; `None` while it runs none (a wake's
/// placement reads it under the woken thread's lock, where no other thread's is taken).
pub fn running_class(cpu: usize) -> Option<QosClass> {
    with(cpu, |q| q.current().map(|task| task.qos)).flatten()
}

/// Whether hart `cpu` has a task of class `qos` or above queued (TASK-0363 P5: a task woken
/// there would wait behind it).
pub fn queued_at_or_above(cpu: usize, qos: QosClass) -> bool {
    with(cpu, |q| QosClass::DESCENDING.iter().any(|&c| c as u8 >= qos as u8 && q.len(c) > 0))
        .unwrap_or(false)
}

/// This hart now runs `pid`.
pub fn set_runs_here(pid: TaskId) {
    let cpu = crate::smp::cpu_current_id().as_index();
    if let Some(slot) = RUNS.get(cpu) {
        slot.store(pid.as_raw(), Ordering::Release);
    }
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

/// Hands task `id`'s record to `owner` (a probe id without a record is left be).
fn hand(id: u32, owner: Owner) {
    if let Some(record) = record::get(Pid::from_raw(id)) {
        record.set_owner(owner);
    }
}

/// The task hart `cpu` ran leaves it without a requeue (it blocked, it exited): its record is
/// free — if the hart still held it (a requeued task's is its queue's).
fn leave(id: u32, cpu: usize) {
    if let Some(record) = record::get(Pid::from_raw(id)) {
        record.pass(Owner::Running(cpu as u16), Owner::Free);
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

    /// The run queues for a path that holds no kernel lock (TASK-0330 P2, ADR-0102): every
    /// operation is a queue's own lock (rank 7) — the handle carries the tick length only.
    pub fn unlocked() -> Self {
        Self { timeslice_ns: crate::determinism::fixed_tick_ns(), _not_send_sync: PhantomData }
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
        let weight = weight_for(id.as_raw(), qos);
        // Running anywhere: update in place (any hart's current slot).
        if harts
            .clone()
            .any(|cpu| with(cpu, |q| q.set_qos_if_current(id.as_raw(), qos, weight)) == Some(true))
        {
            return SetQosOutcome::Updated;
        }
        // Queued: move within the CPU it is queued on (no migration here).
        let mut requeued =
            harts.filter_map(|cpu| with(cpu, |q| q.requeue(id.as_raw(), qos, weight)).flatten());
        match requeued.next() {
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
        let task = arriving(id, qos, now);
        let queued =
            with(cpu, |q| q.push(task, now).map(|()| hand(id.as_raw(), Owner::Queued(cpu as u16))));
        match queued {
            Some(Ok(())) => EnqueueOutcome::Enqueued,
            Some(Err(run_queue::Full)) => rejected_full(qos),
            None => EnqueueOutcome::Rejected(EnqueueRejectReason::InvalidCpu {
                cpu: CpuId::from_raw(cpu as u16),
            }),
        }
    }

    /// Picks the next runnable task.
    pub fn schedule_next(&mut self) -> Option<TaskId> {
        Self::pick_here()
    }

    /// Picks this hart's next runnable task: its own queues under its own lock, nothing else —
    /// so the dispatch calls it without the kernel lock (TASK-0363 P2 S3; `cpu_main`).
    pub fn pick_here() -> Option<TaskId> {
        crate::liveness::bump();
        let cur = Self::current_cpu_index();
        // The record passes with the pick, under the queue lock: the task the hart ran and did
        // not requeue (it blocked) is free, the picked one is this hart's.
        let picked = meter::metered(cur, |q| {
            let old = q.current();
            let new = q.pick();
            if let Some(old) = old.filter(|o| new.is_none_or(|n| n.id != o.id)) {
                leave(old.id, cur);
            }
            if let Some(new) = new {
                hand(new.id, Owner::Running(cur as u16));
            }
            new
        })
        .flatten();
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

    /// The running task is preempted without a pick (a driver of the order that picks on its
    /// own — the selftests; a trap uses `give_way`): requeued as it stands in the order, its
    /// virtual time kept (TASK-0363 P3).
    pub fn preempt_current(&mut self) {
        // Queued while still its hart's current: it waits behind its peers, not behind an
        // idle hart (`ready_stats`). Deterministic fail-closed behavior for queue
        // saturation: a rejected task stays current so it is not silently dropped.
        let now = ready::now_ticks();
        let cur = Self::current_cpu_index();
        let _ = meter::metered(cur, |q| {
            let id = q.current().map(|t| t.id);
            if q.requeue_current(now) {
                id.into_iter().for_each(|id| hand(id, Owner::Queued(cur as u16)));
            }
        });
    }

    /// Marks the current task as finished without re-enqueuing it; its lag is kept for its
    /// next wake (its record).
    pub fn finish_current(&mut self) {
        let cur = Self::current_cpu_index();
        let _ = meter::metered(cur, |q| {
            if let Some(task) = q.take_current() {
                leave(task.id, cur);
                if let Some(record) = record::get(Pid::from_raw(task.id)) {
                    record.set_lag(task.lag);
                }
            }
        });
    }

    /// The CPU `id` is the current task of, if any (ADR-0082: an order for a task running on
    /// another hart is left as a pending mark and delivered by a resched IPI).
    pub fn running_on(&self, id: TaskId) -> Option<CpuId> {
        (0..RUN_QUEUES.len())
            .find(|&cpu| with(cpu, |q| q.current()).flatten().is_some_and(|t| t.id == id.as_raw()))
            .map(|index| CpuId::from_raw(index as u16))
    }

    /// Takes `id` off the hart it is queued on, or names the hart that runs it — decided under
    /// that hart's queue lock, so a dispatch without the kernel lock never starts a task that is
    /// being killed or frozen (TASK-0363 P2 S3). A steal moves a queued task meanwhile, under
    /// both harts' queue locks (TASK-0330 P4): it is on one hart at every look, but a walk
    /// over the harts can pass it — `Absent` for a task whose record is held means look again
    /// (`TaskTable::park`).
    pub fn withdraw(&mut self, id: TaskId) -> Withdrawn {
        for cpu in 0..RUN_QUEUES.len() {
            match with(cpu, |q| q.withdraw(id.as_raw())) {
                Some(run_queue::Withdraw::Running) => {
                    return Withdrawn::Running(CpuId::from_raw(cpu as u16));
                }
                Some(run_queue::Withdraw::Removed) => {
                    hand(id.as_raw(), Owner::Free);
                    return Withdrawn::Removed;
                }
                _ => {}
            }
        }
        Withdrawn::Absent
    }

    /// Removes all queued references to `id` and clears it if currently running.
    pub fn purge(&mut self, id: TaskId) {
        for cpu in 0..RUN_QUEUES.len() {
            let _ = meter::metered(cpu, |q| q.purge(id.as_raw()));
        }
        if let Some(record) = record::get(id) {
            record.set_owner(Owner::Free);
        }
    }

    /// One steal for hart `local` (`run_queue::steal`) under its own queue lock, the hart
    /// charged to now first: the task taken is its running task and its record the hart's
    /// when the lock is dropped. Counted as the pop it replaces was.
    fn steal_for(
        local: usize,
        max_qos: QosClass,
        allow: impl FnMut(u32) -> bool,
        home_of: impl FnMut(u32) -> usize,
    ) -> Option<Task> {
        let stolen = meter::metered(local, |own| {
            let stolen = run_queue::steal(&Machine, local, own, max_qos, allow, home_of);
            if let Some(Stolen::Taken(task)) = stolen {
                hand(task.id, Owner::Running(local as u16));
            }
            stolen
        })??;
        crate::smp::record_work_steal();
        match stolen {
            Stolen::Taken(task) => Some(task),
            Stolen::Rejected => None,
        }
    }

    pub fn selftest_reset_cpu(&mut self, cpu: CpuId) {
        let _ = meter::metered(cpu.as_index(), HartQueues::reset);
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

    /// Runtime work stealing (A8): one bounded steal for the executing hart when its own
    /// queues are empty — under the queue locks alone, so the idle loop calls it without the
    /// kernel lock (TASK-0330 P4). On success the task is this hart's running task, as after
    /// `pick_here`, and the hart its home (an explicit migration: its device lines follow,
    /// RFC-0118). Victims are scanned in ascending order; the QoS ceiling is respected.
    ///
    /// B (TASK-0042) + ADR-0052: a task this hart may not run — its affinity, read from its
    /// record under its hart's queue lock — parks at the FRONT of its home hart's queue (the
    /// pick has no affinity check, so a park here would run it here). It leaves its queue
    /// only for a place that took it.
    pub fn steal_here(max_qos: QosClass) -> Option<TaskId> {
        let cur = Self::current_cpu_index();
        let pid = Self::steal_as(
            max_qos,
            |pid| record::get(pid).is_some_and(|record| record.affinity().contains(cur)),
            |pid| record::get(pid).map_or(0, |record| record.home().as_index()),
        )?;
        if let Some(record) = record::get(pid) {
            record.set_home(CpuId::from_raw(cur as u16));
        }
        crate::irq::rehome(pid);
        Some(pid)
    }

    /// The steal for the executing hart by the caller's rules: `allow` whether it may run a
    /// task, `home_of` where a refused one belongs.
    fn steal_as(
        max_qos: QosClass,
        mut allow: impl FnMut(TaskId) -> bool,
        mut home_of: impl FnMut(TaskId) -> usize,
    ) -> Option<TaskId> {
        // TASK-0363 P3: a reservation's holder runs on the hart it is admitted on — never
        // stolen (its home is that hart: a refused one parks there).
        let task = Self::steal_for(
            Self::current_cpu_index(),
            max_qos,
            |id| !reserve::holds(Pid::from_raw(id)) && allow(Pid::from_raw(id)),
            |id| home_of(Pid::from_raw(id)),
        )?;
        ready::dispatched(&task);
        Some(Pid::from_raw(task.id))
    }

    /// A steal by the executing hart that may run nothing: the first task found parks on `home`.
    pub fn selftest_steal_refused(&mut self, home: CpuId) -> Option<TaskId> {
        Self::steal_as(QosClass::PerfBurst, |_| false, |_| home.as_index())
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
        for class in QosClass::DESCENDING {
            // Its own queue's front, else a steal — which arrives as the hart's running task.
            let own =
                meter::metered(cpu_idx, |q| q.pop_front(class).inspect(|&task| q.adopt(task)));
            let task = own.flatten().or_else(|| {
                (0..Self::SELFTEST_STEAL_MAX_PER_TICK)
                    .find_map(|_| Self::steal_for(cpu_idx, class, |_| true, |_| cpu_idx))
            });
            if let Some(task) = task {
                return Some(Pid::from_raw(task.id));
            }
        }
        let _ = meter::metered(cpu_idx, |q| q.set_current(None));
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
pub mod loan;
pub mod meter;
mod ready;
pub mod record;
pub mod reserve;
