// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared syscall types — Rights, Pid/Cap/Handle/AsHandle, SysResult, AbiError, SpawnFailReason, QosClass
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

// ——— Task and capability primitives (OS build) ———

#[cfg(nexus_env = "os")]
bitflags::bitflags! {
    /// Rights mask accepted by capability-transfer syscalls.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Rights: u32 {
        /// Permit the holder to send messages through the endpoint.
        const SEND = 1 << 0;
        /// Permit the holder to receive messages from the endpoint.
        const RECV = 1 << 1;
        /// Permit the holder to map VMOs into its address space.
        const MAP = 1 << 2;
        /// Permit the holder to manage capabilities (reserved for kernel tests).
        const MANAGE = 1 << 3;
    }
}

/// Kernel task identifier returned from [`spawn`].
#[cfg(nexus_env = "os")]
pub type Pid = u32;

/// Capability slot handle returned from [`cap_transfer`].
#[cfg(nexus_env = "os")]
pub type Cap = u32;

/// Handle identifying a virtual memory object (VMO).
#[cfg(nexus_env = "os")]
pub type Handle = u32;

/// Opaque handle referencing a user address space managed by the kernel.
#[cfg(nexus_env = "os")]
pub type AsHandle = u64;

/// Result returned by privileged syscalls that expose kernel operations.
#[cfg(nexus_env = "os")]
pub type SysResult<T> = core::result::Result<T, AbiError>;

/// Errors surfaced when invoking privileged syscalls from userland.
#[cfg(nexus_env = "os")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiError {
    /// Syscall number is not implemented by the kernel build.
    InvalidSyscall,
    /// Kernel rejected the request due to missing rights or invalid slots.
    CapabilityDenied,
    /// Kernel-side IPC machinery reported a routing error.
    IpcFailure,
    /// Kernel rejected process creation.
    SpawnFailed,
    /// Kernel rejected capability transfer.
    TransferFailed,
    /// Caller does not have any children to wait on.
    ChildUnavailable,
    /// Requested process identifier does not belong to the caller.
    NoSuchPid,
    /// Syscall arguments were invalid for the requested operation.
    InvalidArgument,
    /// The operation's deadline elapsed (ETIMEDOUT).
    TimedOut,
    /// The operation would block / resource temporarily unavailable (EAGAIN).
    WouldBlock,
    /// Kernel returned an error code this ABI build does not know. NEVER
    /// treated as success (fail closed) — a Phase C workpool hang traced back
    /// to -ETIMEDOUT being decoded as Ok.
    Unknown,
    /// Operation unsupported on the current build target.
    Unsupported,
}

/// Spawn failure reasons reported by the kernel (RFC-0013).
#[cfg(nexus_env = "os")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnFailReason {
    /// Unknown/unspecified reason.
    Unknown = 0,
    /// Allocation or memory exhaustion.
    OutOfMemory = 1,
    /// Capability table exhausted.
    CapTableFull = 2,
    /// IPC endpoint quota exhausted.
    EndpointQuota = 3,
    /// Address-space map or handle failure.
    MapFailed = 4,
    /// Invalid or malformed payload/arguments.
    InvalidPayload = 5,
    /// Spawn denied by policy (if gating applies).
    DeniedByPolicy = 6,
}

/// Scheduler quality-of-service hint classes (stable wire values).
#[cfg(nexus_env = "os")]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QosClass {
    /// Lowest-priority background work.
    Idle = 0,
    /// Default service-level scheduling class.
    Normal = 1,
    /// Latency-sensitive interactive class.
    Interactive = 2,
    /// Highest-performance burst class.
    PerfBurst = 3,
}

#[cfg(nexus_env = "os")]
impl QosClass {
    /// Decodes a kernel wire value into a typed QoS class.
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Normal),
            2 => Some(Self::Interactive),
            3 => Some(Self::PerfBurst),
            _ => None,
        }
    }
}

#[cfg(nexus_env = "os")]
impl SpawnFailReason {
    /// Decodes a reason token into the enum, defaulting to Unknown.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::OutOfMemory,
            2 => Self::CapTableFull,
            3 => Self::EndpointQuota,
            4 => Self::MapFailed,
            5 => Self::InvalidPayload,
            6 => Self::DeniedByPolicy,
            _ => Self::Unknown,
        }
    }
}

#[cfg(nexus_env = "os")]
impl AbiError {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    pub(crate) fn from_raw(value: usize) -> Option<Self> {
        if (value as isize) >= 0 {
            return None;
        }
        // Kernel returns negative errno values for syscall failures.
        match -(value as isize) as usize {
            38 => Some(Self::InvalidSyscall),   // ENOSYS
            1 => Some(Self::CapabilityDenied),  // EPERM
            22 => Some(Self::InvalidArgument),  // EINVAL
            10 => Some(Self::ChildUnavailable), // ECHILD
            3 => Some(Self::NoSuchPid),         // ESRCH
            12 => Some(Self::SpawnFailed),      // ENOMEM (best-effort mapping)
            28 => Some(Self::SpawnFailed),      // ENOSPC (best-effort mapping)
            110 => Some(Self::TimedOut),        // ETIMEDOUT
            11 => Some(Self::WouldBlock),       // EAGAIN
            // Fail closed: an unknown NEGATIVE code is an error, never a
            // success value (Phase C: -ETIMEDOUT used to decode as Ok and
            // turned every fence/waitset timeout into a silent pseudo-Ok).
            _ => Some(Self::Unknown),
        }
    }
}
