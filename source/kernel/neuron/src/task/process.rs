// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the process model — a process is its address space (TASK-0333 6d, ADR-0082,
//! RFC-0111). The task that created the space is its leader; a task spawned INTO the parent's
//! own space is a thread of it. The parent controls the whole process: `task_suspend` freezes
//! leader and threads, `task_resume` thaws them, `task_kill` ends them, and the leader's own
//! exit takes its threads (`syscall::api::process_kill`).
//!
//! A frozen task is `Suspended` with `frozen` set: never enqueued, never dispatched, a wake
//! only clears its block. An order for a task running on ANOTHER hart is a `Pending` mark plus
//! a resched IPI; that hart executes it at its next trap entry (`trap::pending`), the way a
//! signal is delivered on the return to user mode. Nothing here waits.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: task freeze ok`, `KSELFTEST: process exit takes threads ok`;
//!   `syscall/api/tests_process.rs` (the refusals)

use super::affinity::clamp_home_to_affinity;
use super::{Pid, ResumeOutcome, Scheduler, TaskState, TaskTable};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// An order for a task that is running on another hart, executed at its next trap entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    /// The process is being frozen: park at the trap entry (the trap re-executes on thaw).
    Freeze,
    /// The process is being killed: run the exit funnel at the trap entry.
    Kill,
}

/// What a freeze order did: how many tasks parked here, how many were handed to their hart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FreezeOutcome {
    pub frozen: usize,
    pub deferred: usize,
}

/// Dispatches of a frozen task — the invariant's counter (zero is the proof).
static FROZEN_RAN: AtomicUsize = AtomicUsize::new(0);
/// The freeze proof line is emitted once per boot.
static FREEZE_PROVEN: AtomicBool = AtomicBool::new(false);

/// The monotonic clock in nanoseconds (the freeze's duration in the proof line).
fn now_ns() -> u64 {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        crate::hal::platform::ticks_to_ns(riscv::register::time::read() as u64)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        0
    }
}

/// Called by `set_current` with a frozen task: the invariant "a frozen task never runs" broke.
pub(super) fn note_frozen_ran(pid: Pid) {
    if FROZEN_RAN.fetch_add(1, Ordering::Relaxed) < 4 {
        log_error!(target: "task", "TASK: frozen task dispatched pid={}", pid.as_raw());
    }
}

impl TaskTable {
    /// Whether `pid` is a live child of `parent` (the authority check of every process order).
    pub fn is_live_child_of(&self, pid: Pid, parent: Pid) -> bool {
        pid != parent
            && self
                .task(pid)
                .is_some_and(|t| t.parent == Some(parent) && t.state != TaskState::Zombie)
    }

    /// Every live task sharing `leader`'s address space other than `leader` itself — the
    /// threads of its process (and, for a thread, its leader and siblings).
    pub fn members_of(&self, leader: Pid) -> Vec<Pid> {
        let Some(space) = self.task(leader).and_then(|t| t.address_space) else {
            return Vec::new();
        };
        self.tasks
            .iter()
            .filter(|t| {
                t.pid != leader && t.address_space == Some(space) && t.state != TaskState::Zombie
            })
            .map(|t| t.pid)
            .collect()
    }

    /// Whether the process `pid` belongs to still has a live leader (the task that created the
    /// space). A thread whose leader is gone has nobody to wait for it: its exit releases it.
    pub fn leader_alive_for(&self, pid: Pid) -> bool {
        let Some(space) = self.task(pid).and_then(|t| t.address_space) else {
            return false;
        };
        self.tasks
            .iter()
            .any(|t| !t.is_thread && t.address_space == Some(space) && t.state != TaskState::Zombie)
    }

    /// Leaves an order for a task running on another hart.
    pub fn mark_pending(&mut self, pid: Pid, order: Pending) {
        if let Some(task) = self.task_mut(pid) {
            // A kill is never downgraded to a freeze.
            if task.pending != Some(Pending::Kill) {
                task.pending = Some(order);
            }
        }
    }

    /// Takes the current task's pending order, if any (the trap entry's question).
    pub fn take_pending(&mut self, pid: Pid) -> Option<Pending> {
        self.task_mut(pid).and_then(|t| t.pending.take())
    }

    /// Records how many threads a leader's exit took (the reap's proof line).
    pub fn note_threads_taken(&mut self, pid: Pid, taken: usize) {
        if let Some(task) = self.task_mut(pid) {
            task.threads_taken = taken.min(u8::MAX as usize) as u8;
        }
    }

    /// Takes a task that is not running on any hart out of its block (a fence waiter is
    /// removed; IPC and waitset waiters go with the exit funnel's `remove_waiter_from_all`).
    pub fn detach_blocked(&mut self, pid: Pid, fences: &mut crate::fence::FenceTable) {
        let Some(task) = self.task_mut(pid) else { return };
        if let Some(super::BlockReason::Fence { fence_id, .. }) = task.block_reason {
            fences.remove_waiter(crate::fence::FenceId(fence_id), pid.as_raw());
        }
        task.clear_blocked();
    }

    /// Freezes one task that is not running on any hart: `Suspended` + `frozen`, out of every
    /// run queue; a block it holds stays (its wake only clears it).
    fn freeze_parked(&mut self, pid: Pid, scheduler: &mut Scheduler) {
        if let Some(task) = self.task_mut(pid) {
            task.state = TaskState::Suspended;
            task.frozen = true;
            task.frozen_at = now_ns();
        }
        scheduler.purge(pid);
    }

