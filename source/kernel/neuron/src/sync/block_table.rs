// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a table whose slots never move (TASK-0330 P2, ADR-0102) — blocks of 64 slots, each
//! block allocated once and kept for the table's life, so a slot found by its index stays where
//! it is while the table grows. A reader takes no lock: it reads the count and a block pointer.
//! Growing is the caller's to serialize (its registry lock); what a slot holds is the slot's own
//! business (a lock inside it). The scheduling records are built the same way.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::*`: addresses stay across growth, the bound, a refused admission)
//! INVARIANTS: a block pointer is written once, before the count that makes its slots visible;
//!   a block is freed only with the table.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Slots in a block.
pub const BLOCK: usize = 64;

/// Why a table did not grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowError {
    /// Every block is in use.
    Full,
    /// The heap did not admit or did not have a block.
    NoMemory,
}

/// Slots of `T` at fixed addresses: 64 per block, as many blocks as the table was made for.
pub struct BlockTable<T> {
    blocks: Box<[AtomicPtr<T>]>,
    len: AtomicUsize,
}

// SAFETY: the table hands out `&T` only; its own words are atomics and its blocks are written
// once. Sharing it is as safe as sharing the `T`s.
unsafe impl<T: Sync> Sync for BlockTable<T> {}
// SAFETY: the table owns its blocks (freed in `Drop`), like a `Vec<Box<[T]>>`.
unsafe impl<T: Send> Send for BlockTable<T> {}

impl<T> BlockTable<T> {
    /// An empty table that can grow to `blocks` blocks. The block pointers are allocated here,
    /// on the heap and never on the caller's stack; a heap that refuses them leaves a table
    /// that cannot grow (`GrowError::Full`).
    pub fn with_blocks(blocks: usize) -> Self {
        let mut pointers = Vec::new();
        if pointers.try_reserve_exact(blocks).is_ok() {
            pointers.extend((0..blocks).map(|_| AtomicPtr::new(null_mut())));
        }
        Self { blocks: pointers.into_boxed_slice(), len: AtomicUsize::new(0) }
    }

    /// The slots the table has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Whether the table has no slot.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Slot `index`, where it has been since it was added.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len() {
            return None;
        }
        let base = self.blocks.get(index / BLOCK)?.load(Ordering::Acquire);
        if base.is_null() {
            return None;
        }
        // SAFETY: `index < len`, and the count is raised only after the block that holds the
        // slot was stored; a block has `BLOCK` initialized slots and lives as long as the table.
        Some(unsafe { &*base.add(index % BLOCK) })
    }

    /// Every slot, in index order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.len()).filter_map(|index| self.get(index))
    }

    /// Adds one slot and answers its index. A new block's slots are built by `make`; `admit`
    /// is asked with the block's bytes before one is allocated. The caller serializes `grow`.
    pub fn grow(
        &self,
        admit: impl FnOnce(usize) -> bool,
        mut make: impl FnMut() -> T,
    ) -> Result<usize, GrowError> {
        let index = self.len.load(Ordering::Acquire);
        let slot = self.blocks.get(index / BLOCK).ok_or(GrowError::Full)?;
        if slot.load(Ordering::Acquire).is_null() {
            if !admit(BLOCK * core::mem::size_of::<T>()) {
                return Err(GrowError::NoMemory);
            }
            let mut fresh = Vec::new();
            fresh.try_reserve_exact(BLOCK).map_err(|_| GrowError::NoMemory)?;
            fresh.extend((0..BLOCK).map(|_| make()));
            let base = Box::into_raw(fresh.into_boxed_slice()) as *mut T;
            slot.store(base, Ordering::Release);
        }
        self.len.store(index + 1, Ordering::Release);
        Ok(index)
    }
}

impl<T> Drop for BlockTable<T> {
    fn drop(&mut self) {
        for slot in self.blocks.iter() {
            let base = slot.swap(null_mut(), Ordering::AcqRel);
            if !base.is_null() {
                // SAFETY: `base` came from a boxed slice of `BLOCK` slots in `grow` and nothing
                // borrows the table any more.
                drop(unsafe { Box::from_raw(core::ptr::slice_from_raw_parts_mut(base, BLOCK)) });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicU32;

    #[test]
    fn a_slot_stays_where_it_is_while_the_table_grows() {
        let table: BlockTable<AtomicU32> = BlockTable::with_blocks(4);
        assert!(table.is_empty() && table.get(0).is_none());
        let first = table.grow(|_| true, || AtomicU32::new(0)).unwrap();
        let at = table.get(first).unwrap() as *const AtomicU32;
        table.get(first).unwrap().store(7, Ordering::Relaxed);
        for _ in 1..3 * BLOCK {
            table.grow(|_| true, || AtomicU32::new(0)).unwrap();
        }
        assert_eq!(table.len(), 3 * BLOCK);
        assert_eq!(table.get(first).unwrap() as *const AtomicU32, at);
        assert_eq!(table.get(first).unwrap().load(Ordering::Relaxed), 7);
        assert_eq!(table.iter().count(), 3 * BLOCK);
        assert!(table.get(3 * BLOCK).is_none(), "a slot of the block that was never added");
    }

    #[test]
    fn test_reject_a_slot_beyond_the_last_block() {
        let table: BlockTable<AtomicU32> = BlockTable::with_blocks(1);
        for _ in 0..BLOCK {
            table.grow(|_| true, || AtomicU32::new(0)).unwrap();
        }
        assert_eq!(table.grow(|_| true, || AtomicU32::new(0)), Err(GrowError::Full));
        assert_eq!(table.len(), BLOCK);
    }

    #[test]
    fn test_reject_a_block_the_heap_does_not_admit() {
        let table: BlockTable<AtomicU32> = BlockTable::with_blocks(2);
        assert_eq!(table.grow(|_| false, || AtomicU32::new(0)), Err(GrowError::NoMemory));
        assert!(table.is_empty());
        // Within an admitted block no admission is asked again.
        table.grow(|_| true, || AtomicU32::new(0)).unwrap();
        assert_eq!(table.grow(|_| false, || AtomicU32::new(0)), Ok(1));
    }
}
