// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `HartTable<T>` — one entry of kernel state per logical CPU, sized by the machine
//! (RFC-0118). The boot hart's entry is the static's own, usable from the first instruction
//! (a trap on the boot hart may touch it before any allocator runs); the entries of the other
//! CPUs are allocated once, before a secondary hart starts, for exactly the harts the device
//! tree names. No table is sized by a constant and no entry is reached by a hart id.
//! An entry is shared (`&T`): what changes in it is an atomic or sits behind the entry's own
//! lock. State only its own hart may touch is [`super::percpu::PerCpu`], built on this.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (host): the boot entry before sizing, sizing once, an index
//!   beyond the machine, the unsized-table detector
//! INVARIANTS: sized at most once; an entry never moves or is freed; `get` never allocates
//! RFC: docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md

use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Why a table was not sized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeError {
    /// It already is: the machine does not change after boot.
    AlreadySized,
    /// The kernel heap cannot hold the entries.
    NoMemory,
}

/// The machine's CPU count once the kernel has sized its per-hart state (0: not yet). From
/// then on an unsized table asked for the entry of a CPU the machine has is a table the
/// sizing forgot.
static MACHINE_HARTS: AtomicUsize = AtomicUsize::new(0);
/// Asks for such an entry — a kernel bug the boot gate reports (`KSELFTEST: per-hart
/// state`), never a silent skip.
static UNSIZED_HITS: AtomicUsize = AtomicUsize::new(0);

/// Declares the kernel's per-hart state sized for a machine of `harts` CPUs (the boot hart,
/// after the last `size`).
pub fn machine_sized(harts: usize) {
    MACHINE_HARTS.store(harts, Ordering::Release);
}

/// How often a secondary's entry was asked of a table the sizing forgot.
#[must_use]
pub fn unsized_hits() -> usize {
    UNSIZED_HITS.load(Ordering::Acquire)
}

/// One `T` per logical CPU.
pub struct HartTable<T> {
    boot: T,
    rest: AtomicPtr<T>,
    /// Entries behind `rest`: CPUs `1..=rest_len`.
    rest_len: AtomicUsize,
}

// SAFETY: the table hands out `&T` to any hart, which is sound exactly when `T: Sync`; the
// pointer is written once (under the `rest_len` claim in `size`) and the slice it names is
// leaked, so a reference handed out stays valid for the kernel's life.
unsafe impl<T: Sync> Sync for HartTable<T> {}

impl<T> HartTable<T> {
    /// A table holding the boot hart's entry; the others come with [`Self::size`].
    pub const fn new(boot: T) -> Self {
        Self { boot, rest: AtomicPtr::new(core::ptr::null_mut()), rest_len: AtomicUsize::new(0) }
    }

    /// The number of CPUs the table has an entry for.
    #[must_use]
    pub fn len(&self) -> usize {
        1 + self.rest_len.load(Ordering::Acquire)
    }

    /// The boot hart's entry.
    #[must_use]
    pub fn boot(&self) -> &T {
        &self.boot
    }

    /// CPU `cpu`'s entry; `None` for a CPU the table has no entry for.
    #[must_use]
    pub fn get(&self, cpu: usize) -> Option<&T> {
        if cpu == 0 {
            return Some(&self.boot);
        }
        let rest = self.rest();
        if rest.is_empty() && cpu < MACHINE_HARTS.load(Ordering::Acquire) {
            UNSIZED_HITS.fetch_add(1, Ordering::AcqRel);
        }
        rest.get(cpu - 1)
    }

