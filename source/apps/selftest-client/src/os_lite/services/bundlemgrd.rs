// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: bundlemgrd v1 IPC client used by the selftest — list-bundles /
//!   list-images / route-execd-deny / malformed-frame reject probes.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — routing + policy phases.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::MsgHeader;
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use crate::markers::{emit_byte, emit_bytes, emit_line};

pub(crate) fn bundlemgrd_v1_list(client: &KernelClient) -> core::result::Result<(u8, u16), ()> {
    let mut req = [0u8; 4];
    nexus_abi::bundlemgrd::encode_list(&mut req);
    emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_SEND);
    // ONE waited send (queue space or bundlemgrd's death); the error is named once.
    let sent = match client.send(&req, IpcWait::Blocking) {
        Ok(()) => true,
        Err(err) => {
            {
                {
                    emit_bytes(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_SEND_ERR.as_bytes());
                    match err {
                        nexus_ipc::IpcError::NoSpace => emit_bytes(b"nospace"),
                        nexus_ipc::IpcError::WouldBlock => emit_bytes(b"wouldblock"),
                        nexus_ipc::IpcError::Timeout => emit_bytes(b"timeout"),
                        nexus_ipc::IpcError::Disconnected => emit_bytes(b"disconnected"),
                        nexus_ipc::IpcError::Unsupported => emit_bytes(b"unsupported"),
                        nexus_ipc::IpcError::Kernel(err) => {
                            emit_bytes(b"kernel:");
                            match err {
                                nexus_abi::IpcError::NoSuchEndpoint => emit_bytes(b"nosuch"),
                                nexus_abi::IpcError::QueueFull => emit_bytes(b"queuefull"),
                                nexus_abi::IpcError::QueueEmpty => emit_bytes(b"queueempty"),
                                nexus_abi::IpcError::PermissionDenied => emit_bytes(b"denied"),
                                nexus_abi::IpcError::TimedOut => emit_bytes(b"timedout"),
                                nexus_abi::IpcError::NoSpace => emit_bytes(b"nospace"),
                                nexus_abi::IpcError::PeerClosed => emit_bytes(b"peer-closed"),
                                nexus_abi::IpcError::Unsupported => emit_bytes(b"unsupported"),
                            }
                        }
                        _ => emit_bytes(b"other"),
                    }
                    emit_byte(b'\n');
                }
            }
            false
        }
    };
    if !sent {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_SEND_FAIL);
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_SENT);
    emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_RECV);
    for _ in 0..512 {
        match client.recv(IpcWait::Blocking) {
            Ok(rsp) => {
                if let Some(decoded) = nexus_abi::bundlemgrd::decode_list_rsp(&rsp) {
                    emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_RECV_OK);
                    return Ok(decoded);
                }
            }
            Err(nexus_ipc::IpcError::Timeout) | Err(nexus_ipc::IpcError::WouldBlock) => {}
            Err(err) => {
                emit_bytes(crate::markers::M_SELFTEST_BUNDLEMGRD_LIST_RECV_ERR.as_bytes());
                match err {
                    nexus_ipc::IpcError::NoSpace => emit_bytes(b"nospace"),
                    nexus_ipc::IpcError::Disconnected => emit_bytes(b"disconnected"),
                    nexus_ipc::IpcError::Unsupported => emit_bytes(b"unsupported"),
                    nexus_ipc::IpcError::Kernel(_) => emit_bytes(b"kernel"),
                    _ => emit_bytes(b"other"),
                }
                emit_byte(b'\n');
                return Err(());
            }
        }
    }
    Err(())
}

/// TASK-0321 P5: the volume probe that replaced the RFC-0012 fetch image —
/// `VOLUME_STATUS` must report a VERIFIED volume with ≥ 1 bundle (the
/// registry `pkg:/` and the launcher derive from it).
pub(crate) fn bundlemgrd_volume_status(client: &KernelClient) -> core::result::Result<u16, ()> {
    use nexus_abi::bundlemgrd as wire;
    let mut req = [0u8; 8];
    let n = wire::encode_volume_status(&mut req).ok_or(())?;
    nexus_ipc::Client::send(client, &req[..n], nexus_ipc::Wait::Blocking).map_err(|_| ())?;
    let rsp = nexus_ipc::Client::recv(client, nexus_ipc::Wait::Blocking).map_err(|_| ())?;
    let (status, _slot, verified, bundles, _build8) =
        wire::decode_volume_status_rsp(&rsp).ok_or(())?;
    if status != wire::STATUS_OK || verified != 1 || bundles == 0 {
        return Err(());
    }
    Ok(bundles)
}

pub(crate) fn bundlemgrd_v1_set_active_slot(
    client: &KernelClient,
    slot: u8,
) -> core::result::Result<(), ()> {
    let mut req = [0u8; 5];
    nexus_abi::bundlemgrd::encode_set_active_slot_req(slot, &mut req);
    nexus_ipc::Client::send(client, &req, nexus_ipc::Wait::Blocking).map_err(|_| ())?;
    let rsp = nexus_ipc::Client::recv(client, nexus_ipc::Wait::Blocking).map_err(|_| ())?;
    let (status, _slot) = nexus_abi::bundlemgrd::decode_set_active_slot_rsp(&rsp).ok_or(())?;
    if status == nexus_abi::bundlemgrd::STATUS_OK {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn bundlemgrd_v1_route_status(
    client: &KernelClient,
    target: &str,
) -> core::result::Result<(u8, u8), ()> {
    // Bundlemgrd v1 route-status:
    // Request: [B, N, ver, OP_ROUTE_STATUS, name_len:u8, name...]
    // Response: [B, N, ver, OP_ROUTE_STATUS|0x80, status:u8, route_status:u8, _, _]
    const MAGIC0: u8 = b'B';
    const MAGIC1: u8 = b'N';
    const VERSION: u8 = 1;
    const OP_ROUTE_STATUS: u8 = 2;

    let name = target.as_bytes();
    if name.is_empty() || name.len() > 48 {
        return Err(());
    }
    let mut req = Vec::with_capacity(5 + name.len());
    req.push(MAGIC0);
    req.push(MAGIC1);
    req.push(VERSION);
    req.push(OP_ROUTE_STATUS);
    req.push(name.len() as u8);
    req.extend_from_slice(name);
    let (send_slot, recv_slot) = client.slots();
    let hdr = MsgHeader::new(0, 0, 0, 0, req.len() as u32); // 2s
    if nexus_abi::ipc_send_v1(send_slot, &hdr, &req, 0, 0).is_err() {
        return Err(());
    }
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    loop {
        match nexus_abi::ipc_recv_v1(recv_slot, &mut rh, &mut buf, nexus_abi::IPC_SYS_TRUNCATE, 0) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n != 8 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
                    continue;
                }
                if buf[3] != (OP_ROUTE_STATUS | 0x80) {
                    continue;
                }
                return Ok((buf[4], buf[5]));
            }
            Err(_) => return Err(()),
        }
    }
}
