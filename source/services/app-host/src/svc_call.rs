// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The fixed-slot request/reply transport every `svc.*` call rides: ONE exchange
//! (`nexus_ipc::exchange::call_into`, TASK-0054C P2-a) over the child's provisioned
//! `@reply` inbox — a fresh SEND clone moved with the request, a wait on the RECV half with
//! last-sender EOF opted in. Exactly two things end the wait: the reply, or the service's
//! death. No deadline, no stale-reply drain: the inbox is private and minted per launch,
//! this host is single-threaded, and only CAP_MOVE replies can land here, so the only way a
//! stale frame ever appeared was a client timeout abandoning its reply — the cause is gone
//! with the clock (RFC-0093 §7). A service that is alive and silent is a supervision truth
//! (ADR-0057), never a client timer.
//!
//! Split out of `effect_host.rs` (structure-gate) — it is TRANSPORT, not the service surface.

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use nexus_sdk_routes::{CHILD_REPLY_RECV_SLOT, CHILD_REPLY_SEND_SLOT};
use nexus_service_topology::SlotPair;

/// Fixed-slot request/reply over the child's provisioned `@reply` inbox (child slots 10/9).
/// Returns the reply frame length, or `None` on any send/recv failure (the caller renders
/// the `Err` arm).
pub(crate) fn call_reply(service_send_slot: u32, req: &[u8], resp: &mut [u8]) -> Option<usize> {
    let reply = SlotPair::new(CHILD_REPLY_SEND_SLOT, CHILD_REPLY_RECV_SLOT);
    nexus_ipc::exchange::call_into(service_send_slot, reply, req, resp)
        .ok()
        .map(|n| n.min(resp.len()))
}
