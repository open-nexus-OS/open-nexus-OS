// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: process control of a CHILD (TASK-0333 6d, ADR-0082, RFC-0111): `task_resume` (32),
//! `task_suspend` (65) and `task_kill` (66). The authority is parentage, checked here for all
//! three — before 6d `task_resume` enqueued any pid. An order names a process: the child and
//! every thread of it. `task_kill` additionally needs the child's `killable` bit, which its
//! parent set at `exec_vmo` (apps yes, services no); the kernel's last resort (6e) honours
//! the same bit.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable (syscall numbers)
//! TEST_COVERAGE: `tests_process.rs` (`test_reject_resume_foreign_task`,
//!   `test_reject_suspend_non_child`, `test_reject_kill_non_child`, `test_reject_kill_unkillable`);
//!   QEMU `KSELFTEST: task freeze ok`, `SELFTEST: task kill ok`

use super::*;
use crate::task::{ExitReason, ResumeOutcome};

/// The child the order names, or `InvalidTarget` (ESRCH): not a live child of the caller.
fn child_of_caller(ctx: &Context<'_>, args: &Args) -> SysResult<task::Pid> {
    let pid = task::Pid::from_raw(args.get(0) as u32);
    if !ctx.tasks.is_live_child_of(pid, ctx.tasks.current_pid()) {
        return Err(Error::InvalidTarget);
    }
    Ok(pid)
}

/// Resumes (or thaws) a child process: the child and every thread of it.
pub(super) fn sys_task_resume(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let pid = child_of_caller(ctx, args)?;
    match ctx.tasks.thaw_process(pid, ctx.scheduler, ctx.router) {
        Ok(resumed) => Ok(resumed),
        Err(ResumeOutcome::EnqueueRejected) => Err(Error::RunQueueFull),
        Err(_) => Err(Error::InvalidTarget),
    }
}

/// Freezes a child process: the child and every thread of it. Returns how many tasks parked
/// here (a running one parks at its hart's next trap entry). Idempotent.
pub(super) fn sys_task_suspend(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let pid = child_of_caller(ctx, args)?;
    let outcome = ctx.tasks.freeze_process(pid, ctx.scheduler, ctx.router);
    Ok(outcome.frozen + outcome.deferred)
}

/// Kills a child process (`ExitReason::Killed`, code -9): refused unless the child is
/// `killable`. The threads go with the leader; the parent hears through its child-death
/// endpoint and reaps the zombie like any other.
pub(super) fn sys_task_kill(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let pid = child_of_caller(ctx, args)?;
    if !ctx.tasks.task(pid).is_some_and(|t| t.killable()) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let tables = ExitTables {
        hart_timers: ctx.hart_timers,
        waitsets: ctx.waitsets,
        fences: ctx.fences,
        address_spaces: ctx.address_spaces,
    };
    match super::process_kill::kill_task(
        ctx.tasks,
        ctx.router,
        ctx.scheduler,
        tables,
        pid,
        -9,
        ExitReason::Killed,
    ) {
        super::process_kill::KillOutcome::Done => Ok(0),
        super::process_kill::KillOutcome::Deferred => Ok(1),
        super::process_kill::KillOutcome::Gone => Err(Error::InvalidTarget),
    }
}
