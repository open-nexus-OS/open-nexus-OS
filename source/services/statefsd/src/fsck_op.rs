// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: statefsd fsck op core (TASK-0051) — cfg-free transport/gating
//! layer over the SHIPPED 0026 engine (`statefs::fsck_with_enc` is the
//! ONE repair semantics; this layer adds wire encode + the quiesce gate
//! and nothing else). The repair bound is structural: repair appends at
//! most one abort per orphan and open transactions are capped at
//! `MAX_OPEN_TXNS` (8) — no separate budget knob to drift. Quiesce:
//! fsck re-opens the journal from the raw device, which would silently
//! DROP any open (RAM-staged) transaction — the gate rejects with
//! `STATUS_BUSY` instead of racing.
//!
//! Wire report v1 (fixed 34 bytes, response payload of
//! `OP_FSCK_CHECK`/`OP_FSCK_REPAIR`):
//! `[0] ver=1 | [1] outcome (0 clean, 1 repaired, 2 unrecoverable)
//!  | [2] layout (1/2) | [3] repaired flag | [4..8] generation u32le
//!  | [8..12] records u32le | [12..16] entries u32le | [16] orphan_count
//!  | [17] anomalies (saturated) | [18] tail_dirty | [19] enc_failures
//!  (saturated) | [20..24] enc_records u32le | [24] fault flag
//!  | [25] reserved reason code | [26..34] fault offset u64le`
//!
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (wire layout is the contract surface)
//! TEST_COVERAGE: tests/fsck_op_contract.rs (outcome classes over a mock
//!   store, quiesce/Busy, report roundtrip, exit-code parity).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md (ops lane)

extern crate alloc;

use statefs::{FsckOutcome, FsckReport, JournalLayout};

/// Fixed wire length of the fsck report payload.
pub const FSCK_REPORT_WIRE_LEN: usize = 34;
/// Wire version byte of the report layout.
pub const FSCK_REPORT_VERSION: u8 = 1;

/// Quiesce gate: fsck may only run while NO transaction is open — the
/// engine re-open drops RAM-staged transactions, so racing one would be
/// silent data loss (the exact class RFC-0087 forbids).
pub fn quiesce_ok(open_txns: usize) -> bool {
    open_txns == 0
}

/// Maps an engine outcome to the wire status byte of the op response
/// (mirrors the fsck-statefs exit-code contract: clean-with-enc-failures
/// is NOT ok).
pub fn outcome_status(report: &FsckReport) -> u8 {
    match report.outcome {
        FsckOutcome::Clean if report.enc_failures > 0 => {
            statefs::protocol::STATUS_INTEGRITY_VIOLATION
        }
        FsckOutcome::Clean => statefs::protocol::STATUS_OK,
        FsckOutcome::Repaired => statefs::protocol::STATUS_OK,
        FsckOutcome::Unrecoverable => statefs::protocol::STATUS_IO_ERROR,
    }
}

/// Encodes the report as the fixed wire payload.
pub fn encode_report(report: &FsckReport) -> [u8; FSCK_REPORT_WIRE_LEN] {
    let mut out = [0u8; FSCK_REPORT_WIRE_LEN];
    out[0] = FSCK_REPORT_VERSION;
    out[1] = match report.outcome {
        FsckOutcome::Clean => 0,
        FsckOutcome::Repaired => 1,
        FsckOutcome::Unrecoverable => 2,
    };
    out[2] = match report.layout {
        JournalLayout::V1 => 1,
        JournalLayout::V2 => 2,
    };
    out[3] = u8::from(report.repaired);
    out[4..8].copy_from_slice(&report.generation.to_le_bytes());
    out[8..12].copy_from_slice(&(report.records.min(u32::MAX as usize) as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(report.entries.min(u32::MAX as usize) as u32).to_le_bytes());
    out[16] = report.orphan_txns.len().min(255) as u8;
    out[17] = report.anomalies.min(255) as u8;
    out[18] = u8::from(report.tail_dirty);
    out[19] = report.enc_failures.min(255) as u8;
    out[20..24].copy_from_slice(&(report.enc_records.min(u32::MAX as usize) as u32).to_le_bytes());
    if let Some(fault) = &report.fault {
        out[24] = 1;
        out[26..34].copy_from_slice(&fault.offset.to_le_bytes());
    }
    out
}

/// Decoded wire report (client side — `nx diagnose` and the selftest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireFsckReport {
    pub outcome: u8,
    pub layout: u8,
    pub repaired: bool,
    pub generation: u32,
    pub records: u32,
    pub entries: u32,
    pub orphan_count: u8,
    pub anomalies: u8,
    pub tail_dirty: bool,
    pub enc_failures: u8,
    pub enc_records: u32,
    pub fault_offset: Option<u64>,
}

/// Decodes the fixed wire payload; `None` on any layout violation.
pub fn decode_report(payload: &[u8]) -> Option<WireFsckReport> {
    if payload.len() != FSCK_REPORT_WIRE_LEN || payload[0] != FSCK_REPORT_VERSION {
        return None;
    }
    if payload[1] > 2 || !(1..=2).contains(&payload[2]) {
        return None;
    }
    Some(WireFsckReport {
        outcome: payload[1],
        layout: payload[2],
        repaired: payload[3] == 1,
        generation: u32::from_le_bytes(payload[4..8].try_into().ok()?),
        records: u32::from_le_bytes(payload[8..12].try_into().ok()?),
        entries: u32::from_le_bytes(payload[12..16].try_into().ok()?),
        orphan_count: payload[16],
        anomalies: payload[17],
        tail_dirty: payload[18] == 1,
        enc_failures: payload[19],
        enc_records: u32::from_le_bytes(payload[20..24].try_into().ok()?),
        fault_offset: if payload[24] == 1 {
            Some(u64::from_le_bytes(payload[26..34].try_into().ok()?))
        } else {
            None
        },
    })
}
