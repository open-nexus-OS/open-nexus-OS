// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sealed objects (ADR-0080) — an object with no writer for the rest of its life, the
//! only kind `exec_vmo` maps a process from, and the device-named mark that forbids the seal (a
//! device told an object's runs may write them at any time). A child module of `vmo`: it reaches
//! the table directly; split out of `vmo.rs` to keep it under the module-size ratchet when the
//! swap stage (TASK-0333 6f-2) grew the commit path.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal (the syscall is `SYSCALL_VMO_SEAL`, 64)
//! TEST_COVERAGE: `KSELFTEST: exec image …` (`selftest/exec_image.rs`: the seal, the lazy and
//!   device-named refusals, a sealed object never mapped writable)
//! INVARIANTS: a sealed object stays sealed; a device-named object is never sealed

use super::{with, VmoId, VmoKind, TABLE};

/// Why a seal was refused (ADR-0080).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// No live object has this id.
    NoSuchObject,
    /// A lazy or fixed object: its pages are not all the object's own, committed frames.
    Kind,
    /// A device was told its runs and may write them at any time.
    DeviceNamed,
    /// An address space holds a writable region of it.
    MappedWritable,
}

/// Seal `id` (ADR-0080): the object has no writer for the rest of its life. Idempotent.
/// Refused while an address space maps it writably or a write is in flight (`vmo_write`) — the
/// object's own count, read in the hold that sets the seal: a mapping and a copy take their
/// count in a hold of the same table (`mapping`).
pub fn seal(id: VmoId) -> Result<(), SealError> {
    let mut table = TABLE.lock();
    let object = table.get_mut(id).ok_or(SealError::NoSuchObject)?;
    if object.lazy.is_some() || object.kind == VmoKind::Fixed {
        return Err(SealError::Kind);
    }
    if object.device_named {
        return Err(SealError::DeviceNamed);
    }
    if object.held.as_ref().is_some_and(|counts| counts.mapped_writable() > 0) {
        return Err(SealError::MappedWritable);
    }
    object.sealed = true;
    Ok(())
}

/// Whether the live object `id` is sealed (ADR-0080).
pub fn is_sealed(id: VmoId) -> bool {
    with(id, |o| o.sealed).unwrap_or(false)
}

/// A device is about to be told `id`'s runs (`vmo_runs`): refused (`false`) for a sealed object
/// (ADR-0080: it has no writer, and the device could write) and for one unlocked for purging
/// (RFC-0100 §4: a purge would take back frames the device reads); from here on the object is
/// never sealable, never unlockable, never squeezed. The check and the mark are one hold of
/// the table: the syscall runs without the kernel lock (TASK-0330 P3), so neither a seal nor
/// an unlock may slip between them.
#[must_use]
pub fn name_for_device(id: VmoId) -> bool {
    match TABLE.lock().get_mut(id) {
        Some(object) if !object.sealed && !object.purgeable => {
            object.device_named = true;
            true
        }
        _ => false,
    }
}
