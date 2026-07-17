// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Policyd integration helpers — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

use core::sync::atomic::Ordering;

/// policyd OP_ROUTE request (v3, nonce-correlated, ID-based).
pub(crate) fn policyd_route_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    requester: &str,
    target: &[u8],
) -> Option<bool> {
    use crate::os_payload::{
        debug_write_byte, debug_write_bytes, debug_write_hex, POLICY_NONCE,
    };

    if requester.len() > 48 || target.is_empty() || target.len() > 48 {
        return None;
    }
    let nonce = POLICY_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = [0u8; 10 + 48 + 48];
    let requester_id = nexus_abi::service_id_from_name(requester.as_bytes());
    let target_id = nexus_abi::service_id_from_name(target);
    let n = nexus_abi::policyd::encode_route_v3_id(nonce, requester_id, target_id, &mut frame)?;

    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(200_000_000),
        Err(_) => 0,
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(pol_send_slot, &hdr, &frame[..n], 0, deadline).is_err() {
        return None;
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    loop {
        let got = nexus_abi::ipc_recv_v1(
            pol_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        )
        .ok()? as usize;
        let got = core::cmp::min(got, buf.len());
        let (_ver, op, got_nonce, status) = nexus_abi::policyd::decode_rsp_v2_or_v3(&buf[..got])?;
        if op != nexus_abi::policyd::OP_ROUTE || got_nonce != nonce {
            continue;
        }
        if requester == "bundlemgrd" && target == b"execd" {
            debug_write_bytes(b"init: policyd route bundlemgrd->execd status=0x");
            debug_write_hex(status as usize);
            debug_write_byte(b'\n');
        }
        return match status {
            nexus_abi::policyd::STATUS_ALLOW => Some(true),
            nexus_abi::policyd::STATUS_DENY => Some(false),
            _ => None,
        };
    }
}

/// policyd OP_CHECK_CAP request (v1).
pub(crate) fn policyd_cap_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    subject_id: u64,
    cap: &[u8],
) -> Option<bool> {
    if cap.is_empty() || cap.len() > 48 {
        return None;
    }
    let mut frame = [0u8; 13 + 48];
    frame[0] = b'P';
    frame[1] = b'O';
    frame[2] = nexus_abi::policyd::VERSION_V1;
    frame[3] = nexus_abi::policyd::OP_CHECK_CAP;
    frame[4..12].copy_from_slice(&subject_id.to_le_bytes());
    frame[12] = cap.len() as u8;
    frame[13..13 + cap.len()].copy_from_slice(cap);
    let n = 13 + cap.len();

    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(1_000_000_000),
        Err(_) => 0,
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(pol_send_slot, &hdr, &frame[..n], 0, deadline).is_err() {
        return None;
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let got = nexus_abi::ipc_recv_v1(
        pol_recv_slot,
        &mut rh,
        &mut buf,
        nexus_abi::IPC_SYS_TRUNCATE,
        deadline,
    )
    .ok()? as usize;
    let got = core::cmp::min(got, buf.len());
    if got < 6 || buf[0] != b'P' || buf[1] != b'O' || buf[2] != nexus_abi::policyd::VERSION_V1 {
        return None;
    }
    if buf[3] != (nexus_abi::policyd::OP_CHECK_CAP | 0x80) {
        return None;
    }
    match buf[4] {
        nexus_abi::policyd::STATUS_ALLOW => Some(true),
        nexus_abi::policyd::STATUS_DENY => Some(false),
        _ => None,
    }
}

/// policyd OP_EXEC request (v3, nonce-correlated, ID-based).
pub(crate) fn policyd_exec_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    requester: &[u8],
    image_id: u8,
) -> Option<bool> {
    use crate::os_payload::POLICY_NONCE;

    if requester.is_empty() || requester.len() > 48 {
        return None;
    }
    let nonce = POLICY_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = [0u8; 10 + 48];
    let requester_id = nexus_abi::service_id_from_name(requester);
    let n = nexus_abi::policyd::encode_exec_v3_id(nonce, requester_id, image_id, &mut frame)?;

    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(1_000_000_000),
        Err(_) => 0,
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(pol_send_slot, &hdr, &frame[..n], 0, deadline).is_err() {
        return None;
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    loop {
        let got = nexus_abi::ipc_recv_v1(
            pol_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        )
        .ok()? as usize;
        let (_ver, op, got_nonce, status) = nexus_abi::policyd::decode_rsp_v2_or_v3(&buf[..got])?;
        if op != nexus_abi::policyd::OP_EXEC || got_nonce != nonce {
            continue;
        }
        return match status {
            nexus_abi::policyd::STATUS_ALLOW => Some(true),
            nexus_abi::policyd::STATUS_DENY => Some(false),
            _ => None,
        };
    }
}
