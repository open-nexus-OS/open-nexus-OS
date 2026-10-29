// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the swap stage's syscalls (RFC-0100 §5, TASK-0333 6f-2, ADR-0088) — `swap_key` (70):
//! the RNG service seeds the boot's key, once; `swap_export` (71), `swap_commit` (72) and
//! `swap_page_in` (73): the pager — the pressure sink's holder, memd — moves sealed records out
//! and back. And the park of a syscall that touched a swapped page (`park_caller`, from the
//! syscall entry): the caller blocks on the page's token, the pager is asked once per page, and
//! the syscall re-executes when the page is back. User memory goes through the caller's page
//! table (`user_copy`): the boot proof drives these from the kernel's address space.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-2)
//! API_STABILITY: Stable (syscall numbers); the export buffer is versioned (`lazy::swap`)
//! TEST_COVERAGE: `KSELFTEST: swap ok (…)` — the refusals (not the RNG service, a second key, no
//!   key, not the sink holder, a stale commit, a tampered and a replayed record), the export, the
//!   commit, the park and its one request, the page-in byte-exact and the wake, the kill of a
//!   thread parked on a record that does not open, the drop at the object's death
//! INVARIANTS: identity is the kernel's (`service_id`, the sink binder's pid), never a payload;
//!   the key is wiped from the kernel stack once installed and never logged; a parked thread is
//!   woken or killed when its request is answered, and only then.

use super::user_copy::{read_user, write_user};
use super::*;
use crate::lazy::seal::{KEY_LEN, MAX_SEALED};
use crate::lazy::swap as wire;
use crate::mm::swap::{ExportError, PageInError};
use crate::task::ExitReason;

/// The synthetic fault cause of a thread whose swapped page could not come back (ADR-0056's
/// range above the trap causes; `0xF1` is an ecall from an unmapped pc).
const PAGE_IN_FAULT: u8 = 0xF2;

/// The pressure sink's holder — the pager (ADR-0086/0088) — or `EPERM`.
fn pager(ctx: &Context<'_>) -> SysResult<()> {
    if crate::mm::pressure_endpoint::holder() == Some(ctx.tasks.current_pid()) {
        Ok(())
    } else {
        Err(Error::Capability(CapError::PermissionDenied))
    }
}

/// Overwrite a secret on the kernel stack (volatile: the stores are never elided).
fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: a valid, aligned, exclusive `u8` of this slice.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
}

