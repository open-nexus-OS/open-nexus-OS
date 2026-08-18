// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS journal v2 crash-injection proof (TASK-0026): every
//! block-write prefix of a transaction must replay to EITHER the pre-txn
//! state OR the post-txn state — never a torn hybrid. Also proves the v1→v2
//! compaction upgrade, threshold-driven rotation, incremental replay after
//! compaction (op-count bounded, independent of lifetime writes), and the
//! documented reject/ignore contract for malformed v2 records.
//! Harness shape mirrors userspace/nxfs/tests/crash_injection.rs (SpyDevice).
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (deterministic, no randomness)

use statefs::journal_v2::{
    encode_commit, encode_payload, encode_prepare, encode_record, JournalOpCode, MAX_OPEN_TXNS,
    MAX_TXN_CHUNK, MAX_TXN_KEYS,
};
use statefs::{CompactionConfig, JournalEngine, StatefsError};
use storage::{BlockDevice, BlockError, MemBlockDevice};

const BLOCK_SIZE: usize = 512;
const BLOCKS: u64 = 64;

/// Records every write in order so tests can replay arbitrary prefixes onto
/// a snapshot (block-granular crash simulation).
struct SpyDevice {
    inner: MemBlockDevice,
    log: Vec<(u64, Vec<u8>)>,
}

impl SpyDevice {
    fn new(inner: MemBlockDevice) -> Self {
        Self { inner, log: Vec::new() }
    }
}

impl BlockDevice for SpyDevice {
    fn block_size(&self) -> usize {
        self.inner.block_size()
    }
    fn block_count(&self) -> u64 {
        self.inner.block_count()
    }
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_block(block_idx, buf)
    }
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.log.push((block_idx, buf.to_vec()));
        self.inner.write_block(block_idx, buf)
    }
    fn sync(&mut self) -> Result<(), BlockError> {
        self.inner.sync()
    }
}

fn snapshot(device: &MemBlockDevice) -> MemBlockDevice {
    let mut copy = MemBlockDevice::new(device.block_size(), device.block_count());
    let mut buf = vec![0u8; device.block_size()];
    for idx in 0..device.block_count() {
        device.read_block(idx, &mut buf).expect("read");
        copy.write_block(idx, &buf).expect("write");
    }
    copy
}

/// Full observable state fingerprint: every key with its value.
fn fingerprint(device: MemBlockDevice) -> (Vec<(String, Vec<u8>)>, MemBlockDevice) {
    let engine = JournalEngine::open(device).expect("open");
    let mut out = Vec::new();
    for key in engine.list("/state/", 1024).expect("list") {
        out.push((key.clone(), engine.get(&key).expect("get")));
    }
    (out, engine.into_device())
}

/// Base image: a legacy v1 journal with pre-existing committed state.
fn base_image() -> MemBlockDevice {
    let device = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    let mut engine = JournalEngine::open(device).expect("open");
    engine.put("/state/test/keep", b"stable content").expect("put");
    engine.put("/state/boot/slot", b"A").expect("put");
    engine.sync().expect("sync");
    engine.into_device()
}

/// Append raw record bytes at the write head of a legacy-layout device
/// (fixture injection for records the engine API refuses to write).
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
// Host DoD: happy path / crash sim / multi-key both-or-neither
// ============================================================================

