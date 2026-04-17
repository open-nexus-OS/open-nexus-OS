// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared `ReplyInboxV1` `nexus_ipc::Client` adapter for selftest
//! probes that consume the deterministic shared reply inbox (RFC-0019).
//! Pre-P2-16 this struct existed verbatim in three probe sites
//! (cap_move_reply, sender_pid, ipc_soak); this single source of truth
//! removes the duplication while preserving byte-identical recv semantics.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`
//!   (cap_move_reply / sender_pid / sender_service_id / ipc_soak markers).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0019-*.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::{ipc_recv_v1, MsgHeader};
use nexus_ipc::{Client, IpcError, Wait as IpcWait};

/// Minimal `nexus_ipc::Client` view over the deterministic shared reply
/// inbox. `send` is intentionally unsupported: the inbox is a receive-only
/// adapter consumed by `recv_match_until`, which paginates / nonce-correlates
/// frames against a `ReplyBuffer`.
///
/// IMPORTANT: Recv is non-blocking + truncating; callers must already be
/// driving a deadline-bounded loop (e.g. `recv_match_until`) so this returns
/// `WouldBlock` on `QueueEmpty` and lets the upper layer yield/budget.
pub(crate) struct ReplyInboxV1 {
    pub(crate) recv_slot: u32,
}

impl Client for ReplyInboxV1 {
    fn send(&self, _frame: &[u8], _wait: IpcWait) -> nexus_ipc::Result<()> {
        Err(IpcError::Unsupported)
    }
    fn recv(&self, _wait: IpcWait) -> nexus_ipc::Result<Vec<u8>> {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64];
        match ipc_recv_v1(
            self.recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
            Err(nexus_abi::IpcError::QueueEmpty) => Err(IpcError::WouldBlock),
            Err(other) => Err(IpcError::Kernel(other)),
        }
    }
}
