// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd fsck ops OS glue (TASK-0051). Policy: CHECK needs
//! `statefs.read`, REPAIR needs `statefs.admin` (deny-by-default, audited
//! by the deny marker). Quiesce: BOTH ops reject with `STATUS_BUSY` while
//! transactions are open — fsck re-opens the journal from the raw device
//! and would silently drop RAM-staged transactions. Engine ownership:
//! `fsck` consumes the device, so the live engine is swapped for a tiny
//! mem placeholder, fsck runs, and the store re-opens (virtio-upgrade
//! pattern: fresh SeqTracker + enrolled replay + enc re-enable). A store
//! that cannot re-open degrades LOUD to the mem placeholder
//! (`statefsd: fsck fail (unrecoverable)` — fatal in proof boots): the
//! journal was unrecoverable, RAM-only is the honest remainder.
//! Sealed-value verify runs on the Host twin (`fsck-statefs --enc-…`):
//! `EncContext` is engine-owned and non-clonable, and the re-open replay
//! AEAD-verifies enrolled records anyway.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: cfg-free core host-tested (tests/fsck_op_contract.rs);
//!   this glue is QEMU-proven (recovery lane fsck markers).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md (ops lane)

extern crate alloc;

use alloc::vec::Vec;

use statefs::protocol as proto;
use statefs::JournalEngine;
use storage::MemBlockDevice;

use crate::emit_os::emit_line;
use crate::fsck_op;
use crate::os_lite::{policyd_allows, Backend, Hardening};

/// Handles `OP_FSCK_CHECK` / `OP_FSCK_REPAIR`; returns the response frame.
pub(crate) fn handle_fsck_frame(
    engine: &mut JournalEngine<Backend>,
    hard: &mut Hardening,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    let op = frame.get(3).copied().unwrap_or(proto::OP_FSCK_CHECK);
    let nonce = decode_nonce(frame);
    let repair = op == proto::OP_FSCK_REPAIR;
    let cap: &[u8] = if repair { b"statefs.admin" } else { b"statefs.read" };
    if !policyd_allows(sender_service_id, cap) {
        crate::emit_os::emit_access_denied("/state (fsck)", sender_service_id);
        return proto::encode_status_response_with_nonce(op, proto::STATUS_ACCESS_DENIED, nonce);
    }
    if !fsck_op::quiesce_ok(engine.open_txns()) {
        emit_line("statefsd: fsck busy (open txns)");
        return proto::encode_status_response_with_nonce(op, proto::STATUS_BUSY, nonce);
    }

    // Swap in a placeholder BEFORE consuming the live engine — if the
    // placeholder cannot even open, the store stays untouched.
    let placeholder = match JournalEngine::open(Backend::Mem(MemBlockDevice::new(512, 64))) {
        Ok(engine) => engine,
        Err(_) => {
            return proto::encode_status_response_with_nonce(op, proto::STATUS_IO_ERROR, nonce)
        }
    };
    let live = core::mem::replace(engine, placeholder);
    let device = live.into_device();
    let (report, device_back) = statefs::fsck_with_enc(device, repair, None);

    // Re-open the store (virtio-upgrade pattern). Failure = the honest
    // RAM-only degradation, never a fabricated success.
    let mut reopened = false;
    if let Some(device) = device_back {
        if let Ok(new_engine) = JournalEngine::open(device) {
            *engine = new_engine;
            hard.tracker = statefs::envelope::SeqTracker::new();
            crate::hardening_os::observe_enrolled(engine, &mut hard.tracker);
            crate::enc_os::try_enable(engine, false);
            reopened = true;
        }
    }
    if !reopened || report.outcome == statefs::FsckOutcome::Unrecoverable {
        emit_line("statefsd: fsck fail (unrecoverable)");
    } else if report.repaired {
        emit_repaired(report.orphan_txns.len());
    } else {
        emit_line("statefsd: fsck check ok (clean)");
    }

    let payload = fsck_op::encode_report(&report);
    let status = if reopened { fsck_op::outcome_status(&report) } else { proto::STATUS_IO_ERROR };
    encode_report_response(op, status, &payload, nonce)
}

fn emit_repaired(orphans: usize) {
    // orphans ≤ MAX_OPEN_TXNS (8): a single digit is the whole story.
    let mut line = *b"statefsd: fsck repaired (n=0)";
    let idx = line.len() - 2;
    line[idx] = b'0' + (orphans.min(9) as u8);
    if let Ok(msg) = core::str::from_utf8(&line) {
        emit_line(msg);
    }
}

fn decode_nonce(frame: &[u8]) -> Option<u64> {
    if frame.len() >= 12 && frame.get(2).copied() == Some(proto::VERSION_V2) {
        Some(u64::from_le_bytes([
            frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
        ]))
    } else {
        None
    }
}

/// GET-shaped response so clients reuse the value decoder: status + u32
/// length + report payload (v2 inserts the nonce like every other reply).
fn encode_report_response(op: u8, status: u8, payload: &[u8], nonce: Option<u64>) -> Vec<u8> {
    let mut out = Vec::with_capacity(21 + payload.len());
    out.push(proto::MAGIC0);
    out.push(proto::MAGIC1);
    out.push(if nonce.is_some() { proto::VERSION_V2 } else { proto::VERSION });
    out.push(op | 0x80);
    out.push(status);
    if let Some(n) = nonce {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}
