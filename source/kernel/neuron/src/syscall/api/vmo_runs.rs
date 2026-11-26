// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `SYSCALL_VMO_RUNS` (60) — the one door a physical address leaves the
//! kernel by (RFC-0098 C4, TASK-0286 P4a), device-scoped since TASK-0246 P1: the
//! caller names the VMO AND the device it programs, and the answer is in that
//! device's bus addresses. Resolves both capabilities, applies the authority rule and
//! the clip-then-translate of `crate::dma_runs` (pure, host-tested) to the object's
//! runs and the device's reach (`mm::devices`), and copies the answer out. A driver
//! uses it for a queue's one base (a contiguous object answers with one run) and for
//! a scatter-gather list (an anonymous object answers with its blocks). A lazy object
//! commits the range it is asked for first — a device cannot fault (ADR-0078) — at
//! most `lazy::MAX_NAMED_PAGES` pages a call.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `dma_runs` host tests (reject matrix); `KSELFTEST: vmo runs ok (…)`
//!   and `KSELFTEST: vmo reach ok (…)` drive `resolve_runs` on QEMU; every virtio
//!   driver and gpud take their bases through it on every boot

use super::ipc_ctx::{locked, Ipc, IpcResult, Outcome};
use super::*;
use crate::dma_runs::{self, Holding, RunsError, MAX_RUNS, RUN_BYTES};

fn runs_error(err: RunsError) -> Error {
    match err {
        RunsError::NotVmo | RunsError::ReadOnly | RunsError::NoDevice => {
            Error::Capability(CapError::PermissionDenied)
        }
        RunsError::Range | RunsError::TooMany | RunsError::OutOfReach => {
            AddressSpaceError::InvalidArgs.into()
        }
    }
}

/// The bus runs behind `offset..offset + len` of the object in the current task's
/// `vmo_slot`, as the device in `device_slot` addresses them, into `out`; returns how
/// many. The syscall's core without the user copy, so the kernel selftest can drive
/// it with kernel buffers.
pub(crate) fn resolve_runs(
    ctx: &mut Context<'_>,
    vmo_slot: usize,
    device_slot: usize,
    offset: usize,
    len: usize,
    out: &mut [(u64, u64)],
) -> SysResult<usize> {
    runs_for(&ctx.ipc()?, vmo_slot, device_slot, offset, len, out)
}

/// [`resolve_runs`] over the caller's record: nothing here is the kernel lock's (TASK-0330
/// P3). The object is the caller's for the whole call — the capability is held, counted,
/// until the answer is made (a sibling thread closing the slot meanwhile frees nothing).
///
/// The device learns the runs only of an object claimed for it (`vmo::name_for_device`: not
/// sealed, not unlocked for purging, and neither from then on). A lazy object is claimed before
/// its pages are committed and named — a purge between the two would take them back; an eager
/// one after its runs passed the range and the reach, so a refused call leaves it sealable.
/// A lazy object's call refused after the claim (no frame, out of the device's reach) leaves
/// it claimed: fail-closed.
fn runs_for(
    ipc: &Ipc<'_>,
    vmo_slot: usize,
    device_slot: usize,
    offset: usize,
    len: usize,
    out: &mut [(u64, u64)],
) -> SysResult<usize> {
    // Both names in one hold of the table; the object's work below runs without its lock.
    let (device, pinned) = {
        let caps = ipc.caps();
        (caps.device(device_slot).ok(), super::pinned::pin(&caps, vmo_slot, Rights::MAP, "runs")?)
    };
    runs_of(pinned.cap, device, offset, len, out)
}

