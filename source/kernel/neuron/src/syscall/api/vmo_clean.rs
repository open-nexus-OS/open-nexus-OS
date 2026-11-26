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

/// A planned clean and the object's capability it runs under: the clean runs with the kernel
/// lock dropped, and the frames it walks stay the object's while this is held.
pub(crate) struct Clean {
    job: Job,
    _pinned: super::pinned::Pinned,
}

impl Clean {
    /// The clean itself: no lock held.
    pub(crate) fn run(&self) {
        crate::mm::vmo::clean::run(&self.job);
    }
}

/// Phase A (BKL held): the clean `(vmo_slot, offset, len)` asks for — `None` when there is
/// nothing to clean (a coherent device's object, an empty range).
pub(crate) fn vmo_clean_plan(ctx: &mut Context<'_>, args: &Args) -> Result<Option<Clean>, Error> {
    let slot = SlotIndex::decode(args.get(0));
    let (offset, len) = (args.get(1), args.get(2));
    let pinned = super::pinned::pin(&ctx.tasks.current_caps_mut(), slot.0, Rights::MAP, "clean")?;
    let Some((id, cap_len, read_only)) = pinned.object() else {
        return Err(Error::Capability(CapError::PermissionDenied));
    };
    let writable = !read_only && !crate::mm::vmo::is_sealed(id);
    crate::clean_span::admits(writable, offset, len, cap_len).map_err(|refusal| match refusal {
        CleanRefusal::NotWritable => Error::Capability(CapError::PermissionDenied),
        CleanRefusal::OutOfRange => Error::from(AddressSpaceError::InvalidArgs),
    })?;
    // A frame the pool does not own (a device window) is never cleaned.
    let job = crate::mm::vmo::clean::plan(id, offset, len)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    Ok(job.map(|job| Clean { job, _pinned: pinned }))
}

/// The unphased `SYSCALL_VMO_CLEAN` (selftest-owned dispatch tables): the clean under the lock.
pub(super) fn sys_vmo_clean(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    if let Some(clean) = vmo_clean_plan(ctx, args)? {
        clean.run();
    }
    Ok(0)
}
