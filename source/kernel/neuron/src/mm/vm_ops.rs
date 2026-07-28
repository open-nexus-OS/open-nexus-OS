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
) -> Result<usize, VmOpError> {
    if len == 0 || len % PAGE_SIZE != 0 || pa % PAGE_SIZE != 0 {
        return Err(VmOpError::Va(VaError::BadInput));
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
    space.va_space_mut().insert(VaRegion { va, len, pa, flags: flags.bits(), kind }).map_err(
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
    if let Err(err) = execute_plan(space, va, pa, len, flags, &mut mapped_end) {
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
    Ok(va)
}

/// Unmaps the exact region `(va, len)` previously returned by [`map_range`].
/// One TLB shootdown per call (not per page); the region record survives
/// until the shootdown completes.
pub fn unmap_range(space: &mut AddressSpace, va: usize, len: usize) -> Result<(), VmOpError> {
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
    crate::smp::tlb::shootdown_all();
    // Peeked above with the same arguments — cannot fail here.
    let _ = space.va_space_mut().remove_exact(va, len);
    Ok(())
}

/// Does ANY live address space still map part of `[pa, pa+len)` through a
/// `vm_map`/`mmio_map_auto` region? The `vmo_destroy` EBUSY guard — a
/// destroy that left live PTEs onto recycled arena pages would hand the next
/// owner's memory to the old mapper.
#[must_use]
pub fn any_space_maps(manager: &AddressSpaceManager, pa: usize, len: usize) -> bool {
    manager.spaces.iter().flatten().any(|space| space.va_space().any_backed_by(pa, len))
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
        page = (((entry >> 10) & ((1usize << 44) - 1)) << 12) as *const _;
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
