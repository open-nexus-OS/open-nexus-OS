// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: killing a task that is NOT the current one (TASK-0333 6d, ADR-0082, RFC-0111): the
//! parent's `task_kill` of a child, and a process leader's exit taking its threads. A task no
//! hart runs is taken out of its block and its run queue and sent through the one exit funnel
//! here; a task running on another hart gets `Pending::Kill` and a resched IPI, and that hart
//! runs the funnel at its next trap entry (`trap::pending`). The decision is `TaskTable::park`:
//! the target is parked (no wake starts it again), or in user mode (the IPI lands), or in a
//! syscall — at a trap entry the pending mark is seen first thing, and one going to sleep off
//! the kernel lock reads it before it sleeps or is woken by it (TASK-0330 P2).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: process exit takes threads ok`; execd's kill probe
//!   (`SELFTEST: task kill ok`); `tests_process.rs` (the refusals)

use super::*;
use crate::task::{ExitReason, Pending, Pid, TaskTable};

/// Where a kill order went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillOutcome {
    /// The funnel ran here: the task is a zombie (or released, for an orphan thread).
    Done,
    /// The task runs on another hart; it exits at its next trap entry.
    Deferred,
    /// Nothing to do: already a zombie, or no such task.
    Gone,
}

/// Kills `target`, which must not be the current task.
pub(crate) fn kill_task(
    tasks: &mut TaskTable,
    router: &crate::ipc::Router,
    scheduler: &mut Scheduler,
    tables: ExitTables<'_>,
    target: Pid,
    status: i32,
    reason: ExitReason,
) -> KillOutcome {
    let Some(task) = tasks.task(target) else { return KillOutcome::Gone };
    if task.state() == crate::task::TaskState::Zombie || target == tasks.current_pid() {
        return KillOutcome::Gone;
    }
    // Parked — off its queue, and no wake starts it again — or left to the hart that runs it
    // (`TaskTable::park`: a dispatch without the kernel lock never starts a task reaped here).
    if let Some(cpu) = TaskTable::park(target, scheduler) {
        tasks.mark_pending(target, Pending::Kill, scheduler);
        let _ = crate::smp::request_resched(cpu);
        return KillOutcome::Deferred;
    }
    // Not running anywhere: out of its block and its queue, then the funnel.
    tasks.detach_blocked(target, tables.fences);
    scheduler.purge(target);
    super::exit_task_and_release(tasks, router, scheduler, tables, target, status, reason);
    KillOutcome::Done
}

/// A leader died: every thread of its process is killed. Returns how many were taken (parked
/// ones here, running ones by their hart).
pub(crate) fn kill_members(
    tasks: &mut TaskTable,
    router: &crate::ipc::Router,
    scheduler: &mut Scheduler,
    mut tables: ExitTables<'_>,
    leader: Pid,
) -> usize {
    let members = tasks.members_of(leader);
    let mut taken = 0usize;
    for member in members {
        match kill_task(tasks, router, scheduler, tables.reborrow(), member, -9, ExitReason::Killed)
        {
            KillOutcome::Done | KillOutcome::Deferred => taken += 1,
            KillOutcome::Gone => {}
        }
    }
    taken
}
