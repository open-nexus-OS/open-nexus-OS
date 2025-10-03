// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Virtual memory primitives for Sv39.

extern crate alloc;
use alloc::vec;      
use alloc::vec::Vec; 
use bitflags::bitflags;

#[cfg(feature = "failpoints")]
use core::sync::atomic::{AtomicBool, Ordering};

/// Size of a page in bytes.
pub const PAGE_SIZE: usize = 4096;
/// Number of entries per Sv39 page table.
const PT_ENTRIES: usize = 512;

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    /// Flags stored in an Sv39 PTE.
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

/// Error returned by mapping operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// Virtual address is not page aligned.
    Unaligned,
    /// Mapping would exceed the tracked region.
    OutOfRange,
    /// Mapping lacks permissions to proceed.
    PermissionDenied,
    /// Mapping overlaps an existing entry.
    Overlap,
    /// Provided flags are not suitable for installing a mapping.
    InvalidFlags,
}

/// Simple Sv39 page table used for bootstrap tasks.
#[derive(Debug)]
pub struct PageTable {
    entries: Vec<usize>,
}

#[cfg(feature = "failpoints")]
static DENY_NEXT_MAP: AtomicBool = AtomicBool::new(false);

impl PageTable {
    /// Creates an empty page table with all entries zeroed.
    pub fn new() -> Self {
        Self { entries: vec![0; PT_ENTRIES] }
    }

    /// Maps `pa` at virtual address `va` with the provided flags.
    pub fn map(&mut self, va: usize, pa: usize, flags: PageFlags) -> Result<(), MapError> {
        if va % PAGE_SIZE != 0 || pa % PAGE_SIZE != 0 {
            return Err(MapError::Unaligned);
        }
        if !flags.contains(PageFlags::VALID) {
            return Err(MapError::InvalidFlags);
        }
        let index = va / PAGE_SIZE;
        if index >= self.entries.len() {
            return Err(MapError::OutOfRange);
        }
        if self.entries[index] != 0 {
            return Err(MapError::Overlap);
        }
        #[cfg(feature = "failpoints")]
        if DENY_NEXT_MAP.swap(false, Ordering::SeqCst) {
            return Err(MapError::PermissionDenied);
        }
        self.entries[index] = pa | flags.bits();
        Ok(())
    }

    /// Returns the stored entry for `va` if present.
    pub fn lookup(&self, va: usize) -> Option<usize> {
        if va % PAGE_SIZE != 0 {
            return None;
        }
        let index = va / PAGE_SIZE;
        self.entries.get(index).copied().filter(|entry| *entry != 0)
    }

    /// Returns the physical address of the page table suitable for SATP.
    pub fn root_ppn(&self) -> usize {
        self.entries.as_ptr() as usize / PAGE_SIZE
    }
}

#[cfg(feature = "failpoints")]
pub mod failpoints {
    use super::DENY_NEXT_MAP;
    use core::sync::atomic::Ordering;

    /// Forces the next `map` invocation to return [`MapError::PermissionDenied`].
    pub fn deny_next_map() {
        DENY_NEXT_MAP.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
