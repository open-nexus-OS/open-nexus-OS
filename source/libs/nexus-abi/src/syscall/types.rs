// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared syscall types — Rights, Pid/Cap/Handle/AsHandle, SysResult, AbiError, SpawnFailReason, QosClass
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

// ——— Task and capability primitives (OS build) ———

// `Rights` is cfg-free on purpose: pure bitflags data consumed by
// host-tested logic (init's RouteTable, ADR-0057) — same rule as
// `ExitReason` below. The syscalls taking it stay os-gated.
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
    /// The target is already occupied (EEXIST) — for a map syscall: a page is
    /// already mapped at that virtual address (ADR-0054). Before this variant
    /// every map refusal arrived as `InvalidArgument`, which cost a full
    /// instrumented-boot investigation to tell "you passed nonsense" apart
    /// from "someone was here first" (TASK-0309).
    AlreadyExists,
    /// The address itself is bad (EFAULT) — outside the canonical range.
    BadAddress,
    /// Memory/window exhausted (ENOMEM) — RFC-0085: `vm_map` found no hole
    /// of the requested size. Formerly collapsed into `SpawnFailed`; the
    /// spawn wrappers now translate locally instead.
    OutOfMemory,
    /// A bounded table/quota is full (ENOSPC) — RFC-0085: the region table.
    /// Formerly collapsed into `SpawnFailed` (spawn wrappers translate).
    NoSpace,
    /// Nothing there (ENOENT) — RFC-0085: `vm_unmap` of an unmapped va.
    NotFound,
    /// The resource still has live users (EBUSY) — RFC-0085: `vmo_destroy`
    /// while an address space still maps the range.
    Busy,
    /// Kernel returned an error code this ABI build does not know. NEVER
    /// treated as success (fail closed) — a Phase C workpool hang traced back
    /// to -ETIMEDOUT being decoded as Ok.
    Unknown,
    /// Operation unsupported on the current build target.
    Unsupported,
}

/// WHY a child died — kernel truth delivered alongside the exit code by
/// `wait_with_reason`/`wait_nohang_with_reason` (ADR-0056). `Clean`/`Error`
/// are the consumer-side split of a voluntary exit by its code; `Fault` and
/// `Killed` come from the kernel and can never be asserted by the child.
/// (cfg-free on purpose: the decode is pure logic, host-tested below.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// Voluntary `exit(0)`.
    Clean,
    /// Voluntary `exit(code != 0)` (panic-abort paths land here too).
    Error,
    /// Kernel killed the task on a trap. `cause` is the scause low byte, or
    /// a synthetic code ≥ 0xF0 (0xF1 = ecall from unmapped sepc).
    Fault {
        /// scause low byte / synthetic ≥0xF0 code.
        cause: u8,
    },
    /// An authority terminated the task (kernel fail-fast; policy/OOM later).
    Killed,
    /// The kernel sent a reason tag this ABI build does not know. NEVER
    /// folded into another variant (ADR-0054 discipline; fail closed).
    Unknown,
}

impl ExitReason {
    /// Decodes the packed a1 status register from `wait`/`wait_nohang`:
    /// low 32 bits = exit code, high 32 bits = the kernel wire word
    /// (low byte reason tag 0/2/3, next byte fault cause).
    pub fn decode_wait_status(raw_status: usize) -> (i32, Self) {
        let code = raw_status as u32 as i32;
        let word = (raw_status >> 32) as u32;
        let reason = match word & 0xff {
            0 => {
                if code == 0 {
                    Self::Clean
                } else {
                    Self::Error
                }
            }
            2 => Self::Fault { cause: ((word >> 8) & 0xff) as u8 },
            3 => Self::Killed,
            _ => Self::Unknown,
        };
        (code, reason)
    }

    /// Deterministic label for markers/log fields.
    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Error => "error",
            Self::Fault { .. } => "fault",
            Self::Killed => "killed",
            Self::Unknown => "unknown",
        }
    }
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
            // RFC-0085: 12/28 stop collapsing into SpawnFailed — the spawn
            // wrappers translate locally, everyone else gets the identity.
            12 => Some(Self::OutOfMemory),   // ENOMEM
            28 => Some(Self::NoSpace),       // ENOSPC
            110 => Some(Self::TimedOut),     // ETIMEDOUT
            11 => Some(Self::WouldBlock),    // EAGAIN
            17 => Some(Self::AlreadyExists), // EEXIST (ADR-0054: map overlap)
            14 => Some(Self::BadAddress),    // EFAULT (ADR-0054: VA out of range)
            2 => Some(Self::NotFound),       // ENOENT (RFC-0085: unmap miss)
            16 => Some(Self::Busy),          // EBUSY (RFC-0085: destroy w/ live maps)
            // Fail closed: an unknown NEGATIVE code is an error, never a
            // success value (Phase C: -ETIMEDOUT used to decode as Ok and
            // turned every fence/waitset timeout into a silent pseudo-Ok).
            _ => Some(Self::Unknown),
        }
    }
}

#[cfg(test)]
mod exit_reason_tests {
    use super::ExitReason;

    /// Mirrors the kernel's `ExitReason::wire_bits` packing (a1 high half).
    fn pack(tag: u32, cause: u32, code: i32) -> usize {
        (((tag | (cause << 8)) as usize) << 32) | (code as u32 as usize)
    }

    #[test]
    fn decode_clean_and_error_split_voluntary_by_code() {
        assert_eq!(ExitReason::decode_wait_status(pack(0, 0, 0)), (0, ExitReason::Clean));
        assert_eq!(ExitReason::decode_wait_status(pack(0, 0, 42)), (42, ExitReason::Error));
        assert_eq!(ExitReason::decode_wait_status(pack(0, 0, -22)), (-22, ExitReason::Error));
    }

    #[test]
    fn decode_fault_carries_cause() {
        assert_eq!(
            ExitReason::decode_wait_status(pack(2, 13, -22)),
            (-22, ExitReason::Fault { cause: 13 })
        );
        assert_eq!(
            ExitReason::decode_wait_status(pack(2, 0xF1, -22)),
            (-22, ExitReason::Fault { cause: 0xF1 })
        );
    }

    #[test]
    fn decode_killed_and_unknown_never_collapse() {
        assert_eq!(ExitReason::decode_wait_status(pack(3, 0, -22)), (-22, ExitReason::Killed));
        // Unknown tag stays Unknown (ADR-0054: no wildcard folding).
        assert_eq!(ExitReason::decode_wait_status(pack(9, 0, 0)), (0, ExitReason::Unknown));
        // Reserved tag 1 (consumer-side error) must not arrive from the
        // kernel; if it ever does, it is Unknown — not silently Error.
        assert_eq!(ExitReason::decode_wait_status(pack(1, 0, 5)), (5, ExitReason::Unknown));
    }
}
