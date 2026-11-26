// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a memory object counts the address spaces that map it (RFC-0102, TASK-0330 P3). A
//! space that records its first region of an object takes a `Mapped` here; a space that maps
//! it already and records its first writable region asks here too. Both are answered in one
//! hold of the object table with the state the answer depends on — an object unlocked for
//! purging is mapped by nobody, a sealed one by no writer — and the seal, the unlock and the
//! destroy read the counts in their own hold of the same table. So a mapping (which runs
//! without the kernel lock) and a seal can no longer each see the other not there yet. A child
//! module of `vmo`: it reaches the table directly.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host — the counts and the entries (`cap/tests_prop.rs`); QEMU —
//!   `KSELFTEST: exec image …` (two spaces map one image, a writable mapper refuses the seal),
//!   `KSELFTEST: vm map reject ok` (a destroy while mapped), `KSELFTEST: vm unmap reclaim ok`
//! INVARIANTS: a `Mapped` of an object is made only here, under the table's lock; the check of
//!   the object's state and the count are one hold

use super::{held_counts, VmoId, VmoObject, TABLE};
use crate::cap::Mapped;

/// Why an object refused a mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapRefusal {
    /// No live object has this id.
    NoSuchObject,
    /// A sealed object has no writer (ADR-0080).
    Sealed,
    /// An object unlocked for purging is untouchable until its lock (RFC-0100 §4).
    Unlocked,
    /// The kernel heap refused the space's record of the object (ADR-0091).
    Heap,
}

fn admits(object: &VmoObject, writable: bool) -> Result<(), MapRefusal> {
    if object.purgeable {
        return Err(MapRefusal::Unlocked);
    }
    if writable && object.sealed {
        return Err(MapRefusal::Sealed);
    }
    Ok(())
}

/// An address space begins to map `id`, with a writable region or not — or a syscall begins
/// to copy through its pages (`vmo_write` as a writer, `vmo_read`; TASK-0330 P4): for the
/// copy's length it reaches the object's frames as a mapping does, and is counted as one.
pub fn begin(id: VmoId, writable: bool) -> Result<Mapped, MapRefusal> {
    let table = TABLE.lock();
    let object = table.get(id).ok_or(MapRefusal::NoSuchObject)?;
    admits(object, writable)?;
    let counts = object.held.as_ref().ok_or(MapRefusal::NoSuchObject)?;
    counts.map(writable).ok_or(MapRefusal::NoSuchObject)
}

/// A space that maps `id` (its `token`) records its first writable region of it.
pub fn make_writable(id: VmoId, token: &mut Mapped) -> Result<(), MapRefusal> {
    let table = TABLE.lock();
    admits(table.get(id).ok_or(MapRefusal::NoSuchObject)?, true)?;
    token.set_writable(true);
    Ok(())
}

/// Address spaces that map the live object `id`, and copies in flight through its pages (0 for
/// one that is gone).
pub fn spaces(id: VmoId) -> usize {
    held_counts(id).map_or(0, |counts| counts.mapped())
}

/// Those that hold a writable region of it.
pub fn writers(id: VmoId) -> usize {
    held_counts(id).map_or(0, |counts| counts.mapped_writable())
}
