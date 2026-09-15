// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: samgrd v1 IPC client + bring-up sequence used by the selftest
//!   (register / lookup / unknown-id reject / malformed-frame reject), plus
//!   the deterministic shared-inbox bootstrap used by reply-correlated probes.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use core::sync::atomic::{AtomicU64, Ordering};

use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use super::super::ipc::clients::{cached_reply_client, cached_samgrd_client};
use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};

pub(crate) fn samgrd_v1_register(
    client: &KernelClient,
    name: &str,
    send_slot: u32,
    recv_slot: u32,
) -> core::result::Result<u8, ()> {
    let n = name.as_bytes();
    if n.is_empty() || n.len() > 48 {
        return Err(());
    }
    let mut req = Vec::with_capacity(13 + n.len());
    req.push(b'S');
    req.push(b'M');
    req.push(1);
    req.push(1);
    req.push(n.len() as u8);
    req.extend_from_slice(&send_slot.to_le_bytes());
    req.extend_from_slice(&recv_slot.to_le_bytes());
    req.extend_from_slice(n);
    let (_client_send, client_recv) = client.slots();
    let mut logged_start = false;
    let mut logged_send_fail = false;
    let mut logged_rsp = false;
    for _ in 0..64 {
        if !logged_start {
            emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND);
            logged_start = true;
        }
        if let Err(err) = client.send(&req, IpcWait::Blocking) {
            if !logged_send_fail {
                match err {
                    nexus_ipc::IpcError::NoSpace => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_NOSPACE);
                    }
                    nexus_ipc::IpcError::Timeout => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_TIMEOUT);
                    }
                    nexus_ipc::IpcError::Disconnected => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_DISCONNECTED);
                    }
                    nexus_ipc::IpcError::WouldBlock => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_WOULDBLOCK);
                    }
                    nexus_ipc::IpcError::Unsupported => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_UNSUPPORTED);
                    }
                    nexus_ipc::IpcError::Kernel(_) => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_KERNEL);
                    }
                    _ => {
                        emit_line(crate::markers::M_SELFTEST_SAMGRD_REGISTER_SEND_FAIL);
                    }
                }
                logged_send_fail = true;
            }
            continue;
        }
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        for _ in 0..128 {
            match nexus_abi::ipc_recv_v1(
                client_recv,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    if !logged_rsp {
                        emit_bytes(crate::markers::M_SELFTEST_SAMGRD_REGISTER_RSP_LEN.as_bytes());
                        emit_hex_u64(n as u64);
                        emit_bytes(b" head=");
                        if n >= 8 {
                            emit_hex_u64(u64::from_le_bytes([
                                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
                            ]));
                        } else if n >= 4 {
                            emit_hex_u64(
                                u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64
                            );
                        } else {
                            emit_hex_u64(0);
                        }
                        emit_byte(b'\n');
                        logged_rsp = true;
                    }
                    let n = n as usize;
                    let rsp = &buf[..n];
                    if rsp.len() != 13 || rsp[0] != b'S' || rsp[1] != b'M' || rsp[2] != 1 {
                        continue;
                    }
                    if rsp[3] != (1 | 0x80) {
                        continue;
                    }
                    return Ok(rsp[4]);
                }
                Err(_) => break,
            }
        }
    }
    Err(())
}

pub(crate) fn samgrd_v1_lookup(
    client: &KernelClient,
    target: &str,
) -> core::result::Result<(u8, u32, u32), ()> {
    let name = target.as_bytes();
    if name.is_empty() || name.len() > 48 {
        return Err(());
    }
    let mut req = Vec::with_capacity(5 + name.len());
    req.push(b'S');
    req.push(b'M');
    req.push(1);
    req.push(2);
    req.push(name.len() as u8);
    req.extend_from_slice(name);
    let (_client_send, client_recv) = client.slots();
    let mut logged_rsp = false;
    for _ in 0..64 {
        if client.send(&req, IpcWait::Blocking).is_err() {
            continue;
        }
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        for _ in 0..128 {
            match nexus_abi::ipc_recv_v1(
                client_recv,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    if !logged_rsp {
                        emit_bytes(crate::markers::M_SELFTEST_SAMGRD_LOOKUP_RSP_LEN.as_bytes());
                        emit_hex_u64(n as u64);
                        emit_bytes(b" head=");
                        if n >= 8 {
                            emit_hex_u64(u64::from_le_bytes([
                                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
                            ]));
                        } else if n >= 4 {
                            emit_hex_u64(
                                u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as u64
                            );
                        } else {
                            emit_hex_u64(0);
                        }
                        emit_byte(b'\n');
                        logged_rsp = true;
                    }
                    let n = n as usize;
                    let rsp = &buf[..n];
                    if rsp.len() != 13 || rsp[0] != b'S' || rsp[1] != b'M' || rsp[2] != 1 {
                        continue;
                    }
                    if rsp[3] != (2 | 0x80) {
                        continue;
                    }
                    let status = rsp[4];
                    let send_slot = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
                    let recv_slot = u32::from_le_bytes([rsp[9], rsp[10], rsp[11], rsp[12]]);
                    return Ok((status, send_slot, recv_slot));
                }
                Err(_) => break,
            }
        }
    }
    Err(())
}

pub(crate) fn fetch_sender_service_id_from_samgrd() -> core::result::Result<u64, ()> {
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    static NONCE: AtomicU64 = AtomicU64::new(3);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    let mut frame = [0u8; 12];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1;
    frame[3] = 5; // OP_SENDER_SERVICE_ID
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    // ONE exchange, no clock (TASK-0324 P7-b): samgrd's answer or its death.
    let mut rsp_buf = [0u8; 32];
    let n = nexus_ipc::exchange::call_into(
        sam_send,
        nexus_service_topology::SlotPair::new(reply_send_slot, reply_recv_slot),
        &frame,
        &mut rsp_buf,
    )
    .map_err(|_| ())?;
    let rsp = &rsp_buf[..n.min(rsp_buf.len())];
    if rsp.len() != 21 || rsp[3] != (5 | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    let got_nonce = u64::from_le_bytes([
        rsp[13], rsp[14], rsp[15], rsp[16], rsp[17], rsp[18], rsp[19], rsp[20],
    ]);
    if got_nonce != nonce {
        return Err(());
    }

    if rsp.len() != 21 || rsp[0] != b'S' || rsp[1] != b'M' || rsp[2] != 1 {
        return Err(());
    }
    if rsp[3] != (5 | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    let got =
        u64::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8], rsp[9], rsp[10], rsp[11], rsp[12]]);
    Ok(got)
}