#[test]
fn txn_happy_path_visible_after_replay() {
    let mut engine = JournalEngine::open(base_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/a", b"alpha").expect("append");
    engine.txn_append(txn, "/state/test/b", b"beta").expect("append");
    engine.txn_commit(txn).expect("commit");
    // Visible immediately...
    assert_eq!(engine.get("/state/test/a").expect("get"), b"alpha");
    // ...and after a full replay.
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/a").expect("get"), b"alpha");
    assert_eq!(reopened.get("/state/test/b").expect("get"), b"beta");
    assert_eq!(reopened.get("/state/test/keep").expect("get"), b"stable content");
}

#[test]
fn chunked_payloads_concatenate_in_order() {
    let mut engine = JournalEngine::open(base_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/big", b"part-one|").expect("append");
    engine.txn_append(txn, "/state/test/big", b"part-two").expect("append");
    engine.txn_commit(txn).expect("commit");
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/big").expect("get"), b"part-one|part-two");
}

#[test]
fn prepared_without_commit_not_visible() {
    let mut engine = JournalEngine::open(base_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/torn", b"never committed").expect("append");
    // Crash: no COMMIT record. Replay must discard the prepared txn.
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/torn"), Err(StatefsError::NotFound));
    assert_eq!(reopened.get("/state/test/keep").expect("get"), b"stable content");
    assert!(reopened.replay_orphans() > 0, "discarded txn is an observable orphan");
}

#[test]
fn txn_abort_discards_buffered_state() {
    let mut engine = JournalEngine::open(base_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/aborted", b"gone").expect("append");
    engine.txn_abort(txn).expect("abort");
    assert_eq!(engine.get("/state/test/aborted"), Err(StatefsError::NotFound));
    assert_eq!(engine.open_txns(), 0);
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/aborted"), Err(StatefsError::NotFound));
}

/// Multi-key txn: both-or-neither across restart at EVERY record boundary
/// (block-write granularity, which subsumes record boundaries).
#[test]
fn multi_key_txn_both_or_neither_at_every_cut() {
    let base = base_image();
    let (pre_state, base) = fingerprint(base);

    // Run the txn fully on a spy to capture the write log + post state.
    let spy = SpyDevice::new(snapshot(&base));
    let mut engine = JournalEngine::open(spy).expect("open spy");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/boot/slot", b"B").expect("append");
    engine.txn_append(txn, "/state/boot/tries", b"\x03").expect("append");
    engine.txn_commit(txn).expect("commit");
    engine.sync_barrier().expect("sync");
    let spy = engine.into_device();
    let log = spy.log;
    let (post_state, _) = fingerprint(spy.inner);

    assert_ne!(pre_state, post_state);
    for cut in 0..=log.len() {
        let mut image = snapshot(&base);
        for (idx, data) in &log[..cut] {
            image.write_block(*idx, data).expect("replay write");
        }
        let (state, _) = fingerprint(image);
        let is_pre = state == pre_state;
        let is_post = state == post_state;
        assert!(is_pre || is_post, "cut={cut}/{}: torn state visible: {state:?}", log.len());
    }
}

#[test]
fn replay_is_idempotent() {
    let mut engine = JournalEngine::open(base_image()).expect("open");
    let txn = engine.txn_begin().expect("begin");
    engine.txn_append(txn, "/state/test/x", b"committed").expect("append");
    engine.txn_commit(txn).expect("commit");
    let orphan = engine.txn_begin().expect("begin orphan");
    engine.txn_append(orphan, "/state/test/y", b"torn").expect("append");
    let device = engine.into_device();

    let (state_a, device) = fingerprint(device);
    let (state_b, device) = fingerprint(device);
    assert_eq!(state_a, state_b, "same journal replayed twice = same state");

    // In-place reopen twice agrees as well.
    let mut engine = JournalEngine::open(device).expect("open");
    engine.reopen().expect("reopen");
    let scanned_first = engine.replayed_records();
    engine.reopen().expect("reopen twice");
    assert_eq!(engine.replayed_records(), scanned_first);
    assert_eq!(engine.get("/state/test/x").expect("get"), b"committed");
    assert_eq!(engine.get("/state/test/y"), Err(StatefsError::NotFound));
}

// ============================================================================
// Host DoD: v1 → v2 upgrade, compaction threshold, incremental replay
// ============================================================================

#[test]
fn v1_journal_upgrade_compact_state_identical() {
    // A legacy engine writes plain v1 records (byte-identical journal).
    let (pre_state, device) = fingerprint(base_image());

    let mut engine = JournalEngine::open(device).expect("open v1");
    assert_eq!(engine.generation(), 0, "legacy journal is generation 0");
    let stats = engine.compact_now().expect("compact");
    assert_eq!(stats.generation, 1, "first compaction writes a v2-generation journal");
    assert_eq!(stats.entries, pre_state.len());

    // State identical right away and across a fresh replay.
    let (post_compact, device) = fingerprint(engine.into_device());
    assert_eq!(post_compact, pre_state);
    let engine = JournalEngine::open(device).expect("open v2");
    assert_eq!(engine.generation(), 1);

    // The rotated journal keeps accepting writes and replaying clean.
    let mut engine = engine;
    engine.put("/state/test/after", b"post-compact write").expect("put");
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/after").expect("get"), b"post-compact write");
    assert_eq!(reopened.get("/state/test/keep").expect("get"), b"stable content");
}

#[test]
fn compaction_threshold_triggers_snapshot_rotate() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_compaction_config(CompactionConfig {
        min_journal_bytes: 512,
        ratio: 2,
        max_entries_per_cycle: 64,
    });
    // Below threshold: no cycle runs.
    engine.put("/state/test/hot", b"v0").expect("put");
    assert_eq!(engine.maybe_compact().expect("maybe"), None);

    // Overwrite churn = garbage; threshold must eventually fire.
    let mut fired = None;
    for i in 0..200 {
        let value = format!("value-{i}");
        engine.put("/state/test/hot", value.as_bytes()).expect("put");
        if let Some(stats) = engine.maybe_compact().expect("maybe") {
            fired = Some(stats);
            break;
        }
    }
    let stats = fired.expect("threshold fired");
    // Bounded cycle work is observable: one live entry + checkpoint.
    assert_eq!(stats.entries, 1);
    assert!(stats.bytes_written < 512, "snapshot is minimal, not lifetime-sized");
    assert!(engine.journal_bytes() == stats.bytes_written);

    // State intact after rotation + replay.
    let value = engine.get("/state/test/hot").expect("get");
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/hot").expect("get"), value);
    assert_eq!(reopened.generation(), stats.generation);
}

#[test]
fn compaction_defers_while_txn_open() {
    let mut engine = JournalEngine::open(base_image()).expect("open");
    engine.set_compaction_config(CompactionConfig {
        min_journal_bytes: 1,
        ratio: 0,
        max_entries_per_cycle: 64,
    });
    let txn = engine.txn_begin().expect("begin");
    assert_eq!(engine.maybe_compact().expect("maybe"), None, "deferred: open txn");
    assert_eq!(engine.compact_now(), Err(StatefsError::IoError));
    engine.txn_abort(txn).expect("abort");
    assert!(engine.maybe_compact().expect("maybe").is_some(), "runs once txns are closed");
}

/// Incremental replay: records scanned at open is bounded by live state +
/// appends since the last compaction — independent of lifetime writes.
#[test]
fn incremental_replay_op_count_bounded() {
    fn scanned_after_cycles(cycles: usize) -> (usize, MemBlockDevice) {
        let mut engine =
            JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
        for i in 0..cycles {
            for j in 0..10 {
                let value = format!("cycle-{i}-{j}");
                engine.put("/state/test/a", value.as_bytes()).expect("put");
                engine.put("/state/test/b", value.as_bytes()).expect("put");
            }
            engine.compact_now().expect("compact");
        }
        let engine = JournalEngine::open(engine.into_device()).expect("reopen");
        (engine.replayed_records(), engine.into_device())
    }

    let (scanned_short, _) = scanned_after_cycles(3);
    let (scanned_long, device) = scanned_after_cycles(9);
    // 2 live entries + 1 checkpoint record, regardless of cycle count:
    // lifetime writes (60 vs 180 puts) do not appear in the replay cost, so
    // MAX_REPLAY_RECORDS is unreachable in normal operation.
    assert_eq!(scanned_short, 3);
    assert_eq!(scanned_long, 3);

    // And state is the last committed values.
    let engine = JournalEngine::open(device).expect("open");
    assert_eq!(engine.get("/state/test/a").expect("get"), b"cycle-8-9");
}

// ============================================================================
// Host DoD: test_reject_* negatives (documented reject/ignore contract)
// ============================================================================

/// Truncated PAYLOAD at the tail: replay stops at the torn record; committed
/// state before it is intact; the open txn is discarded. No panic.
#[test]
fn test_reject_truncated_payload_tail() {
    let engine = JournalEngine::open(base_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    let prepare = encode_prepare(500);
    let payload = encode_payload(500, "/state/test/torn", b"payload-bytes").expect("encode");
    let head = append_raw(&mut device, head, &prepare);
    // Cut the PAYLOAD record short at every possible boundary.
    for cut in 1..payload.len() {
        let mut image = snapshot(&device);
        append_raw(&mut image, head, &payload[..cut]);
        let engine = JournalEngine::open(image).expect("open");
        assert_eq!(engine.get("/state/test/torn"), Err(StatefsError::NotFound), "cut={cut}");
        assert_eq!(engine.get("/state/test/keep").expect("get"), b"stable content");
    }
}

/// COMMIT for a transaction that was never prepared: deterministically
/// ignored (orphan), replay continues, later records still apply.
#[test]
fn test_reject_unknown_txn_commit_ignored() {
    let engine = JournalEngine::open(base_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    let head = append_raw(&mut device, head, &encode_commit(999));
    let follow = encode_record(JournalOpCode::Put, "/state/test/after", b"still applies");
    append_raw(&mut device, head, &follow);

    let engine = JournalEngine::open(device).expect("open");
    assert!(engine.replay_orphans() > 0, "unknown COMMIT counts as orphan");
    assert_eq!(engine.get("/state/test/after").expect("get"), b"still applies");
    assert_eq!(engine.get("/state/test/keep").expect("get"), b"stable content");
}

/// PAYLOAD whose value claims a chunk larger than MAX_TXN_CHUNK: structurally
/// invalid shape — replay stops there (v1 stop-at-first-corruption rule);
/// everything before is intact. The writer API refuses to produce it.
#[test]
fn test_reject_oversized_chunk_record() {
    assert_eq!(
        encode_payload(1, "/state/test/k", &vec![0u8; MAX_TXN_CHUNK + 1]),
        Err(StatefsError::ValueTooLarge)
    );

    let engine = JournalEngine::open(base_image()).expect("open");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    // Hand-build the forbidden record: txn_id ++ oversized chunk.
    let mut value = 7u64.to_le_bytes().to_vec();
    value.extend_from_slice(&vec![0xAB; MAX_TXN_CHUNK + 1]);
    let bad = encode_record(JournalOpCode::TxnPayload, "/state/test/k", &value);
    let head = append_raw(&mut device, head, &encode_prepare(7));
    let head = append_raw(&mut device, head, &bad);
    append_raw(&mut device, head, &encode_commit(7));

    let engine = JournalEngine::open(device).expect("open");
    assert_eq!(engine.get("/state/test/k"), Err(StatefsError::NotFound));
    assert_eq!(engine.get("/state/test/keep").expect("get"), b"stable content");
}

/// Writer-side caps are hard errors; the store stays consistent.
#[test]
fn test_reject_txn_caps_writer_side() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, 4096)).expect("open");
    // Open-txn count cap.
    let ids: Vec<u64> = (0..MAX_OPEN_TXNS).map(|_| engine.txn_begin().expect("begin")).collect();
    assert_eq!(engine.txn_begin(), Err(StatefsError::IoError));
    for id in ids {
        engine.txn_abort(id).expect("abort");
    }
    // Per-txn key cap.
    let txn = engine.txn_begin().expect("begin");
    for i in 0..MAX_TXN_KEYS {
        let key = format!("/state/test/cap{i}");
        engine.txn_append(txn, &key, b"x").expect("append");
    }
    assert_eq!(
        engine.txn_append(txn, "/state/test/overflow", b"x"),
        Err(StatefsError::ValueTooLarge)
    );
    // Unknown txn id.
    assert_eq!(engine.txn_append(4242, "/state/test/k", b"x"), Err(StatefsError::NotFound));
    assert_eq!(engine.txn_commit(4242), Err(StatefsError::NotFound));
    assert_eq!(engine.txn_abort(4242), Err(StatefsError::NotFound));
    // The capped txn still commits its accepted entries atomically.
    engine.txn_commit(txn).expect("commit");
    let reopened = JournalEngine::open(engine.into_device()).expect("reopen");
    assert_eq!(reopened.get("/state/test/cap0").expect("get"), b"x");
    assert_eq!(reopened.get("/state/test/overflow"), Err(StatefsError::NotFound));
}

/// Replay-side cap violation (a journal only a buggy/hostile writer could
/// produce): the transaction is poisoned — nothing of it applies, replay
/// continues, no panic.
#[test]
fn test_reject_txn_buffer_cap_replay_poisons() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, 4096)).expect("open");
    engine.put("/state/test/keep", b"stable content").expect("put");
    let head = engine.journal_bytes();
    let mut device = engine.into_device();
    // PREPARE + one key too many + COMMIT.
    let mut head = append_raw(&mut device, head, &encode_prepare(11));
    for i in 0..=MAX_TXN_KEYS {
        let key = format!("/state/test/poison{i}");
        let record = encode_payload(11, &key, b"x").expect("encode");
        head = append_raw(&mut device, head, &record);
    }
    let head_after = append_raw(&mut device, head, &encode_commit(11));
    let follow = encode_record(JournalOpCode::Put, "/state/test/after", b"applies");
    append_raw(&mut device, head_after, &follow);

    let engine = JournalEngine::open(device).expect("open");
    assert!(engine.replay_orphans() > 0);
    for i in 0..=MAX_TXN_KEYS {
        let key = format!("/state/test/poison{i}");
        assert_eq!(engine.get(&key), Err(StatefsError::NotFound), "poisoned txn never applies");
    }
    assert_eq!(engine.get("/state/test/after").expect("get"), b"applies");
    assert_eq!(engine.get("/state/test/keep").expect("get"), b"stable content");
}

