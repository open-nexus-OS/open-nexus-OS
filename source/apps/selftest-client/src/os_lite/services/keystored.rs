// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: keystored IPC client + selftest probes — device-key bring-up,
//!   pubkey export, sign-denied path, cap_move probe, and the silent
//!   `resolve_keystored_client` helper consumed by both bringup and policy
//!   phases.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup + policy phases.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use nexus_abi::MsgHeader;
use nexus_ipc::exchange;
use nexus_ipc::{KernelClient, Wait as IpcWait};

use super::super::ipc::routing::route_with_retry;
use crate::markers::emit_line;

pub(crate) fn keystored_ping(client: &KernelClient) -> core::result::Result<(), ()> {
    // Keystore IPC v1:
    // Request: [K, S, ver, op, key_len:u8, val_len:u16le, key..., val...]
    // Response: [K, S, ver, op|0x80, status:u8, val_len:u16le, val...]
    const K: u8 = b'K';
    const S: u8 = b'S';
    const VER: u8 = 1;
    const OP_PUT: u8 = 1;
    const OP_GET: u8 = 2;
    const OP_DEL: u8 = 3;
    const OK: u8 = 0;
    const NOT_FOUND: u8 = 1;
    const MALFORMED: u8 = 2;

    fn send_req(
        client: &KernelClient,
        op: u8,
        key: &[u8],
        val: &[u8],
    ) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
        let (send_slot, recv_slot) = client.slots();
        let mut req = alloc::vec::Vec::with_capacity(7 + key.len() + val.len());
        req.push(b'K');
        req.push(b'S');
        req.push(1u8);
        req.push(op);
        req.push(key.len() as u8);
        req.extend_from_slice(&(val.len() as u16).to_le_bytes());
        req.extend_from_slice(key);
        req.extend_from_slice(val);

        let hdr = MsgHeader::new(0, 0, 0, 0, req.len() as u32);
        // WAIT for the answer with no clock (TASK-0324 P7-b): the reply or keystored's death.
        let _ = hdr;
        exchange::send_request(send_slot, &req).map_err(|_| ())?;
        let mut buf = [0u8; 256];
        let n = exchange::recv_response(recv_slot, &mut buf).map_err(|_| ())?;
        let n = core::cmp::min(n, buf.len());
        let mut out = alloc::vec::Vec::with_capacity(n);
        out.extend_from_slice(&buf[..n]);
        Ok(out)
    }

    fn parse_rsp(rsp: &[u8], expect_op: u8) -> core::result::Result<(u8, &[u8]), ()> {
        if rsp.len() < 7 || rsp[0] != K || rsp[1] != S || rsp[2] != VER {
            return Err(());
        }
        if rsp[3] != (expect_op | 0x80) {
            return Err(());
        }
        let status = rsp[4];
        let len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
        if rsp.len() != 7 + len {
            return Err(());
        }
        Ok((status, &rsp[7..]))
    }

    let key = b"k1";
    let val = b"v1";

    // PUT
    let rsp = send_req(client, OP_PUT, key, val)?;
    let (status, _payload) = parse_rsp(&rsp, OP_PUT)?;
    if status != OK {
        return Err(());
    }
    // GET
    let rsp = send_req(client, OP_GET, key, &[])?;
    let (status, payload) = parse_rsp(&rsp, OP_GET)?;
    if status != OK || payload != val {
        return Err(());
    }
    // DEL
    let rsp = send_req(client, OP_DEL, key, &[])?;
    let (status, _payload) = parse_rsp(&rsp, OP_DEL)?;
    if status != OK {
        return Err(());
    }
    // GET miss
    let rsp = send_req(client, OP_GET, key, &[])?;
    let (status, payload) = parse_rsp(&rsp, OP_GET)?;
    if status != NOT_FOUND || !payload.is_empty() {
        return Err(());
    }

    // Malformed frame should return MALFORMED (wrong magic).
    let (send_slot, recv_slot) = client.slots();
    exchange::send_request(send_slot, b"bad").map_err(|_| ())?;
    let mut buf = [0u8; 64];
    let n = exchange::recv_response(recv_slot, &mut buf).map_err(|_| ())?;
    let rsp = &buf[..core::cmp::min(n, buf.len())];
    let (status, _payload) = parse_rsp(&rsp, OP_GET)?;
    if status != MALFORMED {
        return Err(());
    }

    Ok(())
}

/// The keystored client: the declared route (TASK-0324 P4f-5), asked ONCE and verified with
/// one ping. The 128-round loop that tried three slot sources in turn (responder, `new_for`,
/// the literal pair) is gone with P7 — every source resolved to the same declaration.
pub(crate) fn resolve_keystored_client() -> core::result::Result<KernelClient, ()> {
    let client = route_with_retry("keystored")?;
    keystored_ping(&client)?;
    Ok(client)
}

pub(crate) fn keystored_cap_move_probe(
    reply_send_slot: u32,
    reply_recv_slot: u32,
) -> core::result::Result<(), ()> {
    // Use existing keystored v1 GET(miss) but receive reply via CAP_MOVE reply cap.
    emit_line(crate::markers::M_SELFTEST_KEYSTORED_CAPMOVE_BEGIN);
    let keystored = route_with_retry("keystored")?;
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(slot) => slot,
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_KEYSTORED_CAPMOVE_CLONE_FAIL);
            return Err(());
        }
    };

    // Keystore GET miss for key "capmove.miss".
    let key = b"capmove.miss";
    let mut req = alloc::vec::Vec::with_capacity(7 + key.len());
    req.push(b'K');
    req.push(b'S');
    req.push(1); // ver
    req.push(2); // OP_GET
    req.push(key.len() as u8);
    req.extend_from_slice(&0u16.to_le_bytes()); // val_len=0
    req.extend_from_slice(key);

    if keystored.send_with_cap_move_wait(&req, reply_send_clone, IpcWait::Blocking).is_err() {
        emit_line(crate::markers::M_SELFTEST_KEYSTORED_CAPMOVE_SEND_FAIL);
        return Err(());
    }

    // WAIT on the reply inbox for our GET-miss answer — no clock: the answer, or keystored's
    // death (EOF: it holds the moved reply cap, we are the inbox owner).
    let mut buf = [0u8; 128];
    while let Ok(n) = exchange::recv_reply(reply_recv_slot, &mut buf) {
        let rsp = &buf[..core::cmp::min(n, buf.len())];
        // Expect: [K,S,ver,OP_GET|0x80,status,val_len]
        if rsp.len() >= 7
            && rsp[0] == b'K'
            && rsp[1] == b'S'
            && rsp[2] == 1
            && rsp[3] == (2 | 0x80)
            && rsp[4] == 1
        {
            return Ok(());
        }
    }
    emit_line(crate::markers::M_SELFTEST_KEYSTORED_CAPMOVE_NO_REPLY);
    Err(())
}
