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

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

use crate::markers::{emit_byte, emit_bytes, emit_line};

pub(crate) fn bundlemgrd_v1_list(client: &KernelClient) -> core::result::Result<(u8, u16), ()> {
    let mut req = [0u8; 4];
    nexus_abi::bundlemgrd::encode_list(&mut req);
    emit_line("SELFTEST: bundlemgrd list send");
    let mut sent = false;
    let mut logged_send_err = false;
    for _ in 0..256 {
        match client.send(&req, IpcWait::NonBlocking) {
            Ok(()) => {
                sent = true;
                break;
            }
            Err(err) => {
                if !logged_send_err {
                    emit_bytes(b"SELFTEST: bundlemgrd list send err ");
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
                                nexus_abi::IpcError::Unsupported => emit_bytes(b"unsupported"),
                            }
                        }
                        _ => emit_bytes(b"other"),
                    }
                    emit_byte(b'\n');
                    logged_send_err = true;
                }
            }
        }
        let _ = yield_();
    }
    if !sent {
        emit_line("SELFTEST: bundlemgrd list send fail");
        return Err(());
    }
    emit_line("SELFTEST: bundlemgrd list sent");
    emit_line("SELFTEST: bundlemgrd list recv");
    for _ in 0..512 {
        match client.recv(IpcWait::Timeout(core::time::Duration::from_millis(10))) {
            Ok(rsp) => {
                if let Some(decoded) = nexus_abi::bundlemgrd::decode_list_rsp(&rsp) {
                    emit_line("SELFTEST: bundlemgrd list recv ok");
                    return Ok(decoded);
                }
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Timeout) | Err(nexus_ipc::IpcError::WouldBlock) => {
                let _ = yield_();
            }
            Err(err) => {
                emit_bytes(b"SELFTEST: bundlemgrd list recv err ");
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

pub(crate) fn bundlemgrd_v1_fetch_image(client: &KernelClient) -> core::result::Result<(), ()> {
    bundlemgrd_v1_fetch_image_slot(client, None)
}

pub(crate) fn bundlemgrd_v1_fetch_image_slot(
    client: &KernelClient,
    expected_slot: Option<u8>,
) -> core::result::Result<(), ()> {
    let mut req = [0u8; 4];
    nexus_abi::bundlemgrd::encode_fetch_image(&mut req);
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, client, &req, core::time::Duration::from_secs(1))
        .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, client, core::time::Duration::from_secs(1))
        .map_err(|_| ())?;
    let (st, img) = nexus_abi::bundlemgrd::decode_fetch_image_rsp(&rsp).ok_or(())?;
    if st != nexus_abi::bundlemgrd::STATUS_OK {
        return Err(());
    }
    let (count, mut off) = nexus_abi::bundleimg::decode_header(img).ok_or(())?;
    if count == 0 {
        return Err(());
    }
    let mut slot_ok = expected_slot.is_none();
    for _ in 0..count {
        let entry = nexus_abi::bundleimg::decode_next(img, &mut off).ok_or(())?;
        if entry.path == b"build.prop" {
            if let Some(slot) = expected_slot {
                let mut needle = Vec::with_capacity(15);
                needle.extend_from_slice(b"ro.nexus.slot=");
                needle.push(slot);
                needle.push(b'\n');
                if entry.data.windows(needle.len()).any(|w| w == needle.as_slice()) {
                    slot_ok = true;
                }
            }
        }
    }
    if !slot_ok {
        return Err(());
    }
    Ok(())
}

pub(crate) fn bundlemgrd_v1_set_active_slot(
    client: &KernelClient,
    slot: u8,
) -> core::result::Result<(), ()> {
    let mut req = [0u8; 5];
    nexus_abi::bundlemgrd::encode_set_active_slot_req(slot, &mut req);
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, client, &req, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    let rsp =
        nexus_ipc::budget::recv_budgeted(&clock, client, core::time::Duration::from_millis(200))
            .map_err(|_| ())?;
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
    let mut buf = [0u8; 16];
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
                if n != 8 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
                    continue;
                }
                if buf[3] != (OP_ROUTE_STATUS | 0x80) {
                    continue;
                }
                return Ok((buf[4], buf[5]));
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    }
}
