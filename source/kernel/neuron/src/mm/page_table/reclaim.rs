// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Intermediate page tables given back (TASK-0215 5c-3 P2). Found on the board: an
//! unmap of a region mapped with 4 KiB leaves left its emptied level-0 table linked under the
//! level-1 entry, and a later superpage over that 2 MiB span was refused as `Overlap` — every
//! large map landing there failed for the address space's life (gpud's frame readback among
//! them, once gpud gave its buffers back). An unmap now unlinks each table it emptied and keeps
//! it, tagged with its region, until the TLB shootdown that follows the clear landed on every
//! hart — a hart's page walker may hold the old pointer until then — and only then frees it:
//! CLEAR-SHOOTDOWN-FORGET applied to the tables, the order Zircon's and Linux's page-table
//! freeing keep. A superpage that meets a table anyway (a failed map's rollback leaves its
//! tables) is mapped as 4 KiB leaves into that table instead of being refused.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU `KSELFTEST: vm unmap ok` (an emptied table freed by its unmap; a superpage
//!   span over a leftover table mapped as pages and given back); the board's contexts rung and
//!   gpud's frame readback
//! INVARIANTS: a table is retired only when all its entries are invalid, unlinked before it is
//!   retired, and freed only by the unmap that retired it, after that unmap's shootdown; the root
//!   is never retired.

use core::ptr::NonNull;

use super::{
    free_page, table_at, vpn_indices, MapError, PageFlags, PageTable, PageTablePage,
    HUGE_PAGE_SIZE_2M, LEAF_PERMS, PAGE_SIZE, PT_ENTRIES,
};

/// Whether every entry of `table` is invalid.
fn empty(table: NonNull<PageTablePage>) -> bool {
    // SAFETY: an owned, live table page reached through the direct map.
    unsafe { (*table.as_ptr()).entries.iter().all(|e| e & PageFlags::VALID.bits() == 0) }
}

/// The table a valid non-leaf entry names, or `None` for an invalid entry or a leaf.
fn table_of(entry: usize) -> Option<NonNull<PageTablePage>> {
    if entry & PageFlags::VALID.bits() == 0 || entry & LEAF_PERMS.bits() != 0 {
        return None;
    }
    NonNull::new(table_at(entry))
}

impl PageTable {
    /// After an unmap of the region `(va, len)` cleared the leaves of the 2 MiB span at `span`:
    /// the span's level-0 table, when it is now empty, is unlinked from its level-1 entry and
    /// retired; then the level-1 table above, when nothing is left under it. Returns how many
    /// tables were retired. The caller shoots the TLBs down before [`Self::free_retired`].
    pub(crate) fn retire_empty(&mut self, span: usize, region: (usize, usize)) -> usize {
        let idx = vpn_indices(span);
        // SAFETY: the root is owned and live; the entry is this table's own.
        let top = unsafe { &mut (*self.root.as_ptr()).entries[idx[0]] };
        let Some(mid) = table_of(*top) else { return 0 };
        // SAFETY: a valid non-leaf entry of an owned table names an owned, live table.
        let entry = unsafe { &mut (*mid.as_ptr()).entries[idx[1]] };
        let mut retired = 0;
        if let Some(low) = table_of(*entry) {
            if empty(low) {
                *entry = 0;
                self.retired.push((region.0, region.1, low));
                retired += 1;
            }
        }
        // The span's level-1 entry is clear now — a superpage unmapped or its table retired —
        // so the level-1 table may have emptied with it.
        if *entry == 0 && empty(mid) {
            *top = 0;
            self.retired.push((region.0, region.1, mid));
            retired += 1;
        }
        retired
    }

    /// Frees the tables the unmap of `region` retired: its shootdown landed, no hart walks them.
    pub(crate) fn free_retired(&mut self, region: (usize, usize)) {
        let mut i = 0;
        while i < self.retired.len() {
            let (va, len, page) = self.retired[i];
            if (va, len) != region {
                i += 1;
                continue;
            }
            self.retired.swap_remove(i);
            if let Some(at) = self.owned.iter().position(|p| *p == page) {
                self.owned.swap_remove(at);
                free_page(page);
            }
        }
    }

    /// Tables retired and not yet freed (the selftest's witness).
    pub(crate) fn retired_tables(&self) -> usize {
        self.retired.len()
    }

    /// Maps the 2 MiB span at `va` from `pa`: one superpage leaf where its level-1 entry is
    /// free, or 512 leaves of 4 KiB into a table a failed map left there (refused — the span
    /// left as it was — when one of them is live). Returns the leaf size used.
    pub(crate) fn map_span(
        &mut self,
        va: usize,
        pa: usize,
        flags: PageFlags,
    ) -> Result<usize, MapError> {
        match self.map_2m(va, pa, flags) {
            Ok(()) => Ok(HUGE_PAGE_SIZE_2M),
            Err(MapError::Overlap) if self.table_under(va) => {
                for i in 0..PT_ENTRIES {
                    if let Err(e) = self.map(va + i * PAGE_SIZE, pa + i * PAGE_SIZE, flags) {
                        for j in 0..i {
                            let _ = self.unmap_leaf(va + j * PAGE_SIZE);
                        }
                        return Err(e);
                    }
                }
                Ok(PAGE_SIZE)
            }
            Err(e) => Err(e),
        }
    }

    /// Whether a table, not a leaf, sits under the level-1 entry of `va`.
    fn table_under(&self, va: usize) -> bool {
        let idx = vpn_indices(va);
        // SAFETY: the root is owned and live.
        let top = unsafe { (*self.root.as_ptr()).entries[idx[0]] };
        let Some(mid) = table_of(top) else { return false };
        // SAFETY: a valid non-leaf entry of an owned table names an owned, live table.
        let entry = unsafe { (*mid.as_ptr()).entries[idx[1]] };
        table_of(entry).is_some()
    }
}
