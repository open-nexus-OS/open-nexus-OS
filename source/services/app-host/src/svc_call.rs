// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The fixed-slot request/reply transport every `svc.*` call rides.
//!
//! Split out of `effect_host.rs` (structure-gate) — it is TRANSPORT, not the
//! service surface, and it is what carries the DSL's `timeoutMs:` budget.

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use super::effect_host::SVC_DEADLINE_NS;
use nexus_sdk_routes::{CHILD_REPLY_RECV_SLOT, CHILD_REPLY_SEND_SLOT};

/// Fixed-slot request/reply over the child's provisioned `@reply` inbox: clone
/// the reply SEND (child slot 10), MOVE it into the request so the service
/// answers our inbox, send on `service_send_slot` (bounded), then receive on
/// the reply RECV (child slot 9). Returns the reply frame length, or `None` on
/// any send/recv failure or timeout (the caller renders the `Err` arm).
pub(crate) fn call_reply(service_send_slot: u32, req: &[u8], resp: &mut [u8]) -> Option<usize> {
    call_reply_within(service_send_slot, req, resp, SVC_DEADLINE_NS)
}

/// [`call_reply`] with an explicit budget — the DSL's `timeoutMs:`.
///
/// `EffectHost::call` used to take `_timeout_ms` and throw it away, so every
/// `svc.*` call in every app ran on one hardcoded constant no matter what the
/// page asked for. The knob type-checked, documented and did nothing.
pub(crate) fn call_reply_within(
    service_send_slot: u32,
    req: &[u8],
    resp: &mut [u8],
    budget_ns: u64,
) -> Option<usize> {
    let reply_send = nexus_abi::cap_clone(CHILD_REPLY_SEND_SLOT).ok()?;
    let hdr =
        nexus_abi::MsgHeader::new(reply_send, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, req.len() as u32);
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(budget_ns);

    // KERNEL-PARKED send: a full queue registers us as a send-waiter and the
    // receive path wakes us the moment capacity appears (RFC-0083 — the old
    // NONBLOCK + yield_() spin here burned up to 2 s of scheduler quanta).
    if nexus_abi::ipc_send_v1(service_send_slot, &hdr, req, 0, deadline).is_err() {
        // Reclaim the clone (a successful CAP_MOVE would have consumed it).
        let _ = nexus_abi::cap_close(reply_send);
        return None;
    }

    // KERNEL-PARKED receive on the same absolute deadline.
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    match nexus_abi::ipc_recv_v1(
        CHILD_REPLY_RECV_SLOT,
        &mut rh,
        resp,
        nexus_abi::IPC_SYS_TRUNCATE,
        deadline,
    ) {
        Ok(n) => Some((n as usize).min(resp.len())),
        Err(_) => None,
    }
}
