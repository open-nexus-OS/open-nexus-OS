// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: blocking a task and waking it. Split out of `task/mod.rs` when TASK-0054C
//! P4c-2 instrumented the runqueue half of a wake: the module sat at its
//! structure-gate ceiling, and waking is its own concern — every IPC send,
//! every timer fire and every child exit ends here.
//!
//! The measured part is `timed_wake_enqueue`: the purge + enqueue that D3's
//! direct handoff proposed to remove. It costs 0.40 µs (peak 0.9), which is
//! 0.41 % of an exchange, against the 5.6–7.7 % that merging the two traps
//! already bought — which is why the handoff was withdrawn rather than built
//! (RFC-0096 §Amendment 2026-09-17).
//!
//! TASK-0330 P2 (ADR-0102): a block is two steps on the thread's wait word, and both steps and
//! the wake decide under the thread's lock in the scheduling record — not by this table. A
//! thread says it is about to wait (`prepare_block`), registers on what it waits for and looks
//! again, and leaves its hart in `commit_block`; a wake between the steps makes the commit
//! answer `Woken` and the syscall looks again. A wake of a blocked thread ends its wait and
//! enqueues it in one hold, so a waker never waits for a thread to finish blocking.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wait word's rule in `sync::wait_word` (host); the scheduler/IPC ladders
//!   in QEMU

use super::affinity::clamp_home_to_affinity;
use super::{BlockReason, Pid, Scheduler, TaskTable, WakeOutcome};
use crate::sched::EnqueueOutcome;
use crate::sync::wait_word::{Commit, Wake};

impl TaskTable {
    /// The first step of a block (ADR-0102): the current task is about to wait for `reason`.
    /// It registers on what it waits for after this and looks once more; then `commit_block`
    /// (it blocks, unless a wake came) or `abort_block` (it found its answer).
    pub fn prepare_block(&mut self, reason: BlockReason) {
        prepare_block(self.current_pid(), reason);
    }

    /// The second step ([`commit_block`]) for the current task.
    pub fn commit_block(&mut self, scheduler: &mut Scheduler) -> Commit {
        // Under the kernel lock the trap's exit moves the hart to its next space, and a reap
        // waits for that lock.
        commit_block(self.current_pid(), scheduler, || ())
    }

    /// The current task found its answer after `prepare_block`: it does not wait.
    pub fn abort_block(&mut self) {
        abort_block(self.current_pid());
    }

    /// Blocks the current task for a wait nothing registers for between the two steps (a
    /// child's exit, a page-in): both steps at once.
    pub fn block_current(&mut self, reason: BlockReason, scheduler: &mut Scheduler) {
        self.prepare_block(reason);
        let _ = self.commit_block(scheduler);
    }

    /// Wakes a waiting task of this table ([`wake`]); a pid the table never gave out is named.
    pub fn wake(&mut self, pid: Pid, scheduler: &mut Scheduler) -> WakeOutcome {
        if self.task(pid).is_none() {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            log_wake_anomaly(pid, "not-found");
            return WakeOutcome::TaskNotFound;
        }
        wake(pid, scheduler)
    }
}

/// The first step of a block: `pid`, running on this hart, is about to wait for `reason`. The
/// steps are the task's record and this hart's words alone — a syscall off the kernel lock
/// blocks with them (ADR-0102).
pub fn prepare_block(pid: Pid, reason: BlockReason) {
    let cpu = crate::smp::cpu_current_id();
    // A time right on loan serves the call it came with: its borrower waiting for anything but
    // the answer to a call of its own runs with it no more.
    if !matches!(reason, BlockReason::IpcCall { .. }) {
        crate::sched::loan::give_back(pid);
    }
    if let Some(rec) = crate::sched::record::get(pid) {
        let mut thread = rec.thread();
        thread.wait = thread.wait.prepare();
        thread.reason = Some(reason);
        thread.blocked_on = cpu;
        // Its deadline lowers the walk's gate and this hart's own floor (TASK-0363 P8) in the
        // hold that makes the wait visible: a walk beside this block either sees the wait or
        // began before the note and keeps it (`sync::noted_bound`). A deadline in no bound
        // would be delivered by the next walk for another task's.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        if let Some(deadline_ns) = reason.deadline_ns() {
            use crate::trap::own_deadlines as own;
            own::lower(own::Kind::Blocked, cpu.as_index(), deadline_ns);
        }
    }
}

