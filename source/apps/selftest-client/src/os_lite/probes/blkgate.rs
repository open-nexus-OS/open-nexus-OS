// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cross-partition deny probe (TASK-0315, ADR-0044): the selftest
//! holds NO block-plane grant, so a WRITE to the `state` partition must
//! come back `STATUS_DENIED` from blkd's kernel-attributed sender
//! gate. State-neutral by construction (a denied write mutates nothing) —
//! the standing detector shape, not a one-shot.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder (`SELFTEST: blk cross-partition deny ok`,
//!   `SELFTEST: blk system volume deny ok` — TASK-0321 op-aware gate).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use crate::markers::emit_line;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

use nexus_service_topology::slots::selftest_client::REPLY;

// blockproto framing (SSOT: userspace/storage/src/blockproto.rs — the
// selftest speaks the raw wire on purpose: it must NOT link the client
// that would politely refuse to send an unauthorized request).
const MAGIC0: u8 = b'B';
const MAGIC1: u8 = b'K';
const VERSION: u8 = 1;
const OP_READ: u8 = 2;
const OP_READ_VMO: u8 = 6;
const OP_WRITE: u8 = 3;
const PART_STATE: u8 = 0;
const PART_SYSTEM_A: u8 = 5;
const STATUS_DENIED: u8 = 5;

fn route_blkd() -> Option<u32> {
    match budget::route_with_nonce(b"blkd", NonceMismatchBudget::new(64)) {
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
    // TASK-0321 P4b: the bulk path is gated exactly like READ — an
    // unarmed READ_VMO from an ungranted sender must be DENIED (the gate
    // answers before the driver looks for an armed VMO). Both probes must
    // be denied for the single marker.
    let mut vmo_frame = [0u8; 8 + 25];
    vmo_frame[8] = PART_SYSTEM_A;
    // byte_off 0, len 512 (u64le at 17..25), vmo_off 0.
    vmo_frame[17] = 0x00;
    vmo_frame[18] = 0x02;
    if !deny_probe_quiet(OP_READ_VMO, &mut vmo_frame, 0x5E1F_5A02) {
        emit_line(crate::markers::M_SELFTEST_BLK_SYSTEM_VOLUME_DENY_FAIL);
        return;
    }
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
    if deny_probe_quiet(op, frame, nonce) {
        emit_line(ok_marker);
    } else {
        emit_line(fail_marker);
    }
}

/// The probe itself: `true` iff blkd answered OUR nonce with
/// `STATUS_DENIED`; every other outcome (route miss, send failure,
/// timeout, non-denied status) is `false`.
fn deny_probe_quiet(op: u8, frame: &mut [u8], nonce: u32) -> bool {
    let Some(send_slot) = route_blkd() else {
        return false;
    };
    frame[0] = MAGIC0;
    frame[1] = MAGIC1;
    frame[2] = VERSION;
    frame[3] = op;
    frame[4..8].copy_from_slice(&nonce.to_le_bytes());

    // The answer is the blockproto frame carrying OUR op and nonce on the harness' shared
    // inbox; everything else queued belongs to another probe (TASK-0054C P2-d).
    let mut buf = [0u8; 64];
    nexus_ipc::exchange::call_matching(send_slot, REPLY, frame, &mut buf, |rsp| {
        (rsp.len() >= 9
            && rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (op | 0x80)
            && rsp[4..8] == nonce.to_le_bytes())
        .then(|| rsp[8] == STATUS_DENIED)
    })
    .unwrap_or(false)
}
