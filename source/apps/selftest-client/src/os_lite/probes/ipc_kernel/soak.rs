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
use core::time::Duration;

use nexus_abi::{ipc_recv_v1_nb, yield_};
use nexus_ipc::budget::{deadline_after, OsClock};
use nexus_ipc::reqrep::{recv_match_until, ReplyBuffer};
use nexus_ipc::Wait as IpcWait;

use super::super::super::ipc::clients::cached_samgrd_client;
use super::super::super::ipc::reply_inbox::ReplyInboxV1;
use super::plumbing::{ipc_deadline_timeout_probe, ipc_payload_roundtrip};
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
    // Deterministic reply inbox slots distributed by init-lite to selftest-client.
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    let reply_send_slot = REPLY_SEND_SLOT;
    let reply_recv_slot = REPLY_RECV_SLOT;

    // Keep it bounded so QEMU marker runs stay fast/deterministic and do not accumulate kernel heap.
    for _ in 0..96u32 {
        // A) Deadline semantics probe (must timeout).
        ipc_deadline_timeout_probe()?;

        // B) Bootstrap payload roundtrip.
        ipc_payload_roundtrip()?;

        // C) CAP_MOVE ping to samgrd + reply receive (robust against shared inbox mixing).
        let clock = OsClock;
        let deadline_ns = deadline_after(&clock, Duration::from_millis(200)).map_err(|_| ())?;
        let mut pending: ReplyBuffer<8, 64> = ReplyBuffer::new();
        static NONCE: AtomicU64 = AtomicU64::new(0x1000);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        let mut frame = [0u8; 12];
        frame[0] = b'S';
        frame[1] = b'M';
        frame[2] = 1;
        frame[3] = 3; // OP_PING_CAP_MOVE
        frame[4..12].copy_from_slice(&nonce.to_le_bytes());
        let wait = IpcWait::Timeout(core::time::Duration::from_millis(10));
        let mut sent = false;
        for _ in 0..64 {
            match sam.send_with_cap_move_wait(&frame, reply_send_clone, wait) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        if !sent {
            let _ = nexus_abi::cap_close(reply_send_clone);
            return Err(());
        }
        let _ = nexus_abi::cap_close(reply_send_clone);

        let inbox = ReplyInboxV1 {
            recv_slot: reply_recv_slot,
        };
        let rsp = recv_match_until(&clock, &inbox, &mut pending, nonce, deadline_ns, |frame| {
            if frame.len() == 12 && frame[0..4] == *b"PONG" {
                Some(u64::from_le_bytes([
                    frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10],
                    frame[11],
                ]))
            } else {
                None
            }
        })
        .map_err(|_| ())?;
        if rsp.len() != 12 || rsp[0..4] != *b"PONG" {
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
