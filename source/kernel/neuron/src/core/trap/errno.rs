// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: -errno encoding — the ONE table that turns every kernel
//! [`SysError`] into the negative POSIX code userspace decodes
//! (`nexus-abi::AbiError::from_raw`). Split out of `handler.rs`
//! (structure-gate). ADR-0054 rule: NO wildcard arms over an error enum —
//! a new variant must fail compilation until someone names its errno.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable wire values (POSIX numbers, used with POSIX meaning)
//! ADR: docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md

use super::*;

pub(super) const EPERM: usize = 1;
pub(super) const ENOMEM: usize = 12;
pub(super) const EAGAIN: usize = 11;
pub(super) const EINVAL: usize = 22;
pub(super) const ENOSPC: usize = 28;
pub(super) const ENOSYS: usize = 38;
pub(super) const ESRCH: usize = 3;
pub(super) const ECHILD: usize = 10;
pub(super) const ETIMEDOUT: usize = 110;
pub(super) const EPIPE: usize = 32; // RFC-0079: EOF-opted recv, last sender gone.
                                    // ADR-0054: map failures stop collapsing into EINVAL. A service debugging a
                                    // refused mapping needs to know WHICH refusal without a kernel log.
pub(super) const EEXIST: usize = 17; // MapError::Overlap — something is already mapped there.
pub(super) const EFAULT: usize = 14; // MapError::OutOfRange — VA outside the canonical range.
                                     // RFC-0085: vm_unmap of an address nothing is mapped at.
pub(super) const ENOENT: usize = 2; // MapError::NotMapped / VaError::NotFound — no mapping there.
pub(super) const EBUSY: usize = 16; // RFC-0085: vmo_destroy with live vm_map regions.

#[allow(dead_code)]
pub(super) fn encode_error(err: SysError) -> usize {
    match err {
        SysError::InvalidSyscall => errno(ENOSYS),
        SysError::Capability(cap) => match cap {
            crate::cap::CapError::NoSpace => errno(ENOSPC),
            _ => errno(EPERM),
        },
        SysError::Ipc(ipc_err) => ipc_errno(&ipc_err),
        SysError::Spawn(spawn) => spawn_errno(&spawn),
        SysError::Transfer(_) => errno(EPERM),
        SysError::AddressSpace(as_err) => address_space_errno(&as_err),
        SysError::Wait(wait) => wait_errno(&wait),
        SysError::TaskExit => errno(EINVAL),
        SysError::Reschedule => errno(EAGAIN),
        SysError::InvalidTarget => errno(ESRCH),
        SysError::RunQueueFull => errno(ENOSPC),
        SysError::Va(va) => va_errno(&va),
        SysError::ResourceBusy => errno(EBUSY),
    }
}

/// RFC-0085 policy refusals — exhaustive (ADR-0054: a new `VaError` variant
/// must fail compilation until someone names its errno).
#[allow(dead_code)]
pub(super) fn va_errno(err: &crate::va_space::VaError) -> usize {
    use crate::va_space::VaError::*;
    match err {
        WindowExhausted => errno(ENOMEM),
        TableFull => errno(ENOSPC),
        NotFound => errno(ENOENT),
        LenMismatch | BadInput => errno(EINVAL),
        FixedRegion => errno(EPERM),
        Occupied => errno(EEXIST),
    }
}

#[allow(dead_code)]
pub(super) fn ipc_errno(err: &crate::ipc::IpcError) -> usize {
    match err {
        crate::ipc::IpcError::NoSuchEndpoint => errno(ESRCH),
        crate::ipc::IpcError::QueueFull | crate::ipc::IpcError::QueueEmpty => errno(EAGAIN),
        crate::ipc::IpcError::PermissionDenied => errno(EPERM),
        crate::ipc::IpcError::TimedOut => errno(ETIMEDOUT),
        crate::ipc::IpcError::NoSpace => errno(ENOSPC),
        crate::ipc::IpcError::PeerClosed => errno(EPIPE),
    }
}

#[allow(dead_code)]
pub(super) fn spawn_errno(err: &task::SpawnError) -> usize {
    use task::SpawnError::*;
    match err {
        InvalidParent | InvalidEntryPoint | InvalidStackPointer => errno(EINVAL),
        BootstrapNotEndpoint => errno(EPERM),
        Capability(_) => errno(EPERM),
        Ipc(_) => errno(EINVAL),
        AddressSpace(as_err) => address_space_errno(as_err),
        StackExhausted => errno(ENOMEM),
        RunQueueFull => errno(EAGAIN),
    }
}

#[allow(dead_code)]
pub(super) fn address_space_errno(err: &AddressSpaceError) -> usize {
    match err {
        AddressSpaceError::InvalidHandle | AddressSpaceError::InvalidArgs => errno(EINVAL),
        AddressSpaceError::AsidExhausted => errno(ENOSPC),
        AddressSpaceError::InUse => errno(EPERM),
        AddressSpaceError::Unsupported => errno(ENOSYS),
        AddressSpaceError::Mapping(MapError::PermissionDenied) => errno(EPERM),
        // ADR-0054: each map refusal keeps its identity across the ABI. The
        // old wildcard arm folded Overlap/Unaligned/OutOfRange/InvalidFlags
        // into one EINVAL — the same defect class as a `_ => "other"` log arm,
        // in the one table every service depends on. No wildcard: a new
        // MapError variant must fail compilation until someone names its errno.
        AddressSpaceError::Mapping(MapError::Overlap) => errno(EEXIST),
        AddressSpaceError::Mapping(MapError::OutOfRange) => errno(EFAULT),
        AddressSpaceError::Mapping(MapError::Unaligned)
        | AddressSpaceError::Mapping(MapError::InvalidFlags) => errno(EINVAL),
        AddressSpaceError::Mapping(MapError::NotMapped) => errno(ENOENT),
    }
}

#[allow(dead_code)]
pub(super) fn wait_errno(err: &task::WaitError) -> usize {
    use task::WaitError::*;
    match err {
        NoChildren => errno(ECHILD),
        NoSuchPid => errno(ESRCH),
        InvalidTarget => errno(EINVAL),
        WouldBlock => errno(EINVAL),
    }
}

pub(super) const fn errno(code: usize) -> usize {
    (-(code as isize)) as usize
}
