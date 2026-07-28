// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Stable error-name tables (ADR-0054 family): every [`AbiError`] and
//! [`nexus_abi::SpawnFailReason`] variant named individually, NO catch-all —
//! a new variant must fail compilation until someone names it here. Split
//! out of `helpers.rs` (structure-gate).

use nexus_abi::AbiError;

pub(crate) fn abi_error_label(err: AbiError) -> &'static str {
    match err {
        AbiError::InvalidSyscall => "invalid-syscall",
        AbiError::CapabilityDenied => "capability-denied",
        AbiError::IpcFailure => "ipc-failure",
        AbiError::SpawnFailed => "spawn-failed",
        AbiError::TransferFailed => "transfer-failed",
        AbiError::ChildUnavailable => "child-unavailable",
        AbiError::NoSuchPid => "no-such-pid",
        AbiError::InvalidArgument => "invalid-argument",
        AbiError::TimedOut => "timed-out",
        AbiError::WouldBlock => "would-block",
        AbiError::AlreadyExists => "already-exists",
        AbiError::BadAddress => "bad-address",
        AbiError::Unknown => "unknown-errno",
        AbiError::Unsupported => "unsupported",
    }
}

pub(crate) fn spawn_fail_reason_label(reason: nexus_abi::SpawnFailReason) -> &'static str {
    match reason {
        nexus_abi::SpawnFailReason::Unknown => "unknown",
        nexus_abi::SpawnFailReason::OutOfMemory => "oom",
        nexus_abi::SpawnFailReason::CapTableFull => "cap-table-full",
        nexus_abi::SpawnFailReason::EndpointQuota => "endpoint-quota",
        nexus_abi::SpawnFailReason::MapFailed => "map-failed",
        nexus_abi::SpawnFailReason::InvalidPayload => "invalid-payload",
        nexus_abi::SpawnFailReason::DeniedByPolicy => "denied-by-policy",
    }
}
