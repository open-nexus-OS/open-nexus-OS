// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: fsck-statefs outcome matrix (TASK-0026): clean v1/v2 journals,
//! orphan detection + append-only ABORT repair (repaired journal re-replays
//! clean), structural corruption -> Unrecoverable with stable location +
//! reason, zeroed-tail reporting, and determinism. Fixtures are built via
//! the engine API (crash_injection.rs discipline); raw bytes only where the
//! corruption itself is the fixture.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (deterministic, no randomness)

use statefs::journal_v2::{encode_checkpoint, encode_commit, encode_prepare, encode_record};
use statefs::{fsck, FsckOutcome, JournalEngine, JournalLayout, JournalOpCode, StatefsError};
use storage::{BlockDevice, MemBlockDevice};

const BLOCK_SIZE: usize = 512;
const BLOCKS: u64 = 64;

/// Legacy v1 journal with two committed keys.
fn v1_image() -> MemBlockDevice {
    let device = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    let mut engine = JournalEngine::open(device).expect("open");
    engine.put("/state/test/keep", b"stable content").expect("put");
    engine.put("/state/boot/slot", b"A").expect("put");
    engine.sync().expect("sync");
    engine.into_device()
}

/// Compacted v2 journal (generation 1) with the same two keys.
fn v2_image() -> MemBlockDevice {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    engine.compact_now().expect("compact");
    engine.into_device()
}

/// A v1 journal whose tail holds an orphaned transaction (PREPARE + PAYLOAD
/// without COMMIT/ABORT) — built via the engine API, crash-style.
fn orphan_image() -> (MemBlockDevice, u64) {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/torn", b"never committed").expect("append");
    (engine.into_device(), txn)
}

/// Write raw record bytes at `head` of a legacy-layout device (fixture
/// injection for records the engine API refuses to write).
fn append_raw(device: &mut MemBlockDevice, head: usize, bytes: &[u8]) -> usize {
    let block_size = device.block_size();
    let mut buf = vec![0u8; block_size];
    for (i, &byte) in bytes.iter().enumerate() {
        let pos = head + i;
        let block = (pos / block_size) as u64;
        device.read_block(block, &mut buf).expect("read");
        buf[pos % block_size] = byte;
        device.write_block(block, &buf).expect("write");
    }
    head + bytes.len()
}

// ============================================================================
// Clean journals
// ============================================================================

