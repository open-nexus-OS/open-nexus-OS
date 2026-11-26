// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The memory objects one address space maps (RFC-0102, TASK-0330 P3): per object the
//! regions that name it, how many of them are writable, and the one `Mapped` the space holds at
//! the object's header. The header's counts — spaces that map the object, spaces that map it
//! writably — are what the reclaim, the seal, the purge unlock and the squeeze read; they used
//! to ask every address space. The space's first region of an object takes the token, its
//! first writable region turns the token writable — both through the caller's two doors, which
//! the memory-object table answers in the hold that judges the object's state — and the last
//! region gives the token back.
//! OWNERS: @kernel-cap-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host — `cap/tests_prop.rs` (the header's counts against the regions of
//!   several spaces, the refusals leaving nothing behind)
//! INVARIANTS:
//!   - An entry exists while the space has a region of the object; its token is writable
//!     exactly while one of those regions is.
//!   - A refused `add` changes nothing: no entry, no count.
//!
//! RFC: docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md

use alloc::vec::Vec;

use super::Mapped;

struct Entry {
    id: u32,
    regions: u32,
    writable: u32,
    token: Mapped,
}

/// The objects one address space maps.
#[derive(Default)]
pub struct SpaceMaps {
    entries: Vec<Entry>,
}

impl SpaceMaps {
    /// A space that maps nothing.
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// One more region of this space names the object `id`, `writable` or not. The space's
    /// first region of the object takes its token from `first` (which makes it writable or
    /// not, as asked); the first writable region of an object the space maps already asks
    /// `upgrade`. `full`: the error when the kernel heap refuses the entry.
    pub fn add<E>(
        &mut self,
        id: u32,
        writable: bool,
        first: impl FnOnce() -> Result<Mapped, E>,
        upgrade: impl FnOnce(&mut Mapped) -> Result<(), E>,
        full: E,
    ) -> Result<(), E> {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            if writable && entry.writable == 0 {
                upgrade(&mut entry.token)?;
            }
            entry.regions += 1;
            entry.writable += u32::from(writable);
            return Ok(());
        }
        // The entry's room before the token: a refusal leaves no count behind.
        if self.entries.len() == self.entries.capacity() {
            let more = self.entries.capacity().max(4);
            if !crate::kheap::admits(more * core::mem::size_of::<Entry>())
                || self.entries.try_reserve(more).is_err()
            {
                return Err(full);
            }
        }
        let token = first()?;
        self.entries.push(Entry { id, regions: 1, writable: u32::from(writable), token });
        Ok(())
    }

    /// A region of this space that named `id` is gone: the last writable one turns the token
    /// read-only, the last one gives it back.
    pub fn remove(&mut self, id: u32, writable: bool) {
        let Some(at) = self.entries.iter().position(|entry| entry.id == id) else { return };
        let entry = &mut self.entries[at];
        entry.regions = entry.regions.saturating_sub(1);
        if writable {
            entry.writable = entry.writable.saturating_sub(1);
            if entry.writable == 0 {
                entry.token.set_writable(false);
            }
        }
        if entry.regions == 0 {
            drop(self.entries.swap_remove(at));
        }
    }
}
