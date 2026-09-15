// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-IPC soak probe — `ipc_soak_probe` runs a deterministic,
//!   bounded stress mix (~96 iterations) that catches CAP_MOVE reply routing,
//!   deadline/timeout, cap-table churn, and execd lifecycle regressions.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ipc_kernel phase.
//!
//! As of Cut P2-16, the previously-local `ReplyInboxV1` adapter is sourced
//! from `crate::os_lite::ipc::reply_inbox` (single source of truth).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use core::sync::atomic::{AtomicU64, Ordering};

use nexus_abi::ipc_recv_v1_nb;

use super::super::super::ipc::clients::cached_samgrd_client;
use super::plumbing::ipc_payload_roundtrip;
use super::security::cap_move_reply_probe;

/// Deterministic “soak” probe for IPC production-grade behaviour.
///
/// This is not a fuzz engine; it is a bounded, repeatable stress mix intended to catch:
/// - CAP_MOVE reply routing regressions
/// - deadline/timeout regressions
/// - cap_clone/cap_close leaks on common paths
/// - execd lifecycle regressions (spawn + wait)
pub(crate) fn ipc_soak_probe() -> core::result::Result<(), ()> {
    // Set up a few clients once (avoid repeated route lookups / allocations).
    let sam = cached_samgrd_client().map_err(|_| ())?;
    // The harness's declared reply inbox (pinned by init before the harness runs).
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let (reply_send_slot, reply_recv_slot) = (reply.send, reply.recv);

    // Keep it bounded so QEMU marker runs stay fast/deterministic and do not accumulate kernel heap.
    for _ in 0..96u32 {
        // B) Bootstrap payload roundtrip.
        ipc_payload_roundtrip()?;

        // C) CAP_MOVE ping to samgrd: ONE exchange, no clock (TASK-0324 P7-b).
        static NONCE: AtomicU64 = AtomicU64::new(0x1000);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let (sam_send, _) = sam.slots();
        let mut frame = [0u8; 12];
        frame[0] = b'S';
        frame[1] = b'M';
        frame[2] = 1;
        frame[3] = 3; // OP_PING_CAP_MOVE
        frame[4..12].copy_from_slice(&nonce.to_le_bytes());
        let mut rsp_buf = [0u8; 16];
        let n = nexus_ipc::exchange::call_into(sam_send, reply, &frame, &mut rsp_buf)
            .map_err(|_| ())?;
        let rsp = &rsp_buf[..n.min(rsp_buf.len())];
        let pong_nonce = rsp.get(4..12).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])));
        if rsp.len() != 12 || rsp[0..4] != *b"PONG" || pong_nonce != Some(nonce) {
            return Err(());
        }

        // D) cap_clone + immediate close (local drop) on reply cap to exercise cap table churn.
        let c = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        let _ = nexus_abi::cap_close(c);

        // Drain any stray replies so we don't accumulate queued messages if something raced.
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64];
        for _ in 0..8 {
            match ipc_recv_v1_nb(reply_recv_slot, &mut hdr, &mut buf, true) {
                Ok(_n) => {}
                Err(nexus_abi::IpcError::QueueEmpty) => break,
                Err(_) => break,
            }
        }
    }

    // Final sanity: ensure reply inbox still works after churn.
    cap_move_reply_probe()
}
