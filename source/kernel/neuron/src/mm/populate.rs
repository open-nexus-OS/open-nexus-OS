// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memory committed on first touch, executed (TASK-0333 6b-1, ADR-0077, RFC-0100 §2) —
//! the fill behind the user page fault and behind the checked range every syscall passes before
//! it touches user memory. The decisions are `crate::lazy`'s (host-proven); this module reads the
//! facts — the space's region record and its page table — and carries out the verdict: the
//! object commits the frame (`vmo::commit`, zeroed before it is entered), a 4 KiB PTE is written,
//! this hart's TLB is fenced for the page. A swapped page (ADR-0088) is not filled here: the
//! verdict is `PageIn`, and the caller parks the thread until the pager brings the page back.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the verdicts in `crate::lazy`'s tests; the path by every boot (every `.bss`
//!   tail and every stack is lazy) and `SELFTEST: vm commit on touch ok (…)`
//! INVARIANTS: a page table is written only with its space held; a page is mapped only after
//!   its frame is zeroed and entered, to the frame the object names at that moment; a page
//!   never takes a second frame; a lazy page is never executable; no frame is zeroed with a
//!   space held on the fault's own path (`fill_free`).

use super::address_space::{AddressSpace, AddressSpaceManager, AsHandle};
use super::page_table::{PageFlags, PAGE_SIZE};
use crate::lazy::{classify, page_index, Access, RegionView, Rights, Verdict, Violation};
use crate::va_space::VaRegion;

/// Why a page could not be made accessible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillError {
    /// Not a missing page: no region, no right, an eager region — the fault kills, the range is
    /// `EFAULT`.
    Violation(Violation),
    /// The pool had no frame for the page.
    Exhausted,
    /// The page was squeezed and its record did not decode (ADR-0087): never mapped — the fault
    /// kills, the range is `EFAULT`.
    Damaged,
    /// The page is swapped (ADR-0088): the fault or the syscall parks on `token` until the pager
    /// brings it back from `slot`, then re-executes.
    PageIn { token: u64, slot: u32 },
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

const USER: PageFlags = PageFlags::VALID.union(PageFlags::USER);

/// What the page at `page` of `space` needs for `access`: nothing (`None` — it is mapped, this
/// hart's stale miss fenced), or page `index` of the region's object committed and mapped.
fn look(
    space: &AddressSpace,
    page: usize,
    access: Access,
) -> Result<Option<(VaRegion, usize)>, FillError> {
    let pte = space.page_table().leaf_flags(page).ok().filter(|f| f.contains(USER)).map(rights);
    let region = space.va_space().find(page);
    let view = region.map(|r| RegionView {
        rights: rights(PageFlags::from_bits_truncate(r.flags)),
        // A region an unmap is taking away is filled no more (`VaSpace::begin_unmap`).
        lazy: r.lazy && !r.going,
    });
    match classify(view, pte, access) {
        Verdict::Spurious => {
            fence_page(space, page);
            Ok(None)
        }
        Verdict::Violation(why) => Err(FillError::Violation(why)),
        Verdict::Populate => match region {
            Some(region) => Ok(Some((region, page_index(region.va, region.offset, page)))),
            None => Err(FillError::Violation(Violation::NoRegion)),
        },
    }
}

/// Maps `page` of `space` to the committed frame `pa` with `region`'s rights and fences it.
fn install(
    space: &mut AddressSpace,
    page: usize,
    region: &VaRegion,
    pa: usize,
) -> Result<(), FillError> {
    let flags = PageFlags::from_bits_truncate(region.flags) | USER;
    // ADR-0091: no frame for a table page is the fault's out-of-memory (the same kill as an
    // exhausted commit), never a kernel panic.
    space.page_table_mut().map(page, pa, flags).map_err(|e| match e {
        super::MapError::OutOfMemory => FillError::Exhausted,
        _ => FillError::Violation(Violation::EagerRegion),
    })?;
    fence_page(space, page);
    FILLED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Make the page at `va` of `space` accessible from user mode for `access`: a lazy page is
/// committed and mapped, a mapped page is checked, anything else is refused. The space is held
/// by the caller for the whole fill — the frame is zeroed under it; a fault takes
/// [`fill_free`] first.
pub fn fill(space: &mut AddressSpace, va: usize, access: Access) -> Result<(), FillError> {
    let page = va & !(PAGE_SIZE - 1);
    let Some((region, index)) = look(space, page, access)? else { return Ok(()) };
    let pa = super::vmo::commit(region.vmo, index).map_err(|e| match e {
        super::vmo::VmoError::Exhausted => FillError::Exhausted,
        super::vmo::VmoError::Damaged => FillError::Damaged,
        super::vmo::VmoError::Swapped { token, slot } => FillError::PageIn { token, slot },
        _ => FillError::Violation(Violation::NoRegion),
    })?;
    install(space, page, &region, pa)
}

/// [`fill`] for a hart that holds no lock (TASK-0330 P3): the space `handle` names is held to
/// look and again to install, and not while the frame is taken and zeroed — eight threads of
/// one process touching fresh pages zero them side by side. What was looked at may have moved
/// on while the space was free, so the install believes nothing: the region is found again and
/// must be the same, and the object is asked again which frame the page has (the one this hart
/// committed, or the one another thread did first). `true`: the page is accessible. `false`:
/// nothing the caller must undo — it takes the kernel lock and fills from the start, which is
/// where a refusal is reported, a squeezed page inflated and a swapped one waited for.
pub fn fill_free(
    spaces: &AddressSpaceManager,
    handle: AsHandle,
    va: usize,
    access: Access,
) -> bool {
    let page = va & !(PAGE_SIZE - 1);
    let looked = match spaces.get(handle) {
        Ok(space) => look(&space, page, access),
        Err(_) => return false,
    };
    let (region, index) = match looked {
        Ok(Some(needs)) => needs,
        Ok(None) => return true,
        Err(_) => return false,
    };
    if !super::vmo::commit_fresh(region.vmo, index) {
        return false;
    }
    let Ok(mut space) = spaces.get(handle) else { return false };
    match look(&space, page, access) {
        Ok(None) => true,
        Ok(Some(now)) if now == (region, index) => {
            super::vmo::translate(region.vmo, index * PAGE_SIZE)
                .is_some_and(|pa| install(&mut space, page, &region, pa).is_ok())
        }
        _ => false,
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
