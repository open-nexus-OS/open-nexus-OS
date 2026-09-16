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
/// A policy exchange is WAITED for (TASK-0324 P8): the send waits for queue space, the receive
/// for policyd's answer or policyd's death (EOF on init's own reply endpoint) — no clock.
/// `None` = policyd refused to answer this shape or is gone (fail-closed at every caller).
///
/// The response endpoint carries policyd's answers and nothing else since TASK-0054C P2-d. It
/// used to double as init's inbox for its own outbound asks, and a bootctld or updated frame
/// arriving here made `decode_rsp_v2_or_v3(..)?` return `None` from the FUNCTION — a fail-closed
/// route DENY caused by an unrelated protocol. `policyd_cap_allowed` below had it worse: one
/// receive, no loop, so a foreign frame aborted an MMIO grant and with it the boot.
pub(crate) fn policyd_route_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    requester: &str,
    target: &[u8],
) -> Option<bool> {
    use crate::os_payload::{debug_write_byte, debug_write_bytes, debug_write_hex, POLICY_NONCE};

    if requester.len() > 48 || target.is_empty() || target.len() > 48 {
        return None;
    }
    let nonce = POLICY_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = [0u8; 10 + 48 + 48];
    let requester_id = nexus_abi::service_id_from_name(requester.as_bytes());
    let target_id = nexus_abi::service_id_from_name(target);
    let n = nexus_abi::policyd::encode_route_v3_id(nonce, requester_id, target_id, &mut frame)?;

    nexus_ipc::exchange::send_request(pol_send_slot, &frame[..n]).ok()?;
    let mut buf = [0u8; 16];
    loop {
        let got = nexus_ipc::exchange::recv_reply(pol_recv_slot, &mut buf).ok()?;
        let got = core::cmp::min(got, buf.len());
        // Skip what this exchange does not recognise, wait for its own nonce. This used to be
        // `decode(..)?`, which returned None from the FUNCTION — one unreadable frame became a
        // fail-closed route deny (TASK-0054C P2-d).
        let Some((_ver, op, got_nonce, status)) =
            nexus_abi::policyd::decode_rsp_v2_or_v3(&buf[..got])
        else {
            continue;
        };
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

    nexus_ipc::exchange::send_request(pol_send_slot, &frame[..n]).ok()?;
    let mut buf = [0u8; 16];
    let got = nexus_ipc::exchange::recv_reply(pol_recv_slot, &mut buf).ok()?;
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

    nexus_ipc::exchange::send_request(pol_send_slot, &frame[..n]).ok()?;
    let mut buf = [0u8; 16];
    loop {
        let got = nexus_ipc::exchange::recv_reply(pol_recv_slot, &mut buf).ok()?;
        let Some((_ver, op, got_nonce, status)) =
            nexus_abi::policyd::decode_rsp_v2_or_v3(&buf[..got.min(buf.len())])
        else {
            continue;
        };
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
