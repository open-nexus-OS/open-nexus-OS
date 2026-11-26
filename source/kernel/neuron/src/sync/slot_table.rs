// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a bounded table of kernel objects named by an id (TASK-0330 P2, ADR-0102) — the
//! waitsets and the fences. Each object sits behind its own lock, at a slot that never moves,
//! so two harts working on two objects share nothing but the count; finding an object by its
//! id takes no table lock. An id comes from one counter and lives in the slot `id % slots`:
//! the slot remembers the id it holds, so an id whose object was freed names nothing, also
//! once the slot has a new tenant. The counter is taken under the table's registry lock.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::*`: ids differ, a stale id is refused after its slot is reused,
//!   the bound, the counter's wrap never issues 0, removal by a predicate)
//! INVARIANTS: an id is issued again only after the counter went around (2^32 ids; with every
//!   other slot held, a slot's ids are `slots` apart); id 0 is never issued; a caller's closure
//!   runs under the object's lock and takes no lock of a lower class.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::lock_rank::Class;
use super::ranked::RankedLock;

/// A slot: the id it holds and the object, or nothing.
type Slot<T> = RankedLock<Option<(u32, T)>>;

/// The id after `id` (0 names nothing and is never issued).
const fn after(id: u32) -> u32 {
    match id.wrapping_add(1) {
        0 => 1,
        next => next,
    }
}

/// A bounded table of objects, each under its own lock.
pub struct SlotTable<T> {
    slots: Box<[Slot<T>]>,
    /// The next id to try, under the registry lock: one insert chooses a slot at a time.
    next: RankedLock<u32>,
    live: AtomicUsize,
}

impl<T> SlotTable<T> {
    /// A table of `slots` objects whose locks are of `class`.
    pub fn new(class: Class, slots: usize) -> Self {
        Self::starting_at(class, slots, 1)
    }

    fn starting_at(class: Class, slots: usize, first: u32) -> Self {
        let slots: Vec<Slot<T>> = (0..slots)
            .map(|index| {
                let slot = RankedLock::new(class, None);
                slot.set_key(index);
                slot
            })
            .collect();
        Self {
            slots: slots.into_boxed_slice(),
            next: RankedLock::new(Class::Registry, first),
            live: AtomicUsize::new(0),
        }
    }

    fn slot(&self, id: u32) -> Option<&Slot<T>> {
        let slots = self.slots.len();
        if id == 0 || slots == 0 {
            return None;
        }
        self.slots.get(id as usize % slots)
    }

    /// Puts `value` into a free slot and returns its id; `None` when the table is full.
    pub fn insert(&self, value: T) -> Option<u32> {
        let mut next = self.next.lock();
        if self.live.load(Ordering::Acquire) >= self.slots.len() {
            return None;
        }
        // A slot is free and inserts are serialized: `slots` ids in a row reach every slot
        // (one more covers the id the wrap skips).
        for _ in 0..=self.slots.len() {
            let id = *next;
            *next = after(id);
            let Some(slot) = self.slot(id) else { continue };
            let mut slot = slot.lock();
            if slot.is_none() {
                *slot = Some((id, value));
                self.live.fetch_add(1, Ordering::AcqRel);
                return Some(id);
            }
        }
        None
    }

    /// Runs `f` on the object `id` names, under its lock; `None` when `id` names nothing.
    pub fn with<R>(&self, id: u32, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut slot = self.slot(id)?.lock();
        match slot.as_mut() {
            Some((held, value)) if *held == id => Some(f(value)),
            _ => None,
        }
    }

    /// Takes the object `id` names out of the table.
    pub fn remove(&self, id: u32) -> Option<T> {
        let mut slot = self.slot(id)?.lock();
        if !matches!(slot.as_ref(), Some((held, _)) if *held == id) {
            return None;
        }
        self.live.fetch_sub(1, Ordering::AcqRel);
        slot.take().map(|(_, value)| value)
    }