    /// The hart `pid` is running on right now, other than this one.
    fn running_elsewhere(&self, pid: Pid, scheduler: &Scheduler) -> Option<crate::types::CpuId> {
        scheduler.running_on(pid).filter(|cpu| *cpu != crate::smp::cpu_current_id())
    }

    /// Freezes the process `leader` belongs to: the leader and every thread. A task running on
    /// another hart gets a `Pending::Freeze` and a resched IPI. The receive endpoints of every
    /// member are marked frozen in `router` (a send that would park on them is refused).
    pub fn freeze_process(
        &mut self,
        leader: Pid,
        scheduler: &mut Scheduler,
        router: &mut crate::ipc::Router,
    ) -> FreezeOutcome {
        let mut out = FreezeOutcome::default();
        let mut members = self.members_of(leader);
        members.push(leader);
        for pid in members {
            let Some(task) = self.task(pid) else { continue };
            if task.state == TaskState::Zombie || task.pending == Some(Pending::Kill) {
                continue;
            }
            let mut recv = Vec::new();
            task.caps.endpoint_ids_with(crate::cap::Rights::RECV, &mut recv);
            for endpoint in recv {
                router.set_endpoint_frozen(endpoint, true);
            }
            if task.frozen {
                continue;
            }
            if let Some(cpu) = self.running_elsewhere(pid, scheduler) {
                self.mark_pending(pid, Pending::Freeze);
                let _ = crate::smp::request_resched(cpu);
                out.deferred += 1;
            } else {
                self.freeze_parked(pid, scheduler);
                out.frozen += 1;
            }
        }
        out
    }

    /// A pending freeze arrived at its hart: the current task parks (the trap re-executes on
    /// thaw, like `Reschedule`).
    pub fn freeze_current(&mut self, scheduler: &mut Scheduler) {
        let pid = self.current_pid();
        self.freeze_parked(pid, scheduler);
        scheduler.finish_current();
    }

    /// Thaws the process `leader` belongs to: the leader and every thread, their receive
    /// endpoints unmarked. Returns how many tasks were resumed.
    pub fn thaw_process(
        &mut self,
        leader: Pid,
        scheduler: &mut Scheduler,
        router: &mut crate::ipc::Router,
    ) -> Result<usize, ResumeOutcome> {
        let mut members = self.members_of(leader);
        members.push(leader);
        let mut resumed = 0usize;
        for pid in members {
            if let Some(task) = self.task(pid) {
                let mut recv = Vec::new();
                task.caps.endpoint_ids_with(crate::cap::Rights::RECV, &mut recv);
                for endpoint in recv {
                    router.set_endpoint_frozen(endpoint, false);
                }
            }
            match self.resume_task(pid, scheduler) {
                ResumeOutcome::Resumed => resumed += 1,
                ResumeOutcome::NotSuspended | ResumeOutcome::TaskNotFound => {}
                ResumeOutcome::EnqueueRejected => return Err(ResumeOutcome::EnqueueRejected),
            }
        }
        if resumed == 0 {
            return Err(ResumeOutcome::NotSuspended);
        }
        Ok(resumed)
    }

    /// Transitions a suspended task to Running and enqueues it (unless it is still blocked: its
    /// wake enqueues it). Idempotent: if already Running, returns NotSuspended. A pending
    /// freeze that never reached its hart is withdrawn here.
    pub fn resume_task(&mut self, pid: Pid, scheduler: &mut Scheduler) -> ResumeOutcome {
        let idx = pid.as_index();
        let Some(task) = self.tasks.get_mut(idx) else {
            return ResumeOutcome::TaskNotFound;
        };
        if task.pending == Some(Pending::Freeze) {
            task.pending = None;
            return ResumeOutcome::Resumed;
        }
        if task.state != TaskState::Suspended {
            return ResumeOutcome::NotSuspended;
        }
        let was_frozen = task.frozen;
        let frozen_at = task.frozen_at;
        task.frozen = false;
        task.state = TaskState::Running;
        if was_frozen {
            prove_freeze(pid, frozen_at);
        }
        if task.blocked {
            return ResumeOutcome::Resumed;
        }
        // A4/B: keep resume consistent with home CPU, affinity and online set.
        let home_cpu = clamp_home_to_affinity(
            task.affinity_mask,
            task.home_cpu,
            crate::smp::cpu_online_mask(),
        );
        match scheduler.enqueue_on_cpu(home_cpu, pid, task.qos) {
            crate::sched::EnqueueOutcome::Enqueued => {
                if home_cpu != crate::smp::cpu_current_id() {
                    let _ = crate::smp::request_resched(home_cpu);
                }
                ResumeOutcome::Resumed
            }
            crate::sched::EnqueueOutcome::Rejected(_) => {
                task.state = TaskState::Suspended;
                task.frozen = was_frozen;
                ResumeOutcome::EnqueueRejected
            }
        }
    }
}

/// The first thaw of a frozen task is the freeze proof: it was parked for `frozen_ns` and no
/// hart dispatched a frozen task meanwhile.
fn prove_freeze(pid: Pid, frozen_at: u64) {
    if FREEZE_PROVEN.swap(true, Ordering::AcqRel) {
        return;
    }
    let ran = FROZEN_RAN.load(Ordering::Relaxed);
    let frozen_ns = now_ns().saturating_sub(frozen_at);
    if ran == 0 {
        log_info!(
            target: "task",
            "KSELFTEST: task freeze ok (pid={} resumed=1 frozen_ns={} ran=0)",
            pid.as_raw(),
            frozen_ns
        );
    } else {
        log_error!(
            target: "task",
            "KSELFTEST: task freeze FAIL (pid={} ran={})",
            pid.as_raw(),
            ran
        );
    }
}
