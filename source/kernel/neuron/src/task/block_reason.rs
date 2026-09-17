// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: why a task is not runnable. Split out of `task/mod.rs` when
//! TASK-0054C P4a added `IpcCall`: the module sat at its structure-gate
//! ceiling, and a blocking reason is its own concept — every scheduler sweep,
//! every deadline scan and every wake path reads exactly this enum.
//!
//! Three places match it for a DEADLINE (`task/mod.rs`, `syscall/api/mod.rs`,
//! `core/trap/runtime.rs`); each has a wildcard arm, which is why a variant
//! that carries no deadline — like `IpcCall`, clockless by ABI (RFC-0093 §7) —
//! slots in without touching them.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the scheduler/IPC ladders in QEMU

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
}