fn runs_of(
    cap: Capability,
    device: Option<u16>,
    offset: usize,
    len: usize,
    out: &mut [(u64, u64)],
) -> SysResult<usize> {
    let (holding, id, object_len) = match cap.kind {
        CapabilityKind::Vmo { id, len } => (Holding::Vmo, id, len),
        CapabilityKind::VmoRo { id, len } => (Holding::ReadOnlyAlias, id, len),
        _ => (Holding::Other, 0, 0),
    };
    dma_runs::authorize(device.is_some(), holding).map_err(runs_error)?;
    let reach = device
        .and_then(crate::mm::devices::reach)
        .ok_or(Error::Capability(CapError::PermissionDenied))?;
    if crate::mm::vmo::is_sealed(id) {
        // ADR-0080: a sealed object has no writer — a device told its runs could write them.
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    if crate::mm::vmo::purge::is_unlocked(id) {
        // RFC-0100 §4: an object unlocked for purging is nobody's to name until its lock — a
        // device told its runs would read frames a purge takes back.
        return Err(Error::Capability(CapError::PermissionDenied));
    }
    let claim = || {
        crate::mm::vmo::name_for_device(id)
            .then_some(())
            .ok_or(Error::Capability(CapError::PermissionDenied))
    };
    let count = if crate::mm::vmo::is_lazy(id) {
        // A device cannot fault: the range is committed before it is named (ADR-0078), at most
        // `MAX_NAMED_PAGES` pages a call, and answered as an object of its own from byte 0.
        claim()?;
        let runs = crate::mm::vmo::named_runs(id, offset, len).map_err(|e| match e {
            crate::mm::vmo::VmoError::Exhausted => {
                Error::AddressSpace(AddressSpaceError::OutOfFrames)
            }
            e => super::vmo::span_error(e, AddressSpaceError::InvalidArgs.into()),
        })?;
        dma_runs::device_runs(runs, len, 0, len, &reach, out).map_err(runs_error)?
    } else {
        let runs = crate::mm::vmo::runs(id).ok_or(Error::Capability(CapError::PermissionDenied))?;
        let count = dma_runs::device_runs(runs, object_len, offset, len, &reach, out)
            .map_err(runs_error)?;
        // The device has the runs from here on: the object can never be sealed (ADR-0080).
        claim()?;
        count
    };
    Ok(count)
}

/// `SYSCALL_VMO_RUNS` (60) under the kernel lock (the syscall table: a selftest, a kernel-mode
/// task, the re-run of a body that asked for it): the answer buffer is committed first.
pub(super) fn sys_vmo_runs(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let max = args.get(5);
    if max == 0 || max > MAX_RUNS {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    ensure_user_range(ctx, args.get(4), max * RUN_BYTES, UserAccess::Write)?;
    let out = vmo_runs(&ctx.ipc()?, args);
    locked(ctx, out)
}

/// `SYSCALL_VMO_RUNS` (60). Args: (vmo_slot, device_slot, offset, len, out_ptr, max). The body
/// of both entries; the answer is written through the caller's page table, and a page of the
/// buffer that is not ready asks for the lock — the call re-executes whole (a claim it made
/// stands; the runs are the same).
pub(super) fn vmo_runs(ipc: &Ipc<'_>, args: &Args) -> IpcResult {
    let (vmo_slot, device_slot, offset, len, out_ptr, max) =
        (args.get(0), args.get(1), args.get(2), args.get(3), args.get(4), args.get(5));
    if max == 0 || max > MAX_RUNS {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    let mut out = alloc::vec![(0u64, 0u64); max];
    let count = runs_for(ipc, vmo_slot, device_slot, offset, len, &mut out)?;
    let mut bytes = alloc::vec![0u8; count * RUN_BYTES];
    for ((bus, run_len), slot) in out[..count].iter().zip(bytes.chunks_exact_mut(RUN_BYTES)) {
        slot[..8].copy_from_slice(&bus.to_le_bytes());
        slot[8..].copy_from_slice(&run_len.to_le_bytes());
    }
    if !ipc.write(out_ptr, &bytes) {
        return Ok(Outcome::Locked);
    }
    Ok(Outcome::Done(count))
}