#[test]
fn clean_v1_journal_is_clean() {
    let (report, device) = fsck(v1_image(), false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    assert_eq!(report.layout, JournalLayout::V1);
    assert_eq!(report.generation, 0);
    assert_eq!(report.records, 2);
    assert_eq!(report.entries, 2);
    assert!(report.orphan_txns.is_empty());
    assert_eq!(report.anomalies, 0);
    assert!(!report.tail_dirty);
    assert!(!report.repaired);
    assert!(report.fault.is_none());
    assert!(device.is_some());
}

#[test]
fn clean_v2_journal_is_clean() {
    let (report, _) = fsck(v2_image(), false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    assert_eq!(report.layout, JournalLayout::V2);
    assert_eq!(report.generation, 1);
    // Snapshot = CHECKPOINT + one Put per live key.
    assert_eq!(report.records, 3);
    assert_eq!(report.entries, 2);
    assert!(report.orphan_txns.is_empty());
    assert_eq!(report.anomalies, 0);
}

#[test]
fn committed_txn_is_clean() {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/a", b"alpha").expect("append");
    engine.txn_commit(txn).expect("commit");
    let (report, _) = fsck(engine.into_device(), false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    assert!(report.orphan_txns.is_empty());
    assert_eq!(report.entries, 3);
}

#[test]
fn fsck_report_is_deterministic() {
    let (first, device) = fsck(orphan_image().0, false);
    let (second, _) = fsck(device.expect("device"), false);
    assert_eq!(first, second, "same journal fsck'd twice = same report");
}

// ============================================================================
// Orphan detection + repair
// ============================================================================

#[test]
fn orphan_detected_without_repair() {
    let (device, txn) = orphan_image();
    let (report, device) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Repaired);
    assert!(!report.repaired, "without --repair the orphan is only reported");
    assert_eq!(report.orphan_txns, vec![txn]);
    assert!(device.is_some(), "device returned untouched");
}

#[test]
fn orphan_repaired_and_journal_replays_clean() {
    let (device, txn) = orphan_image();
    let (report, device) = fsck(device, true);
    assert_eq!(report.outcome, FsckOutcome::Repaired);
    assert!(report.repaired, "repair re-validated clean");
    assert_eq!(report.orphan_txns, vec![txn]);

    // Second fsck: clean — the orphan is gone.
    let (again, device) = fsck(device.expect("device"), false);
    assert_eq!(again.outcome, FsckOutcome::Clean);
    assert!(again.orphan_txns.is_empty());
    assert_eq!(again.anomalies, 0);

    // And the engine agrees: committed state intact, orphan invisible.
    let engine = JournalEngine::open(device.expect("device")).expect("open");
    assert_eq!(engine.replay_orphans(), 0, "ABORT retired the orphan");
    assert_eq!(engine.get("/state/test/torn"), Err(StatefsError::NotFound));
    assert_eq!(engine.get("/state/test/keep").expect("get"), b"stable content");
    assert_eq!(engine.get("/state/boot/slot").expect("get"), b"A");
}

#[test]
fn multiple_orphans_all_repaired_in_journal_order() {
    let mut engine = JournalEngine::open(v1_image()).expect("open");
    let first = engine.txn_begin().expect("begin");
    let second = engine.txn_begin().expect("begin");
    engine.txn_append(first, "/state/test/one", b"1").expect("append");
    engine.txn_append(second, "/state/test/two", b"2").expect("append");
    let (report, device) = fsck(engine.into_device(), true);
    assert_eq!(report.orphan_txns, vec![first, second]);
    assert!(report.repaired);
    let (again, _) = fsck(device.expect("device"), false);
    assert_eq!(again.outcome, FsckOutcome::Clean);
}

#[test]
fn orphan_in_v2_journal_repaired() {
    let mut engine = JournalEngine::open(v2_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/torn", b"gone").expect("append");
    let (report, device) = fsck(engine.into_device(), true);
    assert_eq!(report.layout, JournalLayout::V2);
    assert_eq!(report.orphan_txns, vec![txn]);
    assert!(report.repaired);
    let (again, _) = fsck(device.expect("device"), false);
    assert_eq!(again.outcome, FsckOutcome::Clean);
}

// ============================================================================
// Inert anomalies + zeroed-tail reporting (informational, never fatal)
// ============================================================================

#[test]
fn unknown_txn_commit_is_inert_anomaly_not_orphan() {
    let engine = JournalEngine::open(v1_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    append_raw(&mut device, head, &encode_commit(999));
    let (report, _) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Clean, "inert anomalies do not change the outcome");
    assert!(report.orphan_txns.is_empty());
    assert_eq!(report.anomalies, 1);
}

#[test]
fn checkpoint_in_v1_journal_is_reported_anomaly() {
    let engine = JournalEngine::open(v1_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    // v1 journals never legitimately contain CHECKPOINT; replay resets state.
    append_raw(&mut device, head, &encode_checkpoint(1, 0));
    let (report, _) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    assert_eq!(report.anomalies, 1);
    assert_eq!(report.entries, 0, "replay applied the state reset");
}

#[test]
fn torn_tail_is_reported_dirty_not_fatal() {
    let engine = JournalEngine::open(v1_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    // Crash residue: a partial PREPARE record at the write head.
    let prepare = encode_prepare(7);
    append_raw(&mut device, head, &prepare[..prepare.len() - 4]);
    let (report, _) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    assert!(report.tail_dirty, "torn tail bytes are reported");
    assert_eq!(report.records, 2, "replay stopped at the torn record");
}

// ============================================================================
// test_reject_*: unrecoverable structural violations (location + reason)
// ============================================================================

#[test]
fn test_reject_corrupted_crc_mid_journal() {
    let device = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    let mut engine = JournalEngine::open(device).expect("open");
    engine.put("/state/test/first", b"one").expect("put");
    engine.put("/state/test/second", b"two").expect("put");
    engine.put("/state/test/third", b"three").expect("put");
    let mut device = engine.into_device();

    // Flip a key byte inside record 2: CRC no longer matches.
    let r1 = encode_record(JournalOpCode::Put, "/state/test/first", b"one");
    let corrupt_at = r1.len() + 15 + 2; // header(15) + 2 bytes into the key
    let block = corrupt_at / BLOCK_SIZE;
    device.raw_storage_mut()[block][corrupt_at % BLOCK_SIZE] ^= 0xFF;

    let (report, device) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
    let f = report.fault.expect("fault");
    assert_eq!(f.offset, r1.len() as u64, "fault located at the corrupted record");
    assert_eq!(f.reason, "crc mismatch");
    assert!(device.is_none());

    // --repair must not pretend: structural damage stays unrecoverable.
    let device2 = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    let mut engine = JournalEngine::open(device2).expect("open");
    engine.put("/state/test/first", b"one").expect("put");
    engine.put("/state/test/second", b"two").expect("put");
    let mut device2 = engine.into_device();
    device2.raw_storage_mut()[0][17] ^= 0xFF;
    let (report, _) = fsck(device2, true);
    assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
    assert!(!report.repaired);
}

#[test]
fn corrupted_record_at_tail_is_discarded_residue_not_fatal() {
    // The same CRC damage with NOTHING valid after it is indistinguishable
    // from a torn append: replay discards it; fsck reports a dirty tail.
    let device = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    let mut engine = JournalEngine::open(device).expect("open");
    engine.put("/state/test/keep", b"stable content").expect("put");
    engine.put("/state/test/last", b"damaged").expect("put");
    let mut device = engine.into_device();
    let r1 = encode_record(JournalOpCode::Put, "/state/test/keep", b"stable content");
    let corrupt_at = r1.len() + 15 + 2; // 2 bytes into the LAST record's key
    device.raw_storage_mut()[corrupt_at / BLOCK_SIZE][corrupt_at % BLOCK_SIZE] ^= 0xFF;
    let (report, _) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    assert!(report.tail_dirty);
    assert_eq!(report.records, 1, "replay keeps everything before the damaged tail");
    assert_eq!(report.entries, 1);
}

#[test]
fn test_reject_truncated_superblock() {
    let mut device = v2_image();
    // Keep the NXS2 magic, wipe the rest of the superblock (truncation-style
    // damage): the superblock no longer validates.
    for byte in device.raw_storage_mut()[0][4..20].iter_mut() {
        *byte = 0;
    }
    let (report, device) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
    let f = report.fault.expect("fault");
    assert_eq!(f.offset, 0);
    assert_eq!(f.reason, "invalid superblock");
    assert!(device.is_none());
}

#[test]
fn test_reject_missing_checkpoint_at_snapshot_start() {
    let mut device = v2_image();
    // First compaction of a legacy journal activates region B; with 64
    // blocks the geometry is region_blocks=31, A=1..31, B=32..62.
    let region_first = 32usize;
    device.raw_storage_mut()[region_first].fill(0);
    let (report, _) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
    let f = report.fault.expect("fault");
    assert_eq!(f.offset, (region_first * BLOCK_SIZE) as u64);
    assert_eq!(f.reason, "missing checkpoint at snapshot start");
}

#[test]
fn test_reject_unknown_opcode_mid_journal() {
    let engine = JournalEngine::open(v1_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    let mut bad = encode_record(JournalOpCode::Put, "/state/test/x", b"v");
    bad[4] = 0xEE; // not a JournalOpCode
    let after_bad = append_raw(&mut device, head, &bad);
    // A valid record AFTER the damage makes it mid-journal (fatal), not
    // discarded tail residue.
    append_raw(&mut device, after_bad, &encode_record(JournalOpCode::Put, "/state/test/y", b"w"));
    let (report, _) = fsck(device, false);
    assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
    let f = report.fault.expect("fault");
    assert_eq!(f.offset, head as u64);
    assert_eq!(f.reason, "unknown opcode");
}
