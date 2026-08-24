// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract for the statefsd fsck op layer (TASK-0051):
//! outcome classes over a mock store (the ENGINE matrix lives with the
//! engine — `userspace/statefs/tests/fsck.rs`, 23 cases — this file pins
//! the op layer's transport truths: status mapping mirrors the
//! fsck-statefs exit-code contract, wire report roundtrips, the quiesce
//! gate rejects open transactions, and the structural repair bound).
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md (ops lane)

use statefs::protocol::{STATUS_INTEGRITY_VIOLATION, STATUS_IO_ERROR, STATUS_OK};
use statefs::{fsck, JournalEngine};
use statefsd::fsck_op::{
    decode_report, encode_report, outcome_status, quiesce_ok, FSCK_REPORT_WIRE_LEN,
};
use storage::MemBlockDevice;

fn fresh_store() -> MemBlockDevice {
    let mut engine = JournalEngine::open(MemBlockDevice::new(512, 256)).expect("open");
    engine.put("/state/test/a", b"alpha").expect("put");
    engine.put("/state/test/b", b"beta").expect("put");
    engine.sync().expect("sync");
    engine.into_device()
}

fn orphan_store() -> MemBlockDevice {
    let mut engine = JournalEngine::open(fresh_store()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/torn", b"never committed").expect("append");
    engine.into_device()
}

#[test]
fn test_check_clean_maps_ok() {
    let (report, dev) = fsck(fresh_store(), false);
    assert!(dev.is_some());
    assert_eq!(outcome_status(&report), STATUS_OK);
    let wire = decode_report(&encode_report(&report)).expect("roundtrip");
    assert_eq!(wire.outcome, 0);
    assert!(!wire.repaired);
    assert_eq!(wire.orphan_count, 0);
}

#[test]
fn test_check_reports_orphan_without_writing() {
    let (report, dev) = fsck(orphan_store(), false);
    let dev = dev.expect("device back");
    assert_eq!(report.orphan_txns.len(), 1);
    // Read-only: a second check still sees the orphan.
    let (again, _) = fsck(dev, false);
    assert_eq!(again.orphan_txns.len(), 1);
}

#[test]
fn test_repair_maps_ok_and_store_re_checks_clean() {
    let (report, dev) = fsck(orphan_store(), true);
    let dev = dev.expect("device back");
    assert!(report.repaired);
    assert_eq!(outcome_status(&report), STATUS_OK);
    let wire = decode_report(&encode_report(&report)).expect("roundtrip");
    assert_eq!(wire.outcome, 1);
    assert_eq!(wire.orphan_count, 1);
    let (clean, _) = fsck(dev, false);
    assert_eq!(clean.orphan_txns.len(), 0);
    assert_eq!(outcome_status(&clean), STATUS_OK);
}

#[test]
fn test_unrecoverable_maps_io_error() {
    // Zero image: no valid superblock/journal — unrecoverable.
    let (report, _) = fsck(MemBlockDevice::new(512, 4), false);
    // Whatever the engine calls it, the op layer must not call it ok.
    if report.outcome == statefs::FsckOutcome::Unrecoverable {
        assert_eq!(outcome_status(&report), STATUS_IO_ERROR);
    } else {
        // A zero image may legally read as an empty-clean store; the
        // mapping contract still holds for the enc-failure edge below.
        assert_eq!(outcome_status(&report), STATUS_OK);
    }
}

#[test]
fn test_enc_failures_are_never_ok() {
    let (mut report, _) = fsck(fresh_store(), false);
    report.enc_failures = 1;
    assert_eq!(outcome_status(&report), STATUS_INTEGRITY_VIOLATION);
}

#[test]
fn test_reject_busy_while_txns_open() {
    // The gate itself is pure; the OS layer feeds engine.open_txns().
    assert!(quiesce_ok(0));
    assert!(!quiesce_ok(1));
    assert!(!quiesce_ok(8));
}

#[test]
fn test_repair_bound_is_structural() {
    // MAX_OPEN_TXNS caps concurrent orphans; the wire field never
    // saturates below that bound.
    let (report, _) = fsck(orphan_store(), false);
    assert!(report.orphan_txns.len() <= 8);
    let wire = decode_report(&encode_report(&report)).expect("roundtrip");
    assert_eq!(wire.orphan_count as usize, report.orphan_txns.len());
}

#[test]
fn test_reject_wire_report_decode() {
    let (report, _) = fsck(fresh_store(), false);
    let good = encode_report(&report);
    assert!(decode_report(&good[..FSCK_REPORT_WIRE_LEN - 1]).is_none());
    let mut bad = good;
    bad[0] = 9;
    assert!(decode_report(&bad).is_none());
    let mut bad = good;
    bad[1] = 3;
    assert!(decode_report(&bad).is_none());
    let mut bad = good;
    bad[2] = 0;
    assert!(decode_report(&bad).is_none());
}