    /// Every entry, CPU 0 first.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        core::iter::once(&self.boot).chain(self.rest().iter())
    }

    fn rest(&self) -> &[T] {
        let len = self.rest_len.load(Ordering::Acquire);
        let ptr = self.rest.load(Ordering::Acquire);
        if len == 0 || ptr.is_null() {
            return &[];
        }
        // SAFETY: `size` published `ptr` before `rest_len`, both name one leaked boxed slice
        // of exactly `len` initialised entries that is never freed or moved.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }

    /// Sizes the table for a machine of `harts` CPUs: the entries of CPUs `1..harts`, each
    /// built by `make`. Once, by the boot hart, before a secondary starts; a machine of one
    /// hart needs nothing.
    pub fn size(&self, harts: usize, mut make: impl FnMut() -> T) -> Result<(), SizeError> {
        if self.rest_len.load(Ordering::Acquire) != 0 {
            return Err(SizeError::AlreadySized);
        }
        let count = harts.saturating_sub(1);
        if count == 0 {
            return Ok(());
        }
        let mut entries = Vec::new();
        entries.try_reserve_exact(count).map_err(|_| SizeError::NoMemory)?;
        entries.extend(core::iter::repeat_with(&mut make).take(count));
        let entries = alloc::boxed::Box::leak(entries.into_boxed_slice());
        // The pointer first: a reader that sees the length sees the entries.
        let claimed = self.rest.compare_exchange(
            core::ptr::null_mut(),
            entries.as_mut_ptr(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        if claimed.is_err() {
            return Err(SizeError::AlreadySized);
        }
        self.rest_len.store(count, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicU32;

    /// The detector's state is the kernel's one machine: the two tests that ask an unsized
    /// table for a secondary's entry take turns.
    static MACHINE: spin::Mutex<()> = spin::Mutex::new(());

    #[test]
    fn the_boot_entry_is_there_before_the_machine_is_sized() {
        let _machine = MACHINE.lock();
        static TABLE: HartTable<AtomicU32> = HartTable::new(AtomicU32::new(7));
        assert_eq!(TABLE.len(), 1);
        assert_eq!(TABLE.get(0).map(|e| e.load(Ordering::Relaxed)), Some(7));
        assert_eq!(TABLE.boot().load(Ordering::Relaxed), 7);
        assert!(TABLE.get(1).is_none());
    }

    #[test]
    fn a_table_is_sized_once_for_the_harts_of_the_machine() {
        let table = HartTable::new(AtomicU32::new(0));
        let mut next = 10;
        let make = || {
            next += 1;
            AtomicU32::new(next)
        };
        assert_eq!(table.size(8, make), Ok(()));
        assert_eq!(table.len(), 8);
        let values: Vec<u32> = table.iter().map(|e| e.load(Ordering::Relaxed)).collect();
        assert_eq!(values, [0, 11, 12, 13, 14, 15, 16, 17]);
        // An entry is shared state: a write through one reference is seen through another.
        table.get(5).unwrap().store(99, Ordering::Relaxed);
        assert_eq!(table.get(5).unwrap().load(Ordering::Relaxed), 99);
        assert_eq!(table.size(8, || AtomicU32::new(0)), Err(SizeError::AlreadySized));
        assert_eq!(table.len(), 8);
    }

    #[test]
    fn a_machine_of_one_hart_needs_no_allocation() {
        let table = HartTable::new(AtomicU32::new(3));
        assert_eq!(table.size(1, || unreachable!("no entry beyond the boot hart")), Ok(()));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_reject_a_cpu_beyond_the_machine() {
        let table = HartTable::new(AtomicU32::new(0));
        table.size(4, || AtomicU32::new(1)).unwrap();
        assert!(table.get(3).is_some());
        assert!(table.get(4).is_none());
        assert!(table.get(usize::MAX).is_none());
    }

    #[test]
    fn a_table_the_sizing_forgot_is_counted_not_skipped_silently() {
        let _machine = MACHINE.lock();
        let forgotten = HartTable::new(AtomicU32::new(0));
        let sized = HartTable::new(AtomicU32::new(0));
        sized.size(2, || AtomicU32::new(0)).unwrap();
        // Before the machine is declared sized nothing counts: a probe during early boot.
        let early = unsized_hits();
        assert!(forgotten.get(1).is_none());
        assert_eq!(unsized_hits(), early);
        machine_sized(2);
        // A CPU the machine does not have is an ordinary miss, on either table.
        assert!(forgotten.get(2).is_none() && sized.get(2).is_none());
        assert_eq!(unsized_hits(), early);
        assert!(sized.get(1).is_some());
        assert!(forgotten.get(1).is_none());
        assert_eq!(unsized_hits(), early + 1, "the forgotten table's miss is counted");
        machine_sized(0);
    }
}
