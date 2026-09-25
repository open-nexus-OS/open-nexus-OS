// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: The ONE socd client (RFC-0106, TASK-0246 P4c): a driver asks the SoC glue owner to
//! bring up the node its device is (`BRING_UP`: power domains on, resets released, clocks on,
//! pads set — `NOT_NEEDED` when the node names none, as on QEMU virt) and for the rate of one
//! of its clocks (`CLOCK_RATE`). One exchange each over the caller's declared route
//! (`nexus_wire::soc`, the reply matched by nonce, `exchange` — no clock); the harness and the
//! block owner call the same two functions.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the codec and socd's verdicts are host-tested (`nexus-wire`, socd
//!   `tests/contract.rs`); the exchange by QEMU (`SELFTEST: soc glue not needed ok`, and
//!   `socd: bring-up … not needed` for the block owner's node in every boot)

use nexus_service_topology::SlotPair;
use nexus_wire::soc;

use crate::exchange;

/// Why an exchange with socd produced no verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocError {
    /// The request does not encode (a path or clock name longer than the wire carries).
    Encode,
    /// The request could not be sent.
    Send,
    /// socd died before it answered.
    NoReply,
    /// socd answered this request with a frame that is not the op's reply.
    Reply,
}

/// `BRING_UP` for the node at `path`, over the route whose request SEND is `send_slot` and
/// whose replies arrive on `reply`: socd's verdict (`STATUS_OK`, `STATUS_NOT_NEEDED`, or why
/// not).
pub fn bring_up(
    send_slot: u32,
    reply: SlotPair,
    path: &str,
    nonce: u32,
) -> Result<soc::BringUpReply, SocError> {
    let mut req = [0u8; 128];
    let n = soc::encode_bring_up_req(&mut req, nonce, path).ok_or(SocError::Encode)?;
    exchange_one(send_slot, reply, &req[..n], nonce, soc::decode_bring_up_rsp)
}

/// `CLOCK_RATE` of the clock `name` of the node at `path`: `(status, hz)` — `STATUS_OK` with
/// the rate, `STATUS_NOT_NEEDED` when the node names no such clock.
pub fn clock_rate(
    send_slot: u32,
    reply: SlotPair,
    path: &str,
    name: &str,
    nonce: u32,
) -> Result<(u8, u64), SocError> {
    let mut req = [0u8; 160];
    let n = soc::encode_clock_rate_req(&mut req, nonce, path, name).ok_or(SocError::Encode)?;
    exchange_one(send_slot, reply, &req[..n], nonce, |rsp| {
        soc::decode_clock_rate_rsp(rsp).map(|(status, _, hz)| (status, hz))
    })
}

/// One request, its reply by nonce (a stranger's frame is dropped and the wait goes on), then
/// the op's decoder.
fn exchange_one<T>(
    send_slot: u32,
    reply: SlotPair,
    req: &[u8],
    nonce: u32,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Result<T, SocError> {
    let mut buf = [0u8; 32];
    let decoded = exchange::call_matching(send_slot, reply, req, &mut buf, |rsp| {
        let ours = rsp.len() >= 9
            && rsp[0] == soc::MAGIC0
            && rsp[1] == soc::MAGIC1
            && rsp[2] == soc::VERSION
            && u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]) == nonce;
        ours.then(|| decode(rsp))
    })
    .map_err(|e| match e {
        crate::IpcError::Disconnected => SocError::NoReply,
        _ => SocError::Send,
    })?;
    decoded.ok_or(SocError::Reply)
}
