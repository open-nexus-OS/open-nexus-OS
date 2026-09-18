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

/// Request and reply in ONE trap (TASK-0054C P4a): the same samgrd ping the
/// cap-move probe does, over `nexus_abi::ipc_call`.
///
/// The reply is 12 bytes — inside the register tier — so the kernel finishes
/// this syscall in our saved frame and we never trap a second time to collect
/// it. What the marker proves is that the answer arrives INTACT that way: a
/// packing bug would show up as a wrong nonce, not as a hang.
pub(crate) fn ipc_call_probe() -> core::result::Result<(), ()> {
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    static NONCE: AtomicU64 = AtomicU64::new(0x4A00);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

    let sam = cached_samgrd_client().map_err(|_| ())?;
    let (sam_send, _) = sam.slots();
    let mut frame = [0u8; 12];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1; // samgrd os-lite version
    frame[3] = 3; // OP_PING_CAP_MOVE
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());

    // The moved capability IS the wait target: a clone of our reply SEND half.
    let clone = nexus_abi::cap_clone(reply.send).map_err(|_| ())?;
    let hdr = nexus_abi::MsgHeader::new(clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, 12);
    let mut out = [0u8; 16];
    let n = match nexus_abi::ipc_call(sam_send, &hdr, &frame, &mut out) {
        Ok(n) => n,
        Err(_) => {
            let _ = nexus_abi::cap_close(clone);
            return Err(());
        }
    };
    if n == 12 && out[0..4] == *b"PONG" && out[4..12] == nonce.to_le_bytes() {
        Ok(())
    } else {
        Err(())
    }
}

/// Reply and receive in ONE trap (TASK-0054C P4b), proven without changing a
/// single service.
///
/// The harness plays both roles against its OWN bootstrap endpoint, which makes
/// the proof deterministic and single-threaded: queue TWO requests to itself,
/// each moving a reply capability; take the first; then `ipc_reply_recv` —
/// which must answer request one AND hand back request two from the same trap.
/// The answer is then read off the reply inbox, so a reply that went nowhere
/// fails the probe instead of passing quietly.
pub(crate) fn reply_recv_probe() -> core::result::Result<(), ()> {
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let bootstrap = nexus_abi::BOOTSTRAP_CAP_SLOT;

    // Two requests to ourselves; the first carries the reply capability.
    let cap = nexus_abi::cap_clone(reply.send).map_err(|_| ())?;
    let hdr_a = nexus_abi::MsgHeader::new(cap, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, 3);
    nexus_abi::ipc_send_v1(bootstrap, &hdr_a, b"REQ", 0, 0).map_err(|_| ())?;
    let hdr_b = nexus_abi::MsgHeader::new(0, 0, 0, 0, 4);
    nexus_abi::ipc_send_v1(bootstrap, &hdr_b, b"NEXT", 0, 0).map_err(|_| ())?;

    // Take the first request and the reply capability it moved.
    let mut in_hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut in_buf = [0u8; 16];
    let mut sid: u64 = 0;
    let n = nexus_abi::ipc_recv_v2(
        bootstrap,
        &mut in_hdr,
        &mut in_buf,
        &mut sid,
        nexus_abi::IPC_SYS_TRUNCATE,
        0,
    )
    .map_err(|_| ())?;
    if &in_buf[..n as usize] != b"REQ" {
        return Err(());
    }
    let reply_cap = in_hdr.src;

    // ONE trap: answer request one, receive request two.
    let out_hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 4);
    let mut next_hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut next_buf = [0u8; 16];
    let mut next_sid: u64 = 0;
    let got = nexus_abi::ipc_reply_recv(
        reply_cap,
        &out_hdr,
        b"PONG",
        bootstrap,
        &mut next_hdr,
        &mut next_buf,
        &mut next_sid,
    )
    .map_err(|_| ())?;
    if &next_buf[..got] != b"NEXT" {
        return Err(());
    }

    // The answer really travelled: read it off our own reply inbox.
    let mut ack = [0u8; 16];
    let acked = nexus_ipc::exchange::recv_reply(reply.recv, &mut ack).map_err(|_| ())?;
    if &ack[..acked] == b"PONG" {
        Ok(())
    } else {
        Err(())
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