    /// Takes every object `gone` names out of the table and hands it to `taken`, after its
    /// slot's lock is released. Returns how many.
    pub fn remove_if(&self, mut gone: impl FnMut(&T) -> bool, mut taken: impl FnMut(T)) -> usize {
        let mut count = 0;
        for slot in self.slots.iter() {
            let removed = {
                let mut slot = slot.lock();
                if slot.as_ref().is_some_and(|(_, value)| gone(value)) {
                    self.live.fetch_sub(1, Ordering::AcqRel);
                    slot.take()
                } else {
                    None
                }
            };
            if let Some((_, value)) = removed {
                count += 1;
                taken(value);
            }
        }
        count
    }

    /// Runs `f` on every object, one lock at a time.
    pub fn each(&self, mut f: impl FnMut(&mut T)) {
        for slot in self.slots.iter() {
            if let Some((_, value)) = slot.lock().as_mut() {
                f(value);
            }
        }
    }

    /// Objects in the table.
    pub fn len(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(slots: usize) -> SlotTable<u32> {
        SlotTable::new(Class::Ipc, slots)
    }

    #[test]
    fn ids_differ_and_name_their_objects() {
        let t = table(4);
        let a = t.insert(10).expect("a");
        let b = t.insert(20).expect("b");
        assert_ne!(a, b);
        assert_eq!(t.with(a, |v| *v), Some(10));
        assert_eq!(t.with(b, |v| *v), Some(20));
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn the_table_is_bounded() {
        let t = table(3);
        for v in 0..3 {
            assert!(t.insert(v).is_some());
        }
        assert_eq!(t.insert(9), None);
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn test_reject_a_stale_id_after_its_slot_is_reused() {
        let t = table(2);
        let old = t.insert(1).expect("old");
        assert_eq!(t.remove(old), Some(1));
        assert_eq!(t.remove(old), None, "a second removal finds nothing");
        // Fill the table: the old id's slot has a new tenant under another id.
        let a = t.insert(2).expect("a");
        let b = t.insert(3).expect("b");
        assert!(a != old && b != old);
        assert_eq!(t.with(old, |v| *v), None, "the freed id names nothing");
        assert_eq!(t.remove(old), None);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn test_reject_id_zero_and_the_wrap_never_issues_it() {
        let t: SlotTable<u32> = SlotTable::starting_at(Class::Ipc, 4, u32::MAX - 1);
        assert_eq!(t.with(0, |v| *v), None);
        let ids: Vec<u32> = (0..4).map(|v| t.insert(v).expect("insert")).collect();
        assert!(ids.iter().all(|&id| id != 0));
        for (v, &id) in ids.iter().enumerate() {
            assert_eq!(t.with(id, |x| *x), Some(v as u32));
        }
        assert_eq!(t.insert(9), None);
    }

    #[test]
    fn a_full_table_minus_one_still_finds_the_free_slot() {
        let t = table(8);
        let ids: Vec<u32> = (0..8).map(|v| t.insert(v).expect("insert")).collect();
        // One slot is freed and refilled over and over: each tenant has an id of its own.
        let mut victim = ids[3];
        let mut seen = alloc::vec![victim];
        for round in 0..100u32 {
            assert!(t.remove(victim).is_some());
            let again = t.insert(round).expect("the one free slot is found");
            assert!(!seen.contains(&again));
            assert_eq!(t.with(victim, |v| *v), None);
            assert_eq!(t.with(again, |v| *v), Some(round));
            seen.push(again);
            victim = again;
        }
        assert_eq!(t.len(), 8);
    }

    #[test]
    fn removal_by_a_predicate_hands_out_what_it_took() {
        let t = table(8);
        for v in 0..6 {
            let _ = t.insert(v);
        }
        let mut taken = Vec::new();
        assert_eq!(t.remove_if(|v| v % 2 == 0, |v| taken.push(v)), 3);
        taken.sort_unstable();
        assert_eq!(taken, [0, 2, 4]);
        assert_eq!(t.len(), 3);
        let mut left = Vec::new();
        t.each(|v| left.push(*v));
        left.sort_unstable();
        assert_eq!(left, [1, 3, 5]);
    }
}
