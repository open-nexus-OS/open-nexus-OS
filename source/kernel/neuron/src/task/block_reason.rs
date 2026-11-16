// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: why a task is not runnable. Split out of `task/mod.rs` when
//! TASK-0054C P4a added `IpcCall`: the module sat at its structure-gate
//! ceiling, and a blocking reason is its own concept — every scheduler sweep,
//! every deadline scan and every wake path reads exactly this enum.
//!
//! A DEADLINE is read in one place, [`BlockReason::deadline_ns`] (an exhaustive match: a new
//! kind is decided at compile time), and decided by `deadline_rule` (host-tested) for the one
//! deadline walk (`syscall::api::expire_deadlines`, TASK-0363 P6/P7). A variant that carries no
//! deadline — like `IpcCall`, clockless by ABI (RFC-0093 §7) — is untimed.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the deadline decision in `deadline_rule` (host); the scheduler/IPC ladders in QEMU

use super::Pid;
use crate::ipc;

/// Scheduler-visible blocking reason for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    IpcRecv {
        endpoint: ipc::EndpointId,
        deadline_ns: u64,
    },
    IpcSend {
        endpoint: ipc::EndpointId,
        deadline_ns: u64,
    },
    WaitChild {
        target: Option<Pid>,
    },
    /// Blocked in `waitset_wait` on a set of endpoints (RFC-0033). The task is
    /// registered as a recv-waiter on *every* member; the first member to deliver
    /// (or the deadline) wakes it. `ws_id` is the kernel-local waitset id (raw `u32`).
    Waitset {
        ws_id: u32,
        deadline_ns: u64,
    },
    /// Blocked in `ipc_call`, request already SENT (TASK-0054C P4a): a recv-waiter
    /// on its own reply endpoint, carrying a [`completion::CallState`]. No deadline.
    IpcCall {
        reply_ep: ipc::EndpointId,
    },
    /// Blocked in `fence_wait` until the fence's monotonic value reaches `target`
    /// (RFC-0033). Registered as a fence waiter; `fence_signal` wakes it once
    /// `value >= target`, or the deadline does. `fence_id` is the raw kernel id.
    Fence {
        fence_id: u32,
        target: u64,
        deadline_ns: u64,
    },
    /// Parked on a swapped page (TASK-0333 6f-2, ADR-0088): a user fault or a syscall touched
    /// page `token` (object id << 32 | page index) while its sealed record was on the pager's
    /// disk. The pager's page-in (syscall 73) wakes every thread parked on the token; the
    /// faulting instruction or the syscall re-executes. No deadline: the kernel never times a
    /// page-in out (`mm::swap`).
    PageIn {
        token: u64,
    },
}

impl BlockReason {
    /// The deadline this wait carries (`None`: none by kind; `Some(0)`: none asked for).
    pub(crate) const fn deadline_ns(self) -> Option<u64> {
        match self {
            Self::IpcRecv { deadline_ns, .. }
            | Self::IpcSend { deadline_ns, .. }
            | Self::Waitset { deadline_ns, .. }
            | Self::Fence { deadline_ns, .. } => Some(deadline_ns),
            Self::WaitChild { .. } | Self::IpcCall { .. } | Self::PageIn { .. } => None,
        }
    }
}
