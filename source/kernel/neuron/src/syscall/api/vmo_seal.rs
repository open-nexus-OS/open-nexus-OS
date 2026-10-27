// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what a VMO's holders may do with it from here on — the read-only alias
//! (`vmo_share_readonly`, RFC-0080: a capability that can only map read-only) and the seal
//! (`vmo_seal`, ADR-0080: the OBJECT has no writer for the rest of its life, whoever holds a
//! capability). Split out of `vmo.rs` (module-size ratchet).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `vmo_ro` host truth table (the alias); `KSELFTEST: exec image reject ok`
//!   (the seal's refusals) and every sealed image of the ladder
//! ADR: docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md

use super::*;

/// `SYSCALL_VMO_SHARE_RO` (51, RFC-0080): derives a READ-ONLY alias
/// (`VmoRo`) of the `Vmo` in `slot` — same physical base/len — into a fresh
/// caller slot, returned as the result. Holders of the alias can only map it
/// read-only (`sys_map` strips WRITE|EXECUTE) and cannot `vmo_write` it. The
/// owner keeps the writable `Vmo` (to fill it); it grants clones of the alias.
pub(super) fn sys_vmo_share_ro(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let slot = args.get(0);
    let cap = ctx.tasks.current_caps_mut().get(slot)?;
    // Only a real (writable) VMO can be downgraded; require MAP authority.
    let CapabilityKind::Vmo { id, len } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    if !cap.rights.contains(Rights::MAP) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let ro = Capability { kind: CapabilityKind::VmoRo { id, len }, rights: Rights::MAP };
    let new_slot = ctx.tasks.current_caps_mut().allocate(ro)?;
    Ok(new_slot)
}

/// Seal the live object `id` (ADR-0080): refused while any address space maps it writably, for
/// a lazy or fixed object, and for one a device was told the runs of. The syscall's core.
pub(super) fn seal_vmo(ctx: &mut Context<'_>, id: u32) -> SysResult<()> {
    if crate::mm::vm_ops::any_space_maps_vmo_writable(ctx.address_spaces, id) {
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    crate::mm::vmo::seal(id).map_err(|err| match err {
        crate::mm::vmo::SealError::NoSuchObject => Error::Capability(CapError::PermissionDenied),
        crate::mm::vmo::SealError::Kind | crate::mm::vmo::SealError::DeviceNamed => {
            AddressSpaceError::InvalidArgs.into()
        }
    })
}

/// `SYSCALL_VMO_SEAL` (64, ADR-0080). Args: (vmo_slot). A holder of the writable `Vmo` with
/// MAP seals it — one way; the object then refuses `vmo_write`, a writable `vm_map` and
/// `vmo_runs`, and is what `exec_vmo` makes a process from.
pub(super) fn sys_vmo_seal(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let cap = ctx.tasks.current_caps_mut().derive(args.get(0), Rights::MAP)?;
    let CapabilityKind::Vmo { id, .. } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    seal_vmo(ctx, id)?;
    Ok(0)
}

/// `SYSCALL_VMO_PURGEABLE` (68, RFC-0100 §4). Args: (vmo_slot, unlock). A holder of the
/// writable `Vmo` with MAP unlocks (1) the object for purging — refused for an eager or fixed
/// object, one a device was told the runs of, or one any space maps — or locks it (0) and learns
/// whether a purge took the content since the unlock (1) or not (0).
pub(super) fn sys_vmo_purgeable(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    use crate::mm::vmo::purge::{self, PurgeError};
    let cap = ctx.tasks.current_caps_mut().derive(args.get(0), Rights::MAP)?;
    let CapabilityKind::Vmo { id, .. } = cap.kind else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    let purge_error = |err: PurgeError| match err {
        PurgeError::NoSuchObject => Error::Capability(CapError::PermissionDenied),
        PurgeError::Refused(_) => AddressSpaceError::InvalidArgs.into(),
    };
    match args.get(1) {
        0 => purge::lock(id).map(usize::from).map_err(purge_error),
        1 => {
            let mapped = crate::mm::vm_ops::any_space_maps_vmo(ctx.address_spaces, id);
            purge::unlock(id, mapped).map(|()| 0).map_err(purge_error)
        }
        _ => Err(AddressSpaceError::InvalidArgs.into()),
    }
}
