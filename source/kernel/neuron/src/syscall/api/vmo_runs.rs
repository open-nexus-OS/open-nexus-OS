// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `SYSCALL_VMO_RUNS` (60) — the one door a physical address leaves the
//! kernel by (RFC-0098 C4, TASK-0286 P4a). Resolves the capability in the slot,
//! applies the authority rule and the clipping of `crate::dma_runs` (pure,
//! host-tested) to the object's runs, and copies the answer out. A driver uses it
//! for a queue's one base (a `contiguous-DMA` object answers with one run) and for
//! a scatter-gather list (an anonymous object answers with its blocks).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `dma_runs` host tests (reject matrix); `KSELFTEST: vmo runs ok (…)`
//!   drives `resolve_runs` on QEMU; every virtio driver and gpud take their bases
//!   through it on every boot

use super::*;
use crate::dma_runs::{self, Holding, RunsError, MAX_RUNS, RUN_BYTES};

fn runs_error(err: RunsError) -> Error {
    match err {
        RunsError::NotVmo | RunsError::ReadOnly | RunsError::NoDevice => {
            Error::Capability(CapError::PermissionDenied)
        }
        RunsError::Range | RunsError::TooMany => AddressSpaceError::InvalidArgs.into(),
    }
}

/// The runs behind `offset..offset + len` of the object in the current task's
/// `slot`, into `out`; returns how many. The syscall's core without the user copy,
/// so the kernel selftest can drive it with kernel buffers.
pub(crate) fn resolve_runs(
    ctx: &mut Context<'_>,
    slot: usize,
    offset: usize,
    len: usize,
    out: &mut [(u64, u64)],
) -> SysResult<usize> {
    let caps = ctx.tasks.current_caps_mut();
    let holds_device = caps.holds_device();
    let cap = caps.derive(slot, Rights::MAP)?;
    let (holding, id, object_len) = match cap.kind {
        CapabilityKind::Vmo { id, len } => (Holding::Vmo, id, len),
        CapabilityKind::VmoRo { id, len } => (Holding::ReadOnlyAlias, id, len),
        _ => (Holding::Other, 0, 0),
    };
    dma_runs::authorize(holds_device, holding).map_err(runs_error)?;
    let runs = crate::mm::vmo::runs(id).ok_or(Error::Capability(CapError::PermissionDenied))?;
    dma_runs::clip(runs, object_len, offset, len, out).map_err(runs_error)
}

/// `SYSCALL_VMO_RUNS` (60). Args: (vmo_slot, offset, len, out_ptr, max).
pub(super) fn sys_vmo_runs(ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let (slot, offset, len, out_ptr, max) =
        (args.get(0), args.get(1), args.get(2), args.get(3), args.get(4));
    if max == 0 || max > MAX_RUNS {
        return Err(AddressSpaceError::InvalidArgs.into());
    }
    ensure_user_slice(out_ptr, max * RUN_BYTES)?;
    let mut out = alloc::vec![(0u64, 0u64); max];
    let count = resolve_runs(ctx, slot, offset, len, &mut out)?;
    for (i, (pa, run_len)) in out[..count].iter().enumerate() {
        let mut bytes = [0u8; RUN_BYTES];
        bytes[..8].copy_from_slice(&pa.to_le_bytes());
        bytes[8..].copy_from_slice(&run_len.to_le_bytes());
        // SAFETY: `out_ptr..out_ptr + max * RUN_BYTES` was checked to be a user
        // slice above and `i < count ≤ max`.
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (out_ptr + i * RUN_BYTES) as *mut u8,
                RUN_BYTES,
            );
        }
    }
    Ok(count)
}
