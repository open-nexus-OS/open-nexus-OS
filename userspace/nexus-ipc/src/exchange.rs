// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE request/reply exchange (TASK-0324 P7-b, RFC-0093 §1) — and it has NO
//! timeout. A request carries a moved SEND clone of the caller's reply channel; the caller
//! then blocks on that channel's RECV with last-sender EOF opted in (RFC-0079). Exactly two
//! things can end the wait: the reply frame, or the peer's DEATH — the kernel reaps a dead
//! task's caps through the same last-peer scan a `cap_close` runs, and the channel's owner
//! (this caller, which keeps the SEND base it clones from) is never counted as its own peer.
//! A live peer that never answers is a supervision truth (ADR-0057), not a client timer:
//! nothing here consults a clock. Before P7-b every caller carried a "liveness bound"
//! because a dead peer could not wake it; that bound is gone with the cause.
//!
//! The reply channel is the caller's declared, init-minted inbox (`slots::<svc>::REPLY`,
//! `nexus-service-topology`) or a channel minted on demand from init's factory
//! (`@mint-pair`, [`mint_reply_channel`]). Exchanges on one channel are sequential — the
//! frame that arrives is the answer to the request just sent — so no nonce filtering and no
//! reply buffer exist here.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU — `SELFTEST: exec child eof on exit ok` proves the death wake; every
//!   exchange in the ladder runs through here.

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use nexus_service_topology::SlotPair;

use crate::{IpcError, Result};

/// Sends `frame` to `send_slot` with a fresh SEND clone of `reply` moved along, then waits for
/// the reply on `reply.recv` into the caller's buffer (alloc-free; the length is the reply's).
/// Blocks without a deadline: the reply or the peer's death ends it. A reply longer than `out`
/// is truncated (`IPC_SYS_TRUNCATE`) — size `out` for the protocol (`IPC_PAYLOAD_MAX` is the
/// transport cap; a protocol's own bound is smaller and known to its caller).
pub fn call_into(send_slot: u32, reply: SlotPair, frame: &[u8], out: &mut [u8]) -> Result<usize> {
    let clone = nexus_abi::cap_clone(reply.send).map_err(|_| IpcError::Unsupported)?;
    let hdr =
        nexus_abi::MsgHeader::new(clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, frame.len() as u32);
    // A full server queue is backpressure to WAIT out; a dead server wakes this send.
    if let Err(e) = nexus_abi::ipc_send_v1(send_slot, &hdr, frame, 0, 0) {
        let _ = nexus_abi::cap_close(clone);
        return Err(map(e));
    }
    recv_reply(reply.recv, out)
}

/// Waits on `recv_slot` for one frame with EOF opted in: the frame, or `Disconnected` once the
/// peer that held the moved SEND cap is gone (it replied-and-closed without a frame queued,
/// or it died).
pub fn recv_reply(recv_slot: u32, out: &mut [u8]) -> Result<usize> {
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    nexus_abi::ipc_recv_v1(
        recv_slot,
        &mut hdr,
        out,
        nexus_abi::IPC_SYS_TRUNCATE | nexus_abi::IPC_SYS_EOF,
        0,
    )
    .map(|n| n as usize)
    .map_err(map)
}

/// Mints a private reply channel from init's endpoint factory (`@mint-pair`, allow-listed
/// per identity in init's responder). For a caller whose exchanges may overlap (a second
/// request before the first answered) — one channel per in-flight exchange.
pub fn mint_reply_channel(mismatch_budget: crate::budget::NonceMismatchBudget) -> Option<SlotPair> {
    match crate::budget::route_with_nonce(b"@mint-pair", mismatch_budget) {
        crate::budget::RouteRetryOutcome::Success { send_slot, recv_slot } => {
            Some(SlotPair::new(send_slot, recv_slot))
        }
        _ => None,
    }
}

/// [`call_into`] for a reply channel the caller ALSO uses for fire-and-forget traffic (audit
/// appends whose acks are never read): waits until `accept` recognises the answer, dropping
/// every other queued frame. Still no clock — the answer or the peer's death ends it.
pub fn call_matching<T>(
    send_slot: u32,
    reply: SlotPair,
    frame: &[u8],
    out: &mut [u8],
    mut accept: impl FnMut(&[u8]) -> Option<T>,
) -> Result<T> {
    let mut n = call_into(send_slot, reply, frame, out)?;
    loop {
        let len = core::cmp::min(n, out.len());
        if let Some(v) = accept(&out[..len]) {
            return Ok(v);
        }
        n = recv_reply(reply.recv, out)?;
    }
}

/// Blocking send of a plain request (no reply cap moved — the target answers on its own
/// response endpoint, a `SharedResponse` route). No deadline: queue space or the peer's death.
pub fn send_request(send_slot: u32, frame: &[u8]) -> Result<()> {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    nexus_abi::ipc_send_v1(send_slot, &hdr, frame, 0, 0).map(|_| ()).map_err(map)
}

/// Blocking receive of one frame from a `SharedResponse` route's RECV half (no EOF: that
/// endpoint is the server's, not ours — its death closes it and wakes us with an error).
pub fn recv_response(recv_slot: u32, out: &mut [u8]) -> Result<usize> {
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    nexus_abi::ipc_recv_v1(recv_slot, &mut hdr, out, nexus_abi::IPC_SYS_TRUNCATE, 0)
        .map(|n| n as usize)
        .map_err(map)
}

fn map(err: nexus_abi::IpcError) -> IpcError {
    match err {
        nexus_abi::IpcError::PeerClosed => IpcError::Disconnected,
        nexus_abi::IpcError::NoSpace => IpcError::NoSpace,
        e => IpcError::Kernel(e),
    }
}
