// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: process control of a CHILD (TASK-0333 6d, ADR-0082, RFC-0111): `task_resume` (32),
//! `task_suspend` (65), `task_kill` (66) and `task_squeeze` (69, 6f-1, ADR-0087). The authority
//! is parentage, checked here for all four — before 6d `task_resume` enqueued any pid. An order
//! names a process: the child and every thread of it. `task_kill` additionally needs the child's
//! `killable` bit, which its parent set at `exec_vmo` (apps yes, services no); the kernel's last
//! resort (6e) honours the same bit. `task_squeeze` needs the child parked (every task frozen
//! and off every hart) and takes only the objects that are the child's alone.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable (syscall numbers)
//! TEST_COVERAGE: `tests_process.rs` (`test_reject_resume_foreign_task`,
//!   `test_reject_suspend_non_child`, `test_reject_kill_non_child`, `test_reject_kill_unkillable`);
//!   QEMU `KSELFTEST: task freeze ok`, `SELFTEST: task kill ok`, `KSELFTEST: task squeeze ok`
//!   (the squeeze's refusals: not a child, not parked, a shared or device-bound object)

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

/// Squeezes one batch of a parked child's private lazy pages (ADR-0087, RFC-0100 §5): pages of
/// zeroes dropped, the rest recorded compressed, a touch brings them back. Args: (pid, from_va,
/// out_ptr, out_len) → the next va (`0`: done); the batch record at `out_ptr`. `ESRCH`: not a
/// live child; `EBUSY`: a task of the child is not parked; `EINVAL`: `from_va` unaligned or the
/// record buffer short; `EFAULT`: the buffer is not the caller's memory.
pub(super) fn sys_task_squeeze(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    use crate::mm::squeeze::{Batch, BATCH_BYTES};
    let pid = child_of_caller(ctx, args)?;
    let (from_va, out_ptr, out_len) = (args.get(1), args.get(2), args.get(3));
    if from_va % PAGE_SIZE != 0 || out_len < BATCH_BYTES {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    ensure_user_range(ctx, out_ptr, BATCH_BYTES, UserAccess::Write)?;
    if !ctx.tasks.process_parked(pid, ctx.scheduler) {
        return Err(Error::ResourceBusy);
    }
    let handle = ctx.tasks.task(pid).and_then(|t| t.address_space()).ok_or(Error::InvalidTarget)?;
    let allowed = squeezable(ctx, handle)?;
    let mut batch = Batch::default();
    let space = ctx.address_spaces.get_mut(handle)?;
    let next = crate::mm::squeeze::run(space, from_va, &allowed, &mut batch);
    // Through the caller's page table: the boot proof drives this from the kernel's own space.
    super::user_copy::write_user(ctx, out_ptr, &batch.encode())?;
    Ok(next)
}

/// The objects of `handle`'s space a squeeze (and the swap stage's export, ADR-0088) may take
/// (`vmo_lifetime::squeeze_allowed`): the objects of its lazy regions, each mapped by one region
/// of its space and by no other space, named by no capability outside its process.
pub(super) fn squeezable(
    ctx: &Context<'_>,
    handle: crate::mm::AsHandle,
) -> SysResult<alloc::vec::Vec<u32>> {
    let regions = ctx.address_spaces.get(handle)?.va_space().regions();
    let mut allowed = alloc::vec::Vec::new();
    for region in regions.iter().filter(|r| r.lazy) {
        let id = region.vmo;
        if allowed.contains(&id) {
            continue;
        }
        let here = regions.iter().filter(|r| r.vmo == id).count();
        let elsewhere = crate::mm::vm_ops::spaces_mapping_vmo(ctx.address_spaces, id) > 1;
        let named = ctx.tasks.vmo_named_outside(handle, id);
        let facts = crate::mm::vmo::squeeze::facts(id, here, elsewhere, named);
        if facts.is_some_and(|f| crate::vmo_lifetime::squeeze_allowed(&f).is_ok()) {
            allowed.push(id);
        }
    }
    Ok(allowed)
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
