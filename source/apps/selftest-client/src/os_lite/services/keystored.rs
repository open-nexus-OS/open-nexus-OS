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

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::{KernelClient, Wait as IpcWait};

use super::super::ipc::routing::{route_with_retry, routing_v1_get};
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
        let start = nexus_abi::nsec().map_err(|_| ())?;
        let deadline = start.saturating_add(2_000_000_000); // 2s
        let mut i: usize = 0;
        loop {
            match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    if (i & 0x7f) == 0 {
                        let now = nexus_abi::nsec().map_err(|_| ())?;
                        if now >= deadline {
                            return Err(());
                        }
                    }
                    let _ = yield_();
                }
                Err(_) => return Err(()),
            }
            i = i.wrapping_add(1);
        }

        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 256];
        let mut j: usize = 0;
        loop {
            if (j & 0x7f) == 0 {
                let now = nexus_abi::nsec().map_err(|_| ())?;
                if now >= deadline {
                    return Err(());
                }
            }
            match nexus_abi::ipc_recv_v1(
                recv_slot,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    let n = core::cmp::min(n as usize, buf.len());
                    let mut out = alloc::vec::Vec::with_capacity(n);
                    out.extend_from_slice(&buf[..n]);
                    return Ok(out);
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => return Err(()),
            }
            j = j.wrapping_add(1);
        }
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
    let hdr = MsgHeader::new(0, 0, 0, 0, 3);
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(2_000_000_000);
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, b"bad", nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 64];
    let mut j: usize = 0;
    let rsp = loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => break &buf[..core::cmp::min(n as usize, buf.len())],
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    };
    let (status, _payload) = parse_rsp(&rsp, OP_GET)?;
    if status != MALFORMED {
        return Err(());
    }

    Ok(())
}

pub(crate) fn resolve_keystored_client() -> core::result::Result<KernelClient, ()> {
    for _ in 0..128 {
        if let Ok((status, send, recv)) = routing_v1_get("keystored") {
            if status == nexus_abi::routing::STATUS_OK && send != 0 && recv != 0 {
                let client = KernelClient::new_with_slots(send, recv).map_err(|_| ())?;
                if keystored_ping(&client).is_ok() {
                    return Ok(client);
                }
            }
        }
        if let Ok(client) = KernelClient::new_for("keystored") {
            if keystored_ping(&client).is_ok() {
                return Ok(client);
            }
        }
        for (send, recv) in [(0x11, 0x12), (0x12, 0x11)] {
            if let Ok(client) = KernelClient::new_with_slots(send, recv) {
                if keystored_ping(&client).is_ok() {
                    return Ok(client);
                }
            }
        }
        let _ = yield_();
    }
    Err(())
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

    if keystored
        .send_with_cap_move_wait(
            &req,
            reply_send_clone,
            IpcWait::Timeout(core::time::Duration::from_millis(200)),
        )
        .is_err()
    {
        emit_line(crate::markers::M_SELFTEST_KEYSTORED_CAPMOVE_SEND_FAIL);
        return Err(());
    }

    // Receive response on reply inbox (nonblocking, bounded by time).
    let start_ns = nexus_abi::nsec().map_err(|_| ())?;
    let deadline_ns = start_ns.saturating_add(1_000_000_000); // 1s
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 128];
    let mut i: usize = 0;
    loop {
        if (i & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline_ns {
                break;
            }
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                let rsp = &buf[..n];
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
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => break,
        }
        i = i.wrapping_add(1);
    }
    emit_line(crate::markers::M_SELFTEST_KEYSTORED_CAPMOVE_NO_REPLY);
    Err(())
}
