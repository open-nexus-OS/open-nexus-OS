// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The selftest client's ONE rngd exchange (`GET_ENTROPY`). The entropy probes
//! and the statefs encryption salt used to carry three hand-copied versions of this wire,
//! each POLLING for the reply (`recv(NonBlocking)` + `yield_()` against a 500 ms wall clock).
//! rngd consults policyd before it answers, so under host load a live rngd missed the
//! budget and looked exactly like one that never answered: `SELFTEST: statefs enc roundtrip
//! FAIL` in a cold `test-all` (2026-09-11), with `rngd: policy check` printed right AFTER
//! the selftest had given up. That is the defect class TASK-0324 P3 removed from
//! `nexus_ipc::policyd`; this is the same fix — WAIT for the answer (deadline-bounded
//! blocking recv, woken by the arriving reply), never poll for it.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU `just test-os` — `SELFTEST: rng entropy ok`, `SELFTEST: rng entropy
//!   oversized ok`, `SELFTEST: statefs enc roundtrip ok`.
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;
use nexus_ipc::KernelClient;

/// rngd wire (pinned by the rngd service): request `[R, G, VERSION, OP, nonce:u32le, n:u16le]`,
/// response `[R, G, VERSION, OP|0x80, status, nonce:u32le, entropy…]`.
const MAGIC: [u8; 2] = [b'R', b'G'];
const VERSION: u8 = 1;
const OP_GET_ENTROPY: u8 = 1;
const RSP_HEADER_LEN: usize = 9;

/// Why an exchange produced no usable reply. Each maps to its own marker at the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RngdError {
    /// The route slots are not (or not yet) provisioned.
    NoSlots,
    /// The request could not be queued within the liveness bound.
    Send,
    /// No nonce-matched reply arrived within the liveness bound.
    NoReply,
    /// A nonce-matched reply carried the wrong opcode.
    WrongOp,
}

/// A nonce-matched rngd reply. `status` is rngd's verdict (0 = ok, 1 = rejected); the
/// entropy bytes are NEVER logged (security invariant).
pub(crate) struct EntropyReply {
    pub(crate) status: u8,
    pub(crate) entropy: Vec<u8>,
}

/// The client for the rngd route. Split from [`get_entropy`] so a caller can emit its
/// "sending" breadcrumb between the two, exactly where the marker ladder expects it.
pub(crate) fn client() -> Result<KernelClient, RngdError> {
    // The selftest → rngd route, declared by the topology (TASK-0324 P4f-5).
    let route = nexus_service_topology::slots::selftest_client::RNGD;
    KernelClient::new_with_slots(route.send, route.recv).map_err(|_| RngdError::NoSlots)
}

/// One `GET_ENTROPY` exchange for `n` bytes, correlated by `nonce`.
///
/// The reply inbox is shared with every other rngd request this client has in flight, so a
/// frame that is not ours (foreign magic, or another exchange's nonce) is dropped and the
/// wait continues until the liveness bound.
pub(crate) fn get_entropy(
    client: &KernelClient,
    n: u16,
    nonce: u32,
) -> Result<EntropyReply, RngdError> {
    let mut req = Vec::with_capacity(10);
    req.extend_from_slice(&[MAGIC[0], MAGIC[1], VERSION, OP_GET_ENTROPY]);
    req.extend_from_slice(&nonce.to_le_bytes());
    req.extend_from_slice(&n.to_le_bytes());
    // ONE exchange, no clock (TASK-0324 P7-b): the reply or rngd's death ends the wait.
    let (send_slot, _) = client.slots();
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    // The inbox is shared with other exchanges: the answer is the frame carrying OUR nonce.
    // Sized for the protocol: the reply header plus the largest entropy request this
    // client makes (the transport cap is `IPC_PAYLOAD_MAX`, not a reply bound).
    let mut buf = [0u8; RSP_HEADER_LEN + 512];
    let rsp = nexus_ipc::exchange::call_matching(send_slot, reply, &req, &mut buf, |rsp| {
        let ours = rsp.len() >= RSP_HEADER_LEN
            && rsp[0..2] == MAGIC
            && rsp[2] == VERSION
            && u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]) == nonce;
        ours.then(|| rsp.to_vec())
    })
    .map_err(|e| match e {
        nexus_ipc::IpcError::Disconnected => RngdError::NoReply,
        _ => RngdError::Send,
    })?;
    if rsp[3] != (OP_GET_ENTROPY | 0x80) {
        return Err(RngdError::WrongOp);
    }
    Ok(EntropyReply { status: rsp[4], entropy: rsp[RSP_HEADER_LEN..].to_vec() })
}