/// `SYSCALL_SWAP_KEY` (70). Args: (key_ptr, len = 32). `EPERM`: not the RNG service; `EBUSY`:
/// the boot has its key.
pub(super) fn sys_swap_key(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    if ctx.tasks.current_service_id() != super::sched_task::service_id_from_name(b"rngd") {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let (ptr, len) = (args.get(0), args.get(1));
    if len != KEY_LEN {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    ensure_user_range(ctx, ptr, KEY_LEN, UserAccess::Read)?;
    let mut key = [0u8; KEY_LEN];
    let read = read_user(ctx, ptr, &mut key);
    let installed = read.map(|()| crate::mm::swap::install_key(&key));
    wipe(&mut key);
    installed?.map_err(|_| Error::ResourceBusy)?;
    Ok(0)
}

/// `SYSCALL_SWAP_EXPORT` (71). Args: (pid, from_va, buf_ptr, buf_len) → the next va (`0`: done).
/// `EPERM`: not the pager; `ESRCH`: no such live process, or the pager itself (it would wait on
/// its own answer); `EINVAL`: no key, `from_va` unaligned, the buffer under one entry.
pub(super) fn sys_swap_export(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    pager(ctx)?;
    let pid = task::Pid::from_raw(args.get(0) as u32);
    let (from_va, buf_ptr) = (args.get(1), args.get(2));
    let buf_len = args.get(3).min(wire::EXPORT_MAX_BUF);
    if from_va % PAGE_SIZE != 0 || buf_len < wire::EXPORT_MIN_BUF {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    if pid == ctx.tasks.current_pid() {
        return Err(Error::InvalidTarget);
    }
    ensure_user_range(ctx, buf_ptr, buf_len, UserAccess::Write)?;
    let handle = ctx
        .tasks
        .task(pid)
        .filter(|t| t.state() != task::TaskState::Zombie)
        .and_then(|t| t.address_space())
        .ok_or(Error::InvalidTarget)?;
    let allowed = super::process_ctl::squeezable(ctx, handle)?;
    let ctx: &Context<'_> = ctx;
    let space = ctx.address_spaces.get(handle)?;
    let mut emit = |at: usize, bytes: &[u8]| write_user(ctx, buf_ptr + at, bytes).is_ok();
    let done = crate::mm::swap::export(space, from_va, &allowed, buf_len, &mut emit).map_err(
        |e| match e {
            ExportError::NoKey | ExportError::Short => Error::from(AddressSpaceError::InvalidArgs),
            ExportError::Fault => Error::from(AddressSpaceError::BadUserMemory),
        },
    )?;
    let mut header = [0u8; wire::EXPORT_HEADER];
    wire::write_header(&mut header, done.records).ok_or(AddressSpaceError::InvalidArgs)?;
    write_user(ctx, buf_ptr, &header)?;
    Ok(done.next)
}

/// `SYSCALL_SWAP_COMMIT` (72). Args: (token, ctr, slot). `ESRCH`: the export is not the page's
/// (a touch cancelled it, a later export superseded it, the object is gone).
pub(super) fn sys_swap_commit(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    pager(ctx)?;
    let (token, ctr) = (args.get(0) as u64, args.get(1) as u64);
    let slot = u32::try_from(args.get(2)).map_err(|_| AddressSpaceError::InvalidArgs)?;
    let (id, index) = wire::token_parts(token);
    crate::mm::vmo::swap::commit(id, index as usize, ctr, slot)
        .map_err(|_| Error::InvalidTarget)?;
    Ok(0)
}

/// `SYSCALL_SWAP_PAGE_IN` (73). Args: (token, slot, sealed_ptr, len). The threads parked on the
/// page are woken once it is back (or stale: `ESRCH` — they retry and find what is there now);
/// a record that does not open or decode is refused (`EINVAL`) and the threads parked on it are
/// killed — the page cannot come back. `ENOMEM`: no frame now, the request stands.
pub(super) fn sys_swap_page_in(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    pager(ctx)?;
    let token = args.get(0) as u64;
    let slot = u32::try_from(args.get(1)).map_err(|_| AddressSpaceError::InvalidArgs)?;
    let (ptr, len) = (args.get(2), args.get(3));
    if len > MAX_SEALED {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    ensure_user_range(ctx, ptr, len, UserAccess::Read)?;
    let result = {
        let ctx: &Context<'_> = ctx;
        crate::mm::swap::page_in(token, slot, len, |buf| read_user(ctx, ptr, buf).is_ok())
    };
    match result {
        Ok(()) => {
            crate::mm::swap::answered(token);
            crate::mm::swap::wake_parked(ctx.tasks, ctx.scheduler, token);
            Ok(0)
        }
        Err(PageInError::Stale) => {
            crate::mm::swap::answered(token);
            crate::mm::swap::wake_parked(ctx.tasks, ctx.scheduler, token);
            Err(Error::InvalidTarget)
        }
        Err(PageInError::Forged | PageInError::Damaged) => {
            crate::mm::swap::answered(token);
            let killed = kill_parked(ctx, token);
            if killed > 0 {
                let (id, index) = wire::token_parts(token);
                log_error!(
                    target: "mm",
                    "MM: page-in refused (vmo={} page={} killed={}) record did not open",
                    id,
                    index,
                    killed
                );
            }
            Err(AddressSpaceError::InvalidArgs.into())
        }
        Err(PageInError::Exhausted) => Err(AddressSpaceError::OutOfFrames.into()),
        Err(PageInError::NoKey) => Err(AddressSpaceError::InvalidArgs.into()),
        Err(PageInError::Fault) => Err(AddressSpaceError::BadUserMemory.into()),
    }
}

/// Kill every thread parked on `token`: its page cannot come back. Answers how many.
fn kill_parked(ctx: &mut Context<'_>, token: u64) -> usize {
    let parked = crate::mm::swap::parked_on(ctx.tasks, token);
    for &pid in &parked {
        let tables = ExitTables {
            hart_timers: ctx.hart_timers,
            waitsets: ctx.waitsets,
            fences: ctx.fences,
            address_spaces: ctx.address_spaces,
        };
        let _ = super::process_kill::kill_task(
            ctx.tasks,
            ctx.router,
            ctx.scheduler,
            tables,
            pid,
            -11,
            ExitReason::Fault { cause: PAGE_IN_FAULT },
        );
    }
    parked.len()
}

/// A syscall touched swapped page `token` (ADR-0088): the caller parks until the pager brings it
/// back from `slot` — the syscall entry does not advance `sepc`, the syscall re-executes then,
/// like `Reschedule`. Without a pager the page cannot come back: `EFAULT`.
pub(crate) fn park_caller(ctx: &mut Context<'_>, token: u64, slot: u32) -> Error {
    if !crate::mm::swap::ask(token, slot) {
        return AddressSpaceError::BadUserMemory.into();
    }
    let cur = ctx.tasks.current_pid();
    ctx.tasks.block_current(BlockReason::PageIn { token }, ctx.scheduler);
    if let Some(next) = ctx.scheduler.schedule_next() {
        ctx.tasks.set_current(next);
        return Error::Reschedule;
    }
    // Nothing else runnable on this hart: park it (pre-runtime, the caller retries at once).
    park_hart_or_self_wake(ctx, cur, |_| {})
}
