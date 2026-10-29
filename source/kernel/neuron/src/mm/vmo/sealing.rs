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
}

/// Seal `id` (ADR-0080): the object has no writer for the rest of its life. Idempotent. The
/// caller has proven no address space maps it writably (`vm_ops::any_space_maps_vmo_writable`).
pub fn seal(id: VmoId) -> Result<(), SealError> {
    let mut table = TABLE.lock();
    let object = table.get_mut(id).ok_or(SealError::NoSuchObject)?;
    if object.lazy.is_some() || object.kind == VmoKind::Fixed {
        return Err(SealError::Kind);
    }
    if object.device_named {
        return Err(SealError::DeviceNamed);
    }
    object.sealed = true;
    Ok(())
}

/// Whether the live object `id` is sealed (ADR-0080).
pub fn is_sealed(id: VmoId) -> bool {
    with(id, |o| o.sealed).unwrap_or(false)
}

/// Record that a device was told `id`'s runs (`vmo_runs`): never sealable from now on.
pub fn mark_device_named(id: VmoId) {
    if let Some(object) = TABLE.lock().get_mut(id) {
        object.device_named = true;
    }
}
