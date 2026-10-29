// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: bytes into and out of the CALLER's memory through its own page table and the direct
//! map (TASK-0333 6f-1/6f-2) — for the syscalls the boot proof drives from the kernel's address
//! space (`task_squeeze`, the swap stage's): a raw user pointer is only valid under the caller's
//! `satp`, a translation is valid under any. The range must have passed `ensure_user_range` with
//! the access first (mapped, committed, the right granted); a page without a translation is
//! `EFAULT`, never a kernel fault.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: task squeeze ok (…)` (the batch record), `KSELFTEST: swap ok (…)`
//!   (the key read, the export buffer written, a sealed record read)

use super::*;

/// The physical pieces of `[ptr, ptr + len)` of the caller's memory, page by page: `f(pa, at, n)`
/// for each piece (`at` its offset in the range).
fn each_piece(
    ctx: &Context<'_>,
    ptr: usize,
    len: usize,
    mut f: impl FnMut(usize, usize, usize),
) -> SysResult<()> {
    let handle =
        ctx.tasks.current_task().address_space().ok_or(AddressSpaceError::InvalidHandle)?;
    let table = ctx.address_spaces.get(handle)?.page_table();
    let mut done = 0;
    while done < len {
        let va = ptr.checked_add(done).ok_or(AddressSpaceError::BadUserMemory)?;
        let n = (PAGE_SIZE - va % PAGE_SIZE).min(len - done);
        let pa = table.translate(va).ok_or(AddressSpaceError::BadUserMemory)?;
        f(pa, done, n);
        done += n;
    }
    Ok(())
}

/// `bytes` into the caller's memory at `ptr` (checked writable first).
pub(super) fn write_user(ctx: &Context<'_>, ptr: usize, bytes: &[u8]) -> SysResult<()> {
    each_piece(ctx, ptr, bytes.len(), |pa, at, n| {
        // SAFETY: `pa` backs the caller's page at `ptr + at`, checked mapped and writable; the
        // direct map reaches every frame; the `n` bytes stay inside that page.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes[at..].as_ptr(),
                crate::phys::phys_to_virt(pa) as *mut u8,
                n,
            );
        }
    })
}

/// The caller's memory at `ptr` into `out` (checked readable first).
pub(super) fn read_user(ctx: &Context<'_>, ptr: usize, out: &mut [u8]) -> SysResult<()> {
    let len = out.len();
    each_piece(ctx, ptr, len, |pa, at, n| {
        // SAFETY: as `write_user` — a frame backing the caller's readable page, `n` bytes inside
        // it, copied into this kernel buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(
                crate::phys::phys_to_virt(pa) as *const u8,
                out[at..].as_mut_ptr(),
                n,
            );
        }
    })
}
