// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: bootctld wire-reply shaping + persist-or-restore commit
//! (split out of `os_lite.rs` under the structure ratchet; behavior
//! unchanged). Owns the response encoders, the deny/audit line, the
//! delegated policyd check, and the commit discipline: the record on
//! disk and the machine in RAM commit together or not at all, and a
//! gated marker fires ONLY after the record persisted.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (OTA + reset + nxra chains, markers gated).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use crate::machine::{BootCtrl, BootCtrlError};
use crate::os_lite::{emit, Authority, POLICYD_SEND_SLOT, REPLY_RECV_SLOT, REPLY_SEND_SLOT};
use crate::persist_os::persist_record;
use crate::wire;

/// Persist-or-restore: the record on disk and the machine in RAM commit
/// together or not at all.
pub(crate) fn commit(
    auth: &mut Authority,
    snapshot: BootCtrl,
    rsp: &mut [u8; 32],
    op: u8,
    payload: &[u8],
) -> usize {
    commit_marked(auth, snapshot, rsp, op, payload, None)
}

/// `commit` that emits `marker` ONLY after the record persisted — a
/// scheduled-switch claim before the disk commit would be fake green.
pub(crate) fn commit_marked(
    auth: &mut Authority,
    snapshot: BootCtrl,
    rsp: &mut [u8; 32],
    op: u8,
    payload: &[u8],
    marker: Option<&str>,
) -> usize {
    match persist_record(&auth.client, &auth.boot) {
        Ok(()) => {
            if let Some(line) = marker {
                emit(line);
            }
            encode_payload(rsp, op, payload)
        }
        Err(_) => {
            auth.boot = snapshot;
            encode_status(rsp, op, wire::STATUS_FAILED)
        }
    }
}

pub(crate) fn machine_fail(rsp: &mut [u8; 32], op: u8, err: BootCtrlError) -> usize {
    // Deterministic machine rejects map to FAILED with the reason byte so
    // the client's audit detail stays truthful.
    let reason = match err {
        BootCtrlError::NotStaged => 1,
        BootCtrlError::AlreadyPending => 2,
        BootCtrlError::NotPending => 3,
        BootCtrlError::NoRollbackTarget => 4,
    };
    let base = encode_status(rsp, op, wire::STATUS_FAILED);
    rsp[5..7].copy_from_slice(&1u16.to_le_bytes());
    rsp[base] = reason;
    base + 1
}

/// Delegated capability check (deny-by-default; Unreachable = deny).
pub(crate) fn policy_allows(sender: u64, cap: &[u8]) -> bool {
    matches!(
        nexus_ipc::policyd::check_cap_on(
            POLICYD_SEND_SLOT,
            REPLY_SEND_SLOT,
            REPLY_RECV_SLOT,
            sender,
            cap,
        ),
        nexus_ipc::policyd::CapDecision::Allow
    )
}

pub(crate) fn deny(rsp: &mut [u8; 32], op: u8, sender: u64) -> usize {
    emit_deny(op, sender);
    encode_status(rsp, op, wire::STATUS_DENIED)
}

pub(crate) fn encode_status(rsp: &mut [u8; 32], op: u8, status: u8) -> usize {
    rsp[0] = wire::MAGIC0;
    rsp[1] = wire::MAGIC1;
    rsp[2] = wire::VERSION;
    rsp[3] = op | 0x80;
    rsp[4] = status;
    rsp[5] = 0;
    rsp[6] = 0;
    7
}

pub(crate) fn encode_payload(rsp: &mut [u8; 32], op: u8, payload: &[u8]) -> usize {
    let base = encode_status(rsp, op, wire::STATUS_OK);
    let len = payload.len().min(rsp.len() - base);
    rsp[5..7].copy_from_slice(&(len as u16).to_le_bytes());
    rsp[base..base + len].copy_from_slice(&payload[..len]);
    base + len
}

pub(crate) fn emit_deny(op: u8, sender: u64) {
    let mut line = [0u8; 64];
    let mut len = 0usize;
    let head = b"bootctld: denied op=0x";
    line[..head.len()].copy_from_slice(head);
    len += head.len();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    line[len] = HEX[(op >> 4) as usize];
    line[len + 1] = HEX[(op & 0xf) as usize];
    len += 2;
    let mid = b" sender=0x";
    line[len..len + mid.len()].copy_from_slice(mid);
    len += mid.len();
    for shift in (0..16).rev() {
        line[len] = HEX[((sender >> (shift * 4)) & 0xf) as usize];
        len += 1;
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}
