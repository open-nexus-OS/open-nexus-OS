// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: bytes into and out of the CALLER's memory through its own page table and the direct
//! map (TASK-0333 6f-1/6f-2) — for the syscalls the boot proof drives from the kernel's address
//! space (`task_squeeze`, the swap stage's): a raw user pointer is only valid under the caller's
//! `satp`, a translation is valid under any. The walk starts at the root the caller's scheduling
//! record names and takes no lock (`mm::user_copy`, ADR-0102) — so it may run while another
//! space is held (the swap export writes the caller's buffer while it reads the parked
//! process's pages; a second space's lock would be the one thing the order forbids). The range
//! must have passed `ensure_user_range` with the access first (mapped, committed, the right
//! granted); a page without a usable translation is `EFAULT`, never a kernel fault.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: task squeeze ok (…)` (the batch record), `KSELFTEST: swap ok (…)`
//!   (the key read, the export buffer written, a sealed record read)

use super::*;

/// `bytes` into the caller's memory at `ptr` (checked writable first).
pub(super) fn write_user(ctx: &Context<'_>, ptr: usize, bytes: &[u8]) -> SysResult<()> {
    if ctx.ipc()?.write(ptr, bytes) {
        Ok(())
    } else {
        Err(AddressSpaceError::BadUserMemory.into())
    }
}

/// The caller's memory at `ptr` into `out` (checked readable first).
pub(super) fn read_user(ctx: &Context<'_>, ptr: usize, out: &mut [u8]) -> SysResult<()> {
    if ctx.ipc()?.read(ptr, out) {
        Ok(())
    } else {
        Err(AddressSpaceError::BadUserMemory.into())
    }
}