/// Wraps a device and, once armed, corrupts every read of one target block
/// — models a device that acked writes but persisted garbage, exactly what
/// the post-compaction readback verify exists to catch (no-fake-green).
struct TamperDevice {
    inner: MemBlockDevice,
    tamper_block: std::rc::Rc<core::cell::Cell<Option<u64>>>,
}

impl BlockDevice for TamperDevice {
    fn block_size(&self) -> usize {
        self.inner.block_size()
    }
    fn block_count(&self) -> u64 {
        self.inner.block_count()
    }
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_block(block_idx, buf)?;
        if self.tamper_block.get() == Some(block_idx) {
            buf[0] ^= 0xFF;
        }
        Ok(())
    }
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.inner.write_block(block_idx, buf)
    }
    fn sync(&mut self) -> Result<(), BlockError> {
        self.inner.sync()
    }
}

#[test]
fn test_reject_compaction_verify_detects_tampered_readback() {
    let tamper = std::rc::Rc::new(core::cell::Cell::new(None));
    let device = TamperDevice {
        inner: MemBlockDevice::new(BLOCK_SIZE, BLOCKS),
        tamper_block: tamper.clone(),
    };
    let mut engine = JournalEngine::open(device).expect("open");
    engine.set_compaction_config(CompactionConfig {
        min_journal_bytes: 1024,
        ratio: 2,
        max_entries_per_cycle: 128,
    });
    for round in 0..50u8 {
        engine.put("/state/test/churn", &[round; 64]).expect("churn put");
    }
    let stats = engine.maybe_compact().expect("compact").expect("cycle ran");
    // Clean readback: the cycle verifies.
    assert!(engine.verify_last_compaction(&stats));
    // Wrong stats must never verify (generation or entry-count drift).
    let mut wrong = stats;
    wrong.entries += 1;
    assert!(!engine.verify_last_compaction(&wrong));
    // Tampered snapshot readback (first block of the fresh region, i.e.
    // region B after the first compaction on a 64-block device): the done
    // marker must be withheld.
    tamper.set(Some(BLOCKS / 2));
    assert!(!engine.verify_last_compaction(&stats));
    // And a tampered superblock readback fails as well.
    tamper.set(Some(0));
    assert!(!engine.verify_last_compaction(&stats));
    tamper.set(None);
    assert!(engine.verify_last_compaction(&stats));
}
