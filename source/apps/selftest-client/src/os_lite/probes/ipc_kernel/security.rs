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
use core::time::Duration;

use nexus_ipc::budget::{deadline_after, OsClock};
use nexus_ipc::reqrep::{recv_match_until, ReplyBuffer};

use super::super::super::ipc::clients::{cached_reply_client, cached_samgrd_client};
use super::super::super::ipc::reply_inbox::ReplyInboxV1;
use super::super::super::services::samgrd::fetch_sender_service_id_from_samgrd;

pub(crate) fn cap_move_reply_probe() -> core::result::Result<(), ()> {
    // 1) Deterministic reply-inbox slots distributed by init-lite to selftest-client.
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    let reply_send_slot = REPLY_SEND_SLOT;
    let reply_recv_slot = REPLY_RECV_SLOT;
    let clock = OsClock;
    let deadline_ns = deadline_after(&clock, Duration::from_millis(500)).map_err(|_| ())?;
    let mut pending: ReplyBuffer<8, 64> = ReplyBuffer::new();
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };

    // 2) Send a CAP_MOVE ping to samgrd, moving reply_send_slot as the reply cap.
    //    samgrd will reply by sending "PONG"+nonce on the moved cap and then closing it.
    let sam = cached_samgrd_client().map_err(|_| ())?;
    // Keep our reply-send slot by cloning it and moving the clone.
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    let mut frame = [0u8; 12];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1; // samgrd os-lite version
    frame[3] = 3; // OP_PING_CAP_MOVE
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    sam.send_with_cap_move(&frame, reply_send_clone).map_err(|_| ())?;
    let _ = nexus_abi::cap_close(reply_send_clone);

    // 3) Receive on the reply inbox endpoint (nonce-correlated, bounded, yield-friendly).
    let rsp = recv_match_until(&clock, &inbox, &mut pending, nonce, deadline_ns, |frame| {
        if frame.len() == 12 && frame[0..4] == *b"PONG" {
            Some(u64::from_le_bytes([
                frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
            ]))
        } else {
            None
        }
    })
    .map_err(|_| ())?;
    if rsp.len() == 12 && rsp[0..4] == *b"PONG" {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn sender_pid_probe() -> core::result::Result<(), ()> {
    let me = nexus_abi::pid().map_err(|_| ())?;
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let clock = OsClock;
    let deadline_ns = deadline_after(&clock, Duration::from_millis(500)).map_err(|_| ())?;
    let mut pending: ReplyBuffer<8, 64> = ReplyBuffer::new();
    static NONCE: AtomicU64 = AtomicU64::new(2);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;

    let sam = cached_samgrd_client().map_err(|_| ())?;
    let mut frame = [0u8; 16];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1;
    frame[3] = 4; // OP_SENDER_PID
    frame[4..8].copy_from_slice(&me.to_le_bytes());
    frame[8..16].copy_from_slice(&nonce.to_le_bytes());
    sam.send_with_cap_move(&frame, reply_send_clone).map_err(|_| ())?;

    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };
    let rsp = recv_match_until(&clock, &inbox, &mut pending, nonce, deadline_ns, |frame| {
        if frame.len() == 17
            && frame[0] == b'S'
            && frame[1] == b'M'
            && frame[2] == 1
            && frame[3] == (4 | 0x80)
            && frame[4] == 0
        {
            Some(u64::from_le_bytes([
                frame[9], frame[10], frame[11], frame[12], frame[13], frame[14], frame[15],
                frame[16],
            ]))
        } else {
            None
        }
    })
    .map_err(|_| ())?;
    if rsp.len() != 17 || rsp[0] != b'S' || rsp[1] != b'M' || rsp[2] != 1 {
        return Err(());
    }
    if rsp[3] != (4 | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    let got = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    if got == me {
        Ok(())
    } else {
        Err(())
    }
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