/// The second step, one hold of the thread lock: `pid` is blocked, off every run queue and its
/// record released — or a wake came since `prepare_block` and it runs on (`Commit::Woken`: the
/// caller looks again at what it waited for). `blocked` runs in that hold once the thread is
/// blocked — before a waker or an order (a kill, a freeze: `TaskTable::park`) can find it so:
/// where a hart under no kernel lock leaves the thread's address space.
pub fn commit_block(pid: Pid, scheduler: &mut Scheduler, blocked: impl FnOnce()) -> Commit {
    let Some(rec) = crate::sched::record::get(pid) else { return Commit::Woken };
    let mut thread = rec.thread();
    let (next, commit) = thread.wait.commit();
    thread.wait = next;
    match commit {
        Commit::Blocked => {
            // Ensure the scheduler does not keep queued references to this PID.
            scheduler.purge(pid);
            scheduler.finish_current();
            blocked();
        }
        Commit::Woken => thread.reason = None,
    }
    commit
}

/// `pid` found its answer after `prepare_block`: it does not wait.
pub fn abort_block(pid: Pid) {
    if let Some(rec) = crate::sched::record::get(pid) {
        rec.thread().end_wait();
    }
}

/// This hart now runs `pid` (PID 0: its own scheduler loop): the hart's word and the
/// floating-point owner follow — records and per-hart words only.
#[track_caller]
pub fn set_current_here(pid: Pid) {
    if pid == Pid::KERNEL {
        crate::smp::trail::left_task();
    }
    let rec = crate::sched::record::get(pid);
    // ADR-0082: a frozen task is never dispatched — counted, the freeze proof's invariant.
    if rec.is_some_and(|rec| rec.thread().frozen) {
        super::process::note_frozen_ran(pid);
    }
    crate::sched::set_runs_here(pid);
    crate::trap::fp::now_runs(rec.and_then(|rec| rec.fp_if_held()));
}

/// Bounded bring-up diagnostic (A3/A4): callers ignore wake outcomes; a silently failing wake
/// stalls IPC for a full heartbeat. Surface the first few anomalies.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn log_wake_anomaly(pid: Pid, what: &str) {
    static WAKE_ANOMALY_LOGGED: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);
    if WAKE_ANOMALY_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 6 {
        log_info!(target: "smp", "KINIT: wake pid={} {}", pid.as_raw(), what);
    }
}

