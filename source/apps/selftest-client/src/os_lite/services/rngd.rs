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
use core::time::Duration;

use nexus_ipc::{Client, KernelClient, Wait};

/// rngd wire (pinned by the rngd service): request `[R, G, VERSION, OP, nonce:u32le, n:u16le]`,
/// response `[R, G, VERSION, OP|0x80, status, nonce:u32le, entropy…]`.
const MAGIC: [u8; 2] = [b'R', b'G'];
const VERSION: u8 = 1;
const OP_GET_ENTROPY: u8 = 1;
const RSP_HEADER_LEN: usize = 9;

/// The selftest → rngd route slots init provisions. One copy for the whole client (there
/// were three); the route migrates onto `nexus-service-topology` with the selftest client
/// itself (TASK-0324 P4f).
const RNGD_SEND_SLOT: u32 = 0x1e;
const RNGD_RECV_SLOT: u32 = 0x1f;

/// Liveness bound, NOT a scheduling guess: the wait is event-driven, so this only decides
/// how long an rngd that has not answered is still presumed alive. rngd asks policyd before
/// it replies; a supervised peer silent for 2 s is a real outage (ADR-0057). Same bound as
/// the policy exchange in `nexus_ipc::policyd` (TASK-0324 P3/P4a).
const LIVENESS_BOUND: Duration = Duration::from_secs(2);

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
    KernelClient::new_with_slots(RNGD_SEND_SLOT, RNGD_RECV_SLOT).map_err(|_| RngdError::NoSlots)
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
    client.send(&req, Wait::Timeout(LIVENESS_BOUND)).map_err(|_| RngdError::Send)?;

    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(LIVENESS_BOUND.as_nanos() as u64);
    loop {
        let now = nexus_abi::nsec().unwrap_or(u64::MAX);
        if now >= deadline {
            return Err(RngdError::NoReply);
        }
        // Blocks in the kernel until a frame arrives or the absolute deadline passes.
        let rsp = client
            .recv(Wait::Timeout(Duration::from_nanos(deadline - now)))
            .map_err(|_| RngdError::NoReply)?;
        if rsp.len() < RSP_HEADER_LEN || rsp[0..2] != MAGIC || rsp[2] != VERSION {
            continue;
        }
        if u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]) != nonce {
            continue;
        }
        if rsp[3] != (OP_GET_ENTROPY | 0x80) {
            return Err(RngdError::WrongOp);
        }
        return Ok(EntropyReply { status: rsp[4], entropy: rsp[RSP_HEADER_LEN..].to_vec() });
    }
}
