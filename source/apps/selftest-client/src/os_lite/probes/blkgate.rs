// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cross-partition deny probe (TASK-0315, ADR-0044): the selftest
//! holds NO block-plane grant, so a WRITE to the `state` partition must
//! come back `STATUS_DENIED` from virtioblkd's kernel-attributed sender
//! gate. State-neutral by construction (a denied write mutates nothing) —
//! the standing detector shape, not a one-shot.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder (`SELFTEST: blk cross-partition deny ok`,
//!   `SELFTEST: blk system volume deny ok` — TASK-0321 op-aware gate).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use core::time::Duration;

use crate::markers::emit_line;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

const REPLY_RECV_SLOT: u32 = 0x17;
const REPLY_SEND_SLOT: u32 = 0x18;

// blockproto framing (SSOT: userspace/storage/src/blockproto.rs — the
// selftest speaks the raw wire on purpose: it must NOT link the client
// that would politely refuse to send an unauthorized request).
const MAGIC0: u8 = b'B';
const MAGIC1: u8 = b'K';
const VERSION: u8 = 1;
const OP_READ: u8 = 2;
const OP_WRITE: u8 = 3;
const PART_STATE: u8 = 0;
const PART_SYSTEM_A: u8 = 5;
const STATUS_DENIED: u8 = 5;

fn route_virtioblkd() -> Option<u32> {
    match budget::route_with_nonce_budgeted(
        b"virtioblkd",
        1,
        2,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, .. } => Some(send_slot),
        _ => None,
    }
}

/// Sends one unauthorized 512-byte WRITE to the `state` partition and
/// requires the DENIED status.
pub(crate) fn blk_cross_partition_deny_proof() {
    // WRITE to `state` (lba 0, zeroed payload — never lands).
    let mut frame = [0u8; 8 + 9 + 512];
    frame[8] = PART_STATE;
    deny_probe(
        OP_WRITE,
        &mut frame,
        0x5E1F_1315,
        crate::markers::M_SELFTEST_BLK_CROSS_PARTITION_DENY_OK,
        crate::markers::M_SELFTEST_BLK_CROSS_PARTITION_DENY_FAIL,
    );
}

/// TASK-0321 (RFC-0089 §12.5): the system volume is READ by bundlemgrd and
/// updated only — an ungranted READ of `system-a` (lba 0, one sector) must
/// come back `STATUS_DENIED` from the op-aware gate. State-neutral (a read).
pub(crate) fn blk_system_volume_deny_proof() {
    let mut frame = [0u8; 8 + 11];
    frame[8] = PART_SYSTEM_A;
    // lba 0 (u64le at 9..17), count 1 (u16le at 17..19).
    frame[17] = 1;
    deny_probe(
        OP_READ,
        &mut frame,
        0x5E1F_5A01,
        crate::markers::M_SELFTEST_BLK_SYSTEM_VOLUME_DENY_OK,
        crate::markers::M_SELFTEST_BLK_SYSTEM_VOLUME_DENY_FAIL,
    );
}

/// Sends one raw blockproto request (`op`, body already placed in `frame`
/// from byte 8) and expects `STATUS_DENIED` for OUR nonce on the shared
/// reply inbox. Any other outcome — route miss, send failure, timeout,
/// a non-denied status — prints `fail_marker`.
fn deny_probe(op: u8, frame: &mut [u8], nonce: u32, ok_marker: &str, fail_marker: &str) {
    let Some(send_slot) = route_virtioblkd() else {
        emit_line(fail_marker);
        return;
    };
    frame[0] = MAGIC0;
    frame[1] = MAGIC1;
    frame[2] = VERSION;
    frame[3] = op;
    frame[4..8].copy_from_slice(&nonce.to_le_bytes());

    let Ok(reply_clone) = nexus_abi::cap_clone(REPLY_SEND_SLOT) else {
        emit_line(fail_marker);
        return;
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(2_000_000_000);
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    let _ = nexus_abi::cap_close(reply_clone);
                    emit_line(fail_marker);
                    return;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => {
                let _ = nexus_abi::cap_close(reply_clone);
                emit_line(fail_marker);
                return;
            }
        }
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 64];
    loop {
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            emit_line(fail_marker);
            return;
        }
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                // Ours iff blockproto reply with our nonce (shared inbox).
                if n >= 9
                    && buf[0] == MAGIC0
                    && buf[1] == MAGIC1
                    && buf[2] == VERSION
                    && buf[3] == (op | 0x80)
                    && buf[4..8] == nonce.to_le_bytes()
                {
                    if buf[8] == STATUS_DENIED {
                        emit_line(ok_marker);
                    } else {
                        emit_line(fail_marker);
                    }
                    return;
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => {
                emit_line(fail_marker);
                return;
            }
        }
    }
}
