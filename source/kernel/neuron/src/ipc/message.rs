// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel IPC message — a header, a tiered [`Payload`], an
//! optionally moved capability and the kernel-stamped sender identity. Split
//! out of `ipc/mod.rs` when TASK-0054C P3b put the payload on tiers: the
//! module was at its structure-gate ceiling, and a message is its own thing.
//! Target-gated with its parent, because `moved_cap` is a `crate::cap`
//! capability — the pure, host-testable part is `crate::ipc_payload`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `crate::ipc_payload` tests (host) + the QEMU IPC ladder
//! RFC: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md

use super::header::MessageHeader;
use super::Payload;

/// Message combining header and payload.
///
/// The payload is a [`Payload`], not a `Vec<u8>`: a control message (up to
/// [`IPC_SHORT_MAX`]) rides inside the message and costs the kernel no heap
/// (TASK-0054C P3b, RFC-0096).
#[derive(Clone, Debug)]
pub struct Message {
    pub header: MessageHeader,
    pub payload: Payload,
    /// Optional capability moved alongside this message (Phase-2 hardening / scalability).
    pub moved_cap: Option<crate::cap::Capability>,
    /// Expected endpoint id for CAP_MOVE (when moving an Endpoint cap).
    ///
    /// SECURITY/ROBUSTNESS: This is a kernel-internal consistency field used to detect and
    /// correct mismatches between the moved capability's endpoint id at send-time vs
    /// receive-time. It MUST NOT be exposed to userspace directly.
    pub capmove_expected_ep: u32,
    /// Kernel-derived stable identity of the sender service (BootstrapInfo v2).
    ///
    /// This is populated by the syscall layer at send-time, and remains stable even if the sender
    /// exits before the receiver dequeues the message.
    pub sender_service_id: u64,
}

impl Message {
    /// Creates a message and truncates the payload length to match `header.len`.
    pub fn new(
        header: MessageHeader,
        payload: Payload,
        moved_cap: Option<crate::cap::Capability>,
    ) -> Self {
        let mut payload = payload;
        payload.truncate(header.len as usize);
        Self { header, payload, moved_cap, capmove_expected_ep: 0, sender_service_id: 0 }
    }
}
