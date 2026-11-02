// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `SYSCALL_VMO_CLEAN` (74, ADR-0092 — ADR-0079's deferred maintenance call): a writer
//! of an object made for a non-coherent device cleans the CPU caches over what it wrote, so the
//! device reads memory that holds it. Only an object the caller may WRITE (`Vmo`, not a read-only
//! alias, not sealed), only inside its capability; a coherent device's object is a no-op. The
//! trap handler runs it phased (the clean with the BKL dropped); selftest tables run it whole.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host `crate::clean_span` (the range rule); QEMU `KSELFTEST: vmo clean ok (`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::*;
use crate::clean_span::CleanRefusal;
use crate::mm::vmo::clean::Job;

/// Phase A (BKL held): the clean `(vmo_slot, offset, len)` asks for — `None` when there is
/// nothing to clean (a coherent device's object, an empty range).
pub(crate) fn vmo_clean_plan(ctx: &mut Context<'_>, args: &Args) -> Result<Option<Job>, Error> {
    let slot = SlotIndex::decode(args.get(0));
    let (offset, len) = (args.get(1), args.get(2));
    let cap = ctx.tasks.current_caps_mut().derive(slot.0, Rights::MAP)?;
    let (id, cap_len, writable) = match cap.kind {
        CapabilityKind::Vmo { id, len } => (id, len, !crate::mm::vmo::is_sealed(id)),
        CapabilityKind::VmoRo { id, len } => (id, len, false),
        _ => return Err(Error::Capability(CapError::PermissionDenied)),
    };
    crate::clean_span::admits(writable, offset, len, cap_len).map_err(|refusal| match refusal {
        CleanRefusal::NotWritable => Error::Capability(CapError::PermissionDenied),
        CleanRefusal::OutOfRange => Error::from(AddressSpaceError::InvalidArgs),
    })?;
    // A frame the pool does not own (a device window) is never cleaned.
    crate::mm::vmo::clean::plan(id, offset, len)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))
}

/// The unphased `SYSCALL_VMO_CLEAN` (selftest-owned dispatch tables): the clean under the lock.
pub(super) fn sys_vmo_clean(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    if let Some(job) = vmo_clean_plan(ctx, args)? {
        crate::mm::vmo::clean::run(&job);
    }
    Ok(0)
}
