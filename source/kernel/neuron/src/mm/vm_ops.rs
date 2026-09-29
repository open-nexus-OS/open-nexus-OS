// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0085 vm_map/vm_unmap EXECUTOR — turns the pure `va_space`
//! decisions (hole choice, superpage plan, region record) into page-table
//! edits. Owns two ordering invariants: RECORD-THEN-MAP (a mapping that
//! cannot be recorded is refused, and a mid-plan failure rolls back to
//! nothing — never a half-mapped zombie) and CLEAR-SHOOTDOWN-FORGET (a
//! region leaves the record only after its PTEs are gone from every hart's
//! TLB). The syscall shell (`syscall/api/vm_map.rs`) feeds it
//! capability-derived facts; policy stays host-proven in `va_space`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (RFC-0085 Phase 3)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: policy host-proven in `va_space_tests`; executor proven in
//! QEMU by `KSELFTEST: vm map ok / vm unmap ok / vm map reject ok` and the
//! selftest-client userspace roundtrip probe
//! ADR: docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md

use super::address_space::{AddressSpace, AddressSpaceManager};
use super::page_table::{MapError, PageFlags, PAGE_SIZE};
use crate::va_space::{superpage_plan, RegionKind, VaError, VaRegion, SUPERPAGE};

/// Executor failure: either the policy refused (errno keeps VaError identity
/// per ADR-0054) or the page table did (impossible for a kernel-chosen hole
/// unless the table diverged — surfaced, never swallowed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmOpError {
    Va(VaError),
    Map(MapError),
}

/// Maps `[pa, pa+len)` at a kernel-chosen VA inside this space's managed
/// window and returns that VA. `flags` must already carry the caller's
/// security floor; ≥2 MiB requests get a pa-phase-congruent VA so interior
/// spans promote to 2 MiB leaves.
pub fn map_range(
    space: &mut AddressSpace,
    pa: usize,
    len: usize,
    flags: PageFlags,
    kind: RegionKind,
    vmo: u32,
) -> Result<usize, VmOpError> {
    map_runs(space, &[(pa, len)], flags, kind, vmo)
}

/// Map physically contiguous runs `(pa, len)` back to back at ONE
/// kernel-chosen va (TASK-0286 P3a: a page-backed VMO is its runs). The va
/// is chosen so the first run keeps its 2 MiB phase; later runs of a
/// largest-first object are superpage-aligned by construction, so every
/// eligible run promotes. One region records the whole span.
pub fn map_runs(
    space: &mut AddressSpace,
    runs: &[(usize, usize)],
    flags: PageFlags,
    kind: RegionKind,
    vmo: u32,
) -> Result<usize, VmOpError> {
    let Some(&(pa, _)) = runs.first() else { return Err(VmOpError::Va(VaError::BadInput)) };
    let mut len = 0usize;
    for &(run_pa, run_len) in runs {
        if run_len == 0 || run_len % PAGE_SIZE != 0 || run_pa % PAGE_SIZE != 0 {
            return Err(VmOpError::Va(VaError::BadInput));
        }
        len = len.checked_add(run_len).ok_or(VmOpError::Va(VaError::BadInput))?;
    }
    let (align, phase) =
        if len >= SUPERPAGE { (SUPERPAGE, pa & (SUPERPAGE - 1)) } else { (PAGE_SIZE, 0) };
    let Some(va) = space.va_space().find_hole(len, align, phase) else {
        log_error!(
            target: "vm",
            "VM-MAP-FAIL reason=window-exhausted want=0x{:x} free_max=0x{:x} regions={}",
            len,
            space.va_space().largest_free_hole(),
            space.va_space().region_count()
        );
        return Err(VmOpError::Va(VaError::WindowExhausted));
    };
    // RECORD-THEN-MAP: reserve the bookkeeping slot before touching the page
    // table, so "mapped but unrecorded" cannot exist even across a failure.
    space.va_space_mut().insert(VaRegion { va, len, pa, flags: flags.bits(), kind, vmo }).map_err(
        |err| {
            if err == VaError::TableFull {
                log_error!(
                    target: "vm",
                    "VM-MAP-FAIL reason=table-full regions={} peak={}",
                    space.va_space().region_count(),
                    space.va_space().peak_regions()
                );
            }
            VmOpError::Va(err)
        },
    )?;
    let mut mapped_end = va;
    let mut at = va;
    let planned = runs.iter().try_for_each(|&(run_pa, run_len)| {
        let r = execute_plan(space, at, run_pa, run_len, flags, &mut mapped_end);
        at += run_len;
        r
    });
    if let Err(err) = planned {
        rollback(space, va, mapped_end);
        let _ = space.va_space_mut().remove_exact(va, len);
        log_error!(
            target: "vm",
            "VM-MAP-FAIL reason=pte va=0x{:x} mapped_to=0x{:x} err={:?}",
            va,
            mapped_end,
            err
        );
        return Err(VmOpError::Map(err));
    }
    fence_asid(space.asid());
    Ok(va)
}

