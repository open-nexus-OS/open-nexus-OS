// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Sv39 page-table implementation with lazy allocation of intermediate levels.

extern crate alloc;

use alloc::{boxed::Box, vec, vec::Vec};
use core::ptr::NonNull;

use bitflags::bitflags;

/// Size of a single page in bytes.
pub const PAGE_SIZE: usize = 4096;
/// Number of entries per Sv39 page-table page.
const PT_ENTRIES: usize = 512;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Flags stored in Sv39 page-table entries.
    pub struct PageFlags: usize {
        const VALID = 1 << 0;
        const READ = 1 << 1;
        const WRITE = 1 << 2;
        const EXECUTE = 1 << 3;
        const USER = 1 << 4;
        const GLOBAL = 1 << 5;
        const ACCESSED = 1 << 6;
        const DIRTY = 1 << 7;
    }
}

/// Error returned when manipulating page tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    /// Virtual or physical address was not page aligned.
    Unaligned,
    /// Mapping extends beyond the canonical Sv39 range.
    OutOfRange,
    /// Mapping violates the W^X policy.
    PermissionDenied,
    /// Mapping collides with an existing entry.
    Overlap,
    /// Flags do not describe a valid leaf entry.
    InvalidFlags,
}

#[repr(align(4096))]
struct PageTablePage {
    entries: [usize; PT_ENTRIES],
}

impl PageTablePage {
    const fn new() -> Self {
        Self { entries: [0; PT_ENTRIES] }
    }
}

/// Three-level Sv39 page table allocating intermediate levels on demand.
pub struct PageTable {
    root: NonNull<PageTablePage>,
    owned: Vec<NonNull<PageTablePage>>,
}

impl PageTable {
    /// Creates an empty Sv39 page table with a fresh root page.
    pub fn new() -> Self {
        let root = Self::alloc_page();
        Self { root, owned: vec![root] }
    }

    /// Returns the physical page number of the root page suitable for SATP.
    pub fn root_ppn(&self) -> usize {
        self.root.as_ptr() as usize / PAGE_SIZE
    }

    /// Looks up the entry mapped at `va` if it exists.
    pub fn lookup(&self, va: usize) -> Option<usize> {
        if va % PAGE_SIZE != 0 || !is_canonical_sv39(va) {
            return None;
        }
        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { (*table.as_ptr()).entries[*index] };
            if entry & PageFlags::VALID.bits() == 0 {
                return None;
            }
            let is_leaf = entry & LEAF_PERMS.bits() != 0;
            if level == indices.len() - 1 {
                return if is_leaf { Some(entry) } else { None };
            }
            if is_leaf {
                return None;
            }
            let next = ((entry >> 10) << 12) as *mut PageTablePage;
            table = NonNull::new(next)?;
        }
        None
    }

    /// Installs a 4 KiB mapping from `va` to `pa` using `flags`.
    pub fn map(&mut self, va: usize, pa: usize, flags: PageFlags) -> Result<(), MapError> {
        if va % PAGE_SIZE != 0 || pa % PAGE_SIZE != 0 {
            return Err(MapError::Unaligned);
        }
        if !is_canonical_sv39(va) {
            return Err(MapError::OutOfRange);
        }
        if flags.intersection(LEAF_PERMS).is_empty() || !flags.contains(PageFlags::VALID) {
            return Err(MapError::InvalidFlags);
        }
        if flags.contains(PageFlags::WRITE) && flags.contains(PageFlags::EXECUTE) {
            return Err(MapError::PermissionDenied);
        }

        let indices = vpn_indices(va);
        let mut table = self.root;
        for (level, index) in indices.iter().enumerate() {
            let entry = unsafe { &mut (*table.as_ptr()).entries[*index] };
            if level == indices.len() - 1 {
                if *entry & PageFlags::VALID.bits() != 0 {
                    return Err(MapError::Overlap);
                }
                let ppn = pa / PAGE_SIZE;
                *entry = (ppn << 10) | flags.bits();
                return Ok(());
            }

            if *entry & PageFlags::VALID.bits() != 0 {
                if *entry & LEAF_PERMS.bits() != 0 {
                    return Err(MapError::Overlap);
                }
                let next = ((*(entry) >> 10) << 12) as *mut PageTablePage;
                table = NonNull::new(next).ok_or(MapError::OutOfRange)?;
                continue;
            }

            let next = Self::alloc_page();
            self.owned.push(next);
            let ppn = next.as_ptr() as usize / PAGE_SIZE;
            *entry = (ppn << 10) | PageFlags::VALID.bits();
            table = next;
        }
        Ok(())
    }

    fn alloc_page() -> NonNull<PageTablePage> {
        let boxed = Box::new(PageTablePage::new());
        // SAFETY: Box never yields a null pointer.
        unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        for page in self.owned.drain(..) {
            // SAFETY: every pointer originates from `alloc_page` and is unique.
            unsafe { drop(Box::from_raw(page.as_ptr())) };
        }
    }
}

const LEAF_PERMS: PageFlags = PageFlags::READ.union(PageFlags::WRITE).union(PageFlags::EXECUTE);

fn vpn_indices(va: usize) -> [usize; 3] {
    let vpn0 = (va >> 12) & 0x1ff;
    let vpn1 = (va >> 21) & 0x1ff;
    let vpn2 = (va >> 30) & 0x1ff;
    [vpn0, vpn1, vpn2]
}

fn is_canonical_sv39(va: usize) -> bool {
    let sign = (va >> 38) & 1;
    let upper = va >> 39;
    if sign == 0 {
        upper == 0
    } else {
        upper == usize::MAX >> 39
    }
}