/// Wakes a waiting task: a blocked one is enqueued on the hart where it runs soonest, one that
/// is still preparing its block keeps the wake for its commit. Everything it reads and changes
/// is the task's scheduling record and the run queues — neither the task table nor the kernel
/// lock (ADR-0102): an IPC path off the lock wakes with this.
///
/// Uses per-CPU runqueues to prevent cross-CPU migration during IPC wakeups,
/// which would otherwise cause starvation under SMP.
pub fn wake(pid: Pid, scheduler: &mut Scheduler) -> WakeOutcome {
    let Some(rec) = crate::sched::record::get(pid) else {
        return WakeOutcome::TaskNotFound;
    };
    // A lender that is woken runs with its time right itself again — before it is placed, so
    // the hart it arrives on finds its borrower without it.
    crate::sched::loan::take_back(pid);
    // One hold of the thread lock from the verdict to the enqueue: the thread is waiting
    // or runnable, never neither, and a second waker finds it not waiting.
    let mut thread = rec.thread();
    let (next, verdict) = thread.wait.wake();
    match verdict {
        Wake::Nothing => {
            drop(thread);
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            log_wake_anomaly(pid, "not-blocked");
            return WakeOutcome::TaskNotBlocked;
        }
        Wake::Noted => {
            thread.wait = next;
            return WakeOutcome::Woken;
        }
        Wake::Run => {}
    }
    // ADR-0082: a frozen task's wake only ends its block — the thaw enqueues it.
    if thread.frozen {
        thread.end_wait();
        return WakeOutcome::Woken;
    }
    // Selftest dummy tasks intentionally carry a zero frame and no AS.
    // Waking them should validate unblock bookkeeping, but must not
    // make them runnable for the real scheduler path. (A blocked task's record is free:
    // this hold may read its frame.)
    if thread.dummy && rec.space().is_none() && rec.frame().sepc == 0 {
        thread.end_wait();
        return WakeOutcome::WokenNoopSelftest;
    }
    let qos = thread.qos;
    // A4/B: the task's home CPU, clamped into its affinity mask and the online set; then
    // TASK-0363 P5: the hart where it runs soonest (`placement`).
    let online = crate::smp::cpu_online();
    let affinity = rec.affinity();
    let home_cpu = clamp_home_to_affinity(affinity, rec.home(), online);
    // TASK-0363 P3: a reservation's holder runs on the hart its reservation is admitted on.
    let target = match crate::sched::reserve::hart_of(pid) {
        Some(hart) => crate::types::CpuId::from_raw(hart as u16),
        None => placement(home_cpu, affinity.intersection(online), qos),
    };
    // No duplicates, then enqueue with the stored QoS. MEASURED: the runqueue half of a
    // wake, what D3's handoff would have removed (TASK-0054C P4c-2).
    if crate::trap::budgets::timed_wake_enqueue(|| {
        scheduler.purge(pid);
        matches!(scheduler.enqueue_on_cpu(target, pid, qos), EnqueueOutcome::Rejected(_))
    }) {
        // It stays blocked: the next wake tries again.
        return WakeOutcome::EnqueueRejected;
    }
    let migrated = target != home_cpu;
    if migrated {
        rec.set_home(target);
    }
    thread.end_wait();
    drop(thread);
    if migrated {
        // An explicit migration, as a steal's: its device lines follow its home.
        crate::irq::rehome(pid);
        crate::smp::record_wake_placement();
    }
    // Cross-core wake: kick the target CPU out of WFI/user so the task
    // runs promptly (best-effort IPI; the evidence chain records it).
    if target != crate::smp::cpu_current_id() {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let ipi_t0 = riscv::register::time::read() as u64;
        let _ = crate::smp::request_resched(target);
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        crate::trap::budgets::record_wake_ipi(
            (riscv::register::time::read() as u64).saturating_sub(ipi_t0),
        );
    }
    WakeOutcome::Woken
}

/// TASK-0363 P5: where a task homed on `home`, allowed on `allowed`, of class `qos` runs
/// soonest — the rule is `placement::place` (host-tested), fed here with what the machine
/// shows now: the free harts, the home's cluster, its queue and the class it runs. Reads the
/// run queues and per-hart words only (called under the woken thread's lock).
fn placement(
    home: crate::types::CpuId,
    allowed: nexus_sched::CpuSet,
    qos: crate::sched::QosClass,
) -> crate::types::CpuId {
    let h = home.as_index();
    let home_runs_lower =
        crate::sched::running_class(h).is_some_and(|running| (running as u8) < (qos as u8));
    let wake = crate::placement::Wake {
        home: h,
        waker: crate::smp::cpu_current_id().as_index(),
        allowed,
        free: crate::smp::free_harts(),
        near: crate::topology::cluster_of(h),
        backlog: crate::sched::queued_at_or_above(h, qos),
        home_runs_lower,
    };
    crate::types::CpuId::from_raw(crate::placement::place(&wake) as u16)
}
