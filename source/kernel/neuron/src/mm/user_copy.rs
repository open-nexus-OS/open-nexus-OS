// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: user memory copied without a lock (TASK-0330 P2, ADR-0102). A syscall that left
//! the kernel lock cannot ask the address space's manager whether a range is mapped — and a
//! copy through the MMU would fault in the kernel when a thread of the same process unmaps the
//! page on another hart. So the copy translates itself (`user_walk`, from the root the task's
//! record names) and moves the bytes through the direct map, piece by piece, inside one
//! stretch in which this hart acknowledges no shootdown (`tlb::NoAck`): an unmap clears,
//! waits for every hart's acknowledgement, then frees — the frames and table pages read here
//! stay what they were until the stretch ends. A range the walk cannot vouch for (a lazy page
//! not yet filled, a right missing, a device window) is a `Miss`: the syscall runs again under
//! the kernel lock, where the page is filled or the violation named. Bytes copied before a
//! miss stay: the path under the lock copies the whole range again.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `user_walk` on the host (the walk and its refusals); `KSELFTEST: user copy ok`
//!   on every QEMU boot (bytes land in the frame a table names, across a page boundary; a
//!   read-only page, a kernel-only page, an unmapped page and a device window are refused)
//! INVARIANTS: no lock is taken and no shootdown ordered inside a copy; only frames inside a
//!   memory bank are touched; nothing is written through a page without WRITE.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use crate::user_walk::{self, Access, Piece};

pub use crate::user_walk::Miss;

/// Entry `index` of the table page at `table_pa`, through the direct map.
fn entry(table_pa: usize, index: usize) -> usize {
    if !in_ram(table_pa, super::PAGE_SIZE) {
        // Not a table of ours: an invalid entry ends the walk.
        return 0;
    }
    let table = crate::phys::phys_to_virt(table_pa) as *const usize;
    // SAFETY: a page-table page inside a bank, reached through the direct map; it is not freed
    // while this hart acknowledges no shootdown (`tlb::NoAck`, held by every caller). One word,
    // read once: a writer on another hart stores whole entries.
    unsafe { core::ptr::read_volatile(table.add(index)) }
}

/// `pa..pa + len` lies inside one memory bank (the direct map covers it; a device window and
/// RAM the tree does not name are not the kernel's to copy through).
fn in_ram(pa: usize, len: usize) -> bool {
    crate::hal::platform::memory_banks()
        .any(|(base, size)| pa >= base && pa - base <= size && len <= size - (pa - base))
}

/// The frame bytes of `piece`, through the direct map.
fn frame(piece: Piece) -> Result<*mut u8, Miss> {
    if !in_ram(piece.pa, piece.len) {
        return Err(Miss);
    }
    Ok(crate::phys::phys_to_virt(piece.pa) as *mut u8)
}

/// Copies the user range `va..va + dst.len()` of the space rooted at `root_pa` into `dst`.
pub fn copy_in(root_pa: usize, va: usize, dst: &mut [u8]) -> Result<(), Miss> {
    // SAFETY: `dst` is the caller's own, for exactly its length.
    unsafe { copy_in_raw(root_pa, va, dst.as_mut_ptr(), dst.len()) }
}

/// Copies `src` to the user range `va..va + src.len()` of the space rooted at `root_pa`.
pub fn copy_out(root_pa: usize, va: usize, src: &[u8]) -> Result<(), Miss> {
    // SAFETY: `src` is the caller's own, for exactly its length.
    unsafe { copy_out_raw(root_pa, va, src.as_ptr(), src.len()) }
}

/// [`copy_in`] to `len` bytes at `to` that are no Rust object's: a memory object's frame
/// through the direct map, which a mapping of it may read or write meanwhile.
///
/// # Safety
/// `to..to + len` is memory the caller may write for the call's length (frames it holds).
pub unsafe fn copy_in_raw(root_pa: usize, va: usize, to: *mut u8, len: usize) -> Result<(), Miss> {
    let _stretch = crate::smp::tlb::NoAck::begin();
    user_walk::pieces(root_pa, va, len, Access::Read, entry, |piece| {
        let from = frame(piece)?;
        // SAFETY: `from` is `piece.len` bytes of a user-readable frame inside a bank, alive for
        // the stretch; `to + offset` stays inside the caller's `len` bytes (the walk's pieces
        // partition them).
        unsafe { core::ptr::copy_nonoverlapping(from, to.add(piece.offset), piece.len) };
        Ok(())
    })
}

/// [`copy_out`] from `len` bytes at `from` that are no Rust object's (as [`copy_in_raw`]).
///
/// # Safety
/// `from..from + len` is memory the caller may read for the call's length (frames it holds).
pub unsafe fn copy_out_raw(
    root_pa: usize,
    va: usize,
    from: *const u8,
    len: usize,
) -> Result<(), Miss> {
    let _stretch = crate::smp::tlb::NoAck::begin();
    user_walk::pieces(root_pa, va, len, Access::Write, entry, |piece| {
        let to = frame(piece)?;
        // SAFETY: `to` is `piece.len` bytes of a user-writable frame inside a bank, alive for
        // the stretch; `from + offset` stays inside the caller's `len` bytes.
        unsafe { core::ptr::copy_nonoverlapping(from.add(piece.offset), to, piece.len) };
        Ok(())
    })
}
