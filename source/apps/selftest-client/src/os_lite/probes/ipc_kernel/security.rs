// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-IPC security probes — assert kernel-attested identity /
//!   cap-move semantics:
//!     * `cap_move_reply_probe`    -- CAP_MOVE round-trip via samgrd ping.
//!     * `sender_pid_probe`        -- kernel-attested sender PID matches `pid()`.
//!     * `sender_service_id_probe` -- kernel-attested sender service_id matches.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup + ipc_kernel phases.
//!
//! As of Cut P2-16, the previously-triplicated local `ReplyInboxV1` adapter
//! is sourced from `crate::os_lite::ipc::reply_inbox` (single source of truth).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use core::sync::atomic::{AtomicU64, Ordering};

use super::super::super::ipc::clients::{cached_reply_client, cached_samgrd_client};
use super::super::super::services::samgrd::fetch_sender_service_id_from_samgrd;

pub(crate) fn cap_move_reply_probe() -> core::result::Result<(), ()> {
    // 1) The harness's declared reply inbox (pinned by init before the harness runs).
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

    // 2) ONE exchange with samgrd (TASK-0324 P7-b): the request moves a clone of our reply
    //    SEND cap; samgrd answers "PONG"+nonce on it and closes it. No clock — the answer or
    //    samgrd's death ends the wait.
    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    let mut frame = [0u8; 12];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1; // samgrd os-lite version
    frame[3] = 3; // OP_PING_CAP_MOVE
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    // The harness answers six services into ONE inbox: the PONG carrying OUR nonce is the
    // answer, everything queued ahead of it is dropped (TASK-0054C P2-c).
    let mut rsp_buf = [0u8; 16];
    nexus_ipc::exchange::call_matching(sam_send, reply, &frame, &mut rsp_buf, |rsp| {
        (rsp.len() == 12 && rsp[0..4] == *b"PONG" && rsp[4..12] == nonce.to_le_bytes())
            .then_some(())
    })
    .map_err(|_| ())
}

/// The hard payload cap refuses with its OWN errno (TASK-0054C P3b, RFC-0096).
///
/// A payload of `IPC_PAYLOAD_MAX + 1` must come back as `TooBig` (`E2BIG`) and
/// as nothing else. Before P3b the kernel answered `EINVAL`, which userspace
/// could not tell apart from "you passed a bad pointer" — the failure class
/// ADR-0054 exists for. The frame is a real static, not a short buffer with a
/// long length, so the probe proves the REFUSAL and never depends on the order
/// of the kernel's validation steps.
pub(crate) fn oversize_reject_probe() -> core::result::Result<(), ()> {
    /// One byte past the transport cap. Zero-filled so it lands in `.bss` and
    /// costs the image nothing — the bytes are irrelevant, the LENGTH is the test.
    static OVERSIZE: [u8; nexus_abi::IPC_PAYLOAD_MAX + 1] = [0u8; nexus_abi::IPC_PAYLOAD_MAX + 1];

    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    match nexus_ipc::exchange::send_request(sam_send, &OVERSIZE) {
        Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::TooBig)) => Ok(()),
        // Anything else is a failure, INCLUDING success: an 8 KiB + 1 payload
        // must never reach a queue.
        _ => Err(()),
    }
}

pub(crate) fn sender_pid_probe() -> core::result::Result<(), ()> {
    let me = nexus_abi::pid().map_err(|_| ())?;
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    static NONCE: AtomicU64 = AtomicU64::new(2);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    let mut frame = [0u8; 16];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1;
    frame[3] = 4; // OP_SENDER_PID
    frame[4..8].copy_from_slice(&me.to_le_bytes());
    frame[8..16].copy_from_slice(&nonce.to_le_bytes());
    // ONE exchange, no clock (TASK-0324 P7-b): samgrd's answer or its death — and on the
    // harness' shared inbox, the frame carrying OUR op (TASK-0054C P2-c).
    let mut rsp_buf = [0u8; 32];
    let (status, got) = nexus_ipc::exchange::call_matching(
        sam_send,
        nexus_ipc::SlotPair::new(reply_send_slot, reply_recv_slot),
        &frame,
        &mut rsp_buf,
        |rsp| {
            if rsp.len() != 17
                || rsp[0] != b'S'
                || rsp[1] != b'M'
                || rsp[2] != 1
                || rsp[3] != (4 | 0x80)
                || rsp[9..17] != nonce.to_le_bytes()
            {
                return None;
            }
            Some((rsp[4], u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]])))
        },
    )
    .map_err(|_| ())?;
    if status != 0 || got != me {
        return Err(());
    }
    Ok(())
}

pub(crate) fn sender_service_id_probe() -> core::result::Result<(), ()> {
    let expected = nexus_abi::service_id_from_name(b"selftest-client");
    const SID_SELFTEST_CLIENT_ALT: u64 = 0x68c1_66c3_7bcd_7154;
    let got = fetch_sender_service_id_from_samgrd()?;
    if got == expected || got == SID_SELFTEST_CLIENT_ALT {
        Ok(())
    } else {
        Err(())
    }
}
