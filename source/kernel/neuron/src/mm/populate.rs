// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memory committed on first touch, executed (TASK-0333 6b-1, ADR-0077, RFC-0100 §2) —
//! the fill behind the user page fault and behind the checked range every syscall passes before
//! it touches user memory. The decisions are `crate::lazy`'s (host-proven); this module reads the
//! facts — the space's region record and its page table — and carries out the verdict: the
//! object commits the frame (`vmo::commit`, zeroed before it is entered), a 4 KiB PTE is written,
//! this hart's TLB is fenced for the page.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the verdicts in `crate::lazy`'s tests; the path by every boot (every `.bss`
//!   tail and every stack is lazy) and `SELFTEST: vm commit on touch ok (…)`
//! INVARIANTS: called with the kernel lock held; a page is mapped only after its frame is
//!   zeroed and entered; a page never takes a second frame; a lazy page is never executable.

use super::address_space::AddressSpace;
use super::page_table::{PageFlags, PAGE_SIZE};
use crate::lazy::{classify, page_index, Access, RegionView, Rights, Verdict, Violation};

/// Why a page could not be made accessible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillError {
    /// Not a missing page: no region, no right, an eager region — the fault kills, the range is
    /// `EFAULT`.
    Violation(Violation),
    /// The pool had no frame for the page.
    Exhausted,
}

/// Pages filled since boot (a fault or a kernel range that committed and mapped a page).
static FILLED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Pages filled since boot.
pub fn filled() -> usize {
    FILLED.load(core::sync::atomic::Ordering::Relaxed)
}

fn rights(flags: PageFlags) -> Rights {
    Rights {
        read: flags.contains(PageFlags::READ),
        write: flags.contains(PageFlags::WRITE),
        execute: flags.contains(PageFlags::EXECUTE),
    }
}

/// Make the page at `va` of `space` accessible from user mode for `access`: a lazy page is
/// committed and mapped, a mapped page is checked, anything else is refused. The caller holds
/// the kernel lock.
pub fn fill(space: &mut AddressSpace, va: usize, access: Access) -> Result<(), FillError> {
    let page = va & !(PAGE_SIZE - 1);
    let user = PageFlags::VALID | PageFlags::USER;
    let pte = space.page_table().leaf_flags(page).ok().filter(|f| f.contains(user)).map(rights);
    let region = space.va_space().find(page);
    let view = region.map(|r| RegionView {
        rights: rights(PageFlags::from_bits_truncate(r.flags)),
        lazy: r.lazy,
    });
    match classify(view, pte, access) {
        Verdict::Spurious => {
            fence_page(space, page);
            Ok(())
        }
        Verdict::Violation(why) => Err(FillError::Violation(why)),
        Verdict::Populate => {
            let Some(region) = region else {
                return Err(FillError::Violation(Violation::NoRegion));
            };
            let index = page_index(region.va, region.offset, page);
            let pa = super::vmo::commit(region.vmo, index).map_err(|e| match e {
                super::vmo::VmoError::Exhausted => FillError::Exhausted,
                _ => FillError::Violation(Violation::NoRegion),
            })?;
            let flags = PageFlags::from_bits_truncate(region.flags) | user;
            space
                .page_table_mut()
                .map(page, pa, flags)
                .map_err(|_| FillError::Violation(Violation::EagerRegion))?;
            fence_page(space, page);
            FILLED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
    }
}

/// Make every page of `[ptr, ptr + len)` accessible for `access` (the syscalls' checked range).
pub fn fill_range(
    space: &mut AddressSpace,
    ptr: usize,
    len: usize,
    access: Access,
) -> Result<(), FillError> {
    let Some(end) = ptr.checked_add(len) else {
        return Err(FillError::Violation(Violation::NoRegion));
    };
    let mut page = ptr & !(PAGE_SIZE - 1);
    while page < end {
        fill(space, page, access)?;
        page += PAGE_SIZE;
    }
    Ok(())
}

/// `sfence.vma` for one page of one address space, on this hart: a PTE that went from invalid
/// to valid may still be cached as a miss here (another hart that cached it takes a spurious
/// fault and fences itself).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn fence_page(space: &AddressSpace, page: usize) {
    let asid = space.asid().as_raw() as usize;
    // SAFETY: a TLB fence scoped to one page of one ASID; no memory or register side effects.
    unsafe {
        core::arch::asm!(
            "sfence.vma {0}, {1}",
            in(reg) page,
            in(reg) asid,
            options(nostack, preserves_flags)
        );
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn fence_page(_space: &AddressSpace, _page: usize) {}