/// `sfence.vma` for one address space after its PTEs changed — invalid→valid included:
/// RISC-V lets the walker cache a miss, so the next access could fault on a page that is
/// mapped (Linux's `update_mmu_cache` fences for the same reason). QEMU never caches a
/// miss and hid every missing fence; the board's first store into a freshly mapped DMA
/// table page-faulted (TASK-0260B P3). Harmless for a space this hart is not running.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn fence_asid(asid: crate::types::Asid) {
    let raw = asid.as_raw() as usize;
    // SAFETY: a TLB fence scoped to one ASID; no memory or register side effects.
    unsafe {
        core::arch::asm!("sfence.vma x0, {0}", in(reg) raw, options(nostack, preserves_flags));
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub(crate) fn fence_asid(_asid: crate::types::Asid) {}

/// Phase A of an unmap: validate `(va, len)` and clear its PTEs. The region
/// STAYS recorded — its va must not become reusable before the TLB
/// shootdown lands (CLEAR-SHOOTDOWN-FORGET). The syscall shell runs the
/// shootdown with the BKL DROPPED (phased like VMO_CREATE; a BKL-held
/// shootdown wait tripped the 10ms `bkl budget ok` gate at SMP≥2), then
/// calls [`forget_range`].
pub fn clear_range(space: &mut AddressSpace, va: usize, len: usize) -> Result<(), VmOpError> {
    let region = space.va_space().peek_exact(va, len).map_err(VmOpError::Va)?;
    let end = region.va + region.len;
    let mut cursor = region.va;
    while cursor < end {
        match space.page_table_mut().unmap_leaf(cursor) {
            Ok(size) => cursor += size,
            // A hole inside a tracked region means record/table divergence —
            // constructively impossible for executor-created regions. Step a
            // page and keep clearing rather than leaving live PTEs behind.
            Err(_) => cursor += PAGE_SIZE,
        }
    }
    Ok(())
}

/// Phase C: forget the region after the shootdown completed. Idempotent-ish
/// by construction: phase A peeked the same arguments.
pub fn forget_range(space: &mut AddressSpace, va: usize, len: usize) {
    let _ = space.va_space_mut().remove_exact(va, len);
}

/// Unmaps the exact region `(va, len)` previously returned by [`map_range`]:
/// clear, ONE shootdown, forget. In-kernel callers only (selftest) — the
/// vm_unmap SYSCALL phases the shootdown outside the BKL instead.
pub fn unmap_range(space: &mut AddressSpace, va: usize, len: usize) -> Result<(), VmOpError> {
    clear_range(space, va, len)?;
    crate::smp::tlb::shootdown_all();
    forget_range(space, va, len);
    Ok(())
}

/// Does ANY live address space still map part of `[pa, pa+len)` through a
/// `vm_map`/`mmio_map_auto` region? The `vmo_destroy` EBUSY guard — a
/// destroy that left live PTEs onto recycled arena pages would hand the next
/// owner's memory to the old mapper.
#[must_use]
pub fn any_space_maps_vmo(manager: &AddressSpaceManager, id: u32) -> bool {
    manager.spaces.iter().flatten().any(|space| space.va_space().any_backed_by_vmo(id))
}

/// Leaf span (4 KiB or 2 MiB) mapped at `va`, or `None`. The selftest oracle
/// for superpage promotion: proves the 2 MiB leaf EXISTS instead of assuming
/// the plan produced one. Read-only sibling of the `page_table` walkers.
#[must_use]
pub fn leaf_span_at(space: &AddressSpace, va: usize) -> Option<usize> {
    use super::page_table::{vpn_indices, LEAF_PERMS};
    const SPAN_BY_LEVEL: [usize; 3] = [1 << 30, SUPERPAGE, PAGE_SIZE];
    let idx = vpn_indices(va);
    let mut page = space.page_table().root.as_ptr().cast_const();
    for (level, span) in SPAN_BY_LEVEL.into_iter().enumerate() {
        // vpn_indices returns [vpn2, vpn1, vpn0] — already top-down walk order.
        let entry = unsafe { (*page).entries[idx[level]] };
        if entry & PageFlags::VALID.bits() == 0 {
            return None;
        }
        if entry & LEAF_PERMS.bits() != 0 {
            return Some(span);
        }
        page = crate::phys::phys_to_virt(((entry >> 10) & ((1usize << 44) - 1)) << 12) as *const _;
    }
    None
}

fn execute_plan(
    space: &mut AddressSpace,
    va: usize,
    pa: usize,
    len: usize,
    flags: PageFlags,
    mapped_end: &mut usize,
) -> Result<(), MapError> {
    for chunk in superpage_plan(va, pa, len).into_iter().flatten() {
        let step = if chunk.superpage { SUPERPAGE } else { PAGE_SIZE };
        let mut off = 0;
        while off < chunk.len {
            let (cva, cpa) = (chunk.va + off, chunk.pa + off);
            if chunk.superpage {
                space.page_table_mut().map_2m(cva, cpa, flags)?;
            } else {
                space.page_table_mut().map(cva, cpa, flags)?;
            }
            off += step;
            *mapped_end = cva + step;
        }
    }
    Ok(())
}

fn rollback(space: &mut AddressSpace, va: usize, mapped_end: usize) {
    let mut cursor = va;
    while cursor < mapped_end {
        match space.page_table_mut().unmap_leaf(cursor) {
            Ok(size) => cursor += size,
            Err(_) => cursor += PAGE_SIZE,
        }
    }
}
