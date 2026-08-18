// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS record-encryption engine proof (TASK-0027): sealed at
//! rest (plaintext never hits the medium), replay-side AEAD verify (a
//! tampered plain put is skipped, a tampered txn chunk discards its WHOLE
//! transaction), compaction snapshots stay decryptable, nonce ids stay
//! monotonic across reopen, the single-chunk rule for enrolled txn values,
//! and the legacy-plaintext migration window. Split out of
//! crash_injection.rs (structure ratchet); same fixture discipline.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (deterministic, no randomness)

use statefs::{CompactionConfig, JournalEngine, StatefsError};
use storage::{BlockDevice, MemBlockDevice};

const BLOCK_SIZE: usize = 512;
const BLOCKS: u64 = 64;

fn enc_ctx() -> statefs::enc::EncContext {
    let mut ctx = statefs::enc::EncContext::new([3u8; statefs::enc::SALT_LEN]);
    let key = statefs::enc::record_key_from_ikm(b"crash-injection-ikm", "app").unwrap();
    let class = ctx.add_class("app", &key).unwrap();
    ctx.enroll("/state/app/", class).unwrap();
    ctx
}

fn device_image(device: &MemBlockDevice) -> Vec<u8> {
    let mut image = vec![0u8; device.block_size() * device.block_count() as usize];
    let mut buf = vec![0u8; device.block_size()];
    for idx in 0..device.block_count() {
        device.read_block(idx, &mut buf).expect("read");
        let start = idx as usize * device.block_size();
        image[start..start + device.block_size()].copy_from_slice(&buf);
    }
    image
}

fn write_image(device: &mut MemBlockDevice, image: &[u8]) {
    let bs = device.block_size();
    for idx in 0..device.block_count() {
        let start = idx as usize * bs;
        device.write_block(idx, &image[start..start + bs]).expect("write");
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Flip one ciphertext byte of the sealed value at `sealed_pos` and repair
/// the enclosing record's CRC (a disk attacker can recompute CRCs — only
/// the AEAD may catch this). `value_prefix` = bytes between the record's
/// value start and the sealed blob (8-byte txn id for TxnPayload records).
fn tamper_sealed_at(image: &mut [u8], sealed_pos: usize, key: &str, value_prefix: usize) {
    // Flip the first ciphertext byte (right past the 20-byte sealed header).
    image[sealed_pos + statefs::enc::SEALED_OVERHEAD - 16] ^= 0x01;
    // Record layout: NXSF(4) op(1) keylen(2) vallen(4) key value crc(4) —
    // the 15-byte RECORD_HEADER_SIZE counts the trailing CRC, so the value
    // begins 11 + keylen bytes after the record start.
    let rec_start = sealed_pos - value_prefix - key.len() - 11;
    let value_len = u32::from_le_bytes([
        image[rec_start + 7],
        image[rec_start + 8],
        image[rec_start + 9],
        image[rec_start + 10],
    ]) as usize;
    let total = 15 + key.len() + value_len;
    let crc = statefs::crc32c(&image[rec_start..rec_start + total - 4]);
    image[rec_start + total - 4..rec_start + total].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn test_enc_put_seals_at_rest_and_roundtrips() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_enc_context(enc_ctx());
    engine.put("/state/app/enc/token", b"secret-plaintext-v1").expect("put");
    // Wire-visible: plaintext out, sealed in memory and on the medium.
    assert_eq!(engine.get("/state/app/enc/token").expect("get"), b"secret-plaintext-v1");
    let device = engine.into_device();
    let image = device_image(&device);
    assert!(find(&image, b"secret-plaintext-v1").is_none(), "plaintext must not hit the medium");
    assert!(find(&image, &statefs::enc::SEALED_MAGIC).is_some(), "sealed blob on the medium");
    // A key-less open (pre-derive boot window) serves ciphertext, never
    // silently-wrong plaintext; installing the context opens it again.
    let mut engine = JournalEngine::open(device).expect("reopen");
    let raw = engine.get("/state/app/enc/token").expect("raw get");
    assert!(statefs::enc::is_sealed(&raw));
    engine.set_enc_context(enc_ctx());
    assert_eq!(engine.get("/state/app/enc/token").expect("get"), b"secret-plaintext-v1");
}

#[test]
fn test_reject_tampered_put_skipped_on_replay_and_get() {
    let key = "/state/app/enc/tamper";
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_enc_context(enc_ctx());
    engine.put(key, b"attacker-target").expect("put");
    engine.sync().expect("sync");
    let device = engine.into_device();
    let mut image = device_image(&device);
    let sealed_pos = find(&image, &statefs::enc::SEALED_MAGIC).expect("sealed");
    tamper_sealed_at(&mut image, sealed_pos, key, 0);
    let mut device = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    write_image(&mut device, &image);
    // get-side: CRC is fine, AEAD is not — never plaintext, never silence.
    let mut engine = JournalEngine::open(device).expect("open tampered");
    engine.set_enc_context(enc_ctx());
    assert_eq!(engine.get(key), Err(StatefsError::IntegrityViolation));
    // replay-side (context present at replay): the record is rejected.
    engine.reopen().expect("reopen");
    assert_eq!(engine.get(key), Err(StatefsError::NotFound));
    assert_eq!(engine.replay_enc_rejects(), 1);
}

#[test]
fn test_reject_tampered_txn_chunk_poisons_whole_txn() {
    let key_a = "/state/app/enc/txn-a";
    let key_b = "/state/app/enc/txn-b";
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_enc_context(enc_ctx());
    let id = engine.txn_begin().expect("begin");
    engine.txn_append(id, key_a, b"txn-value-a").expect("append a");
    engine.txn_append(id, key_b, b"txn-value-b").expect("append b");
    engine.txn_commit(id).expect("commit");
    engine.sync().expect("sync");
    let device = engine.into_device();
    let mut image = device_image(&device);
    // First sealed blob = key_a's chunk (TxnPayload value = id(8) + sealed).
    let sealed_pos = find(&image, &statefs::enc::SEALED_MAGIC).expect("sealed");
    tamper_sealed_at(&mut image, sealed_pos, key_a, 8);
    let mut device = MemBlockDevice::new(BLOCK_SIZE, BLOCKS);
    write_image(&mut device, &image);
    let mut engine = JournalEngine::open(device).expect("open tampered");
    engine.set_enc_context(enc_ctx());
    engine.reopen().expect("reopen");
    // Both-or-neither: ONE tampered chunk discards the WHOLE transaction.
    assert_eq!(engine.get(key_a), Err(StatefsError::NotFound));
    assert_eq!(engine.get(key_b), Err(StatefsError::NotFound));
    assert!(engine.replay_enc_rejects() >= 1);
    assert!(engine.replay_orphans() >= 1);
}

#[test]
fn test_enc_compaction_snapshot_stays_decryptable() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_enc_context(enc_ctx());
    engine.set_compaction_config(CompactionConfig {
        min_journal_bytes: 1024,
        ratio: 2,
        max_entries_per_cycle: 128,
    });
    for round in 0..40u8 {
        engine.put("/state/app/enc/churn", &[round; 64]).expect("churn");
    }
    let stats = engine.maybe_compact().expect("compact").expect("cycle");
    assert!(engine.verify_last_compaction(&stats));
    // The snapshot copied ciphertext; a verified replay still opens it.
    engine.reopen().expect("reopen");
    assert_eq!(engine.get("/state/app/enc/churn").expect("get"), [39u8; 64]);
    assert_eq!(engine.replay_enc_rejects(), 0);
}

#[test]
fn test_enc_nonce_ids_stay_monotonic_across_reopen() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_enc_context(enc_ctx());
    engine.put("/state/app/enc/n1", b"one").expect("put");
    engine.put("/state/app/enc/n2", b"two").expect("put");
    let raw = {
        engine.set_enc_context(enc_ctx());
        let device = engine.into_device();
        let mut e = JournalEngine::open(device).expect("reopen");
        // No context installed: raw sealed bytes expose their header ids.
        let a = statefs::enc::sealed_txn_id(&e.get("/state/app/enc/n1").expect("n1")).unwrap();
        let b = statefs::enc::sealed_txn_id(&e.get("/state/app/enc/n2").expect("n2")).unwrap();
        // A fresh write after replay must consume a HIGHER id — the nonce
        // counter is re-seeded above every surviving sealed header.
        e.set_enc_context(enc_ctx());
        e.put("/state/app/enc/n3", b"three").expect("put");
        let device = e.into_device();
        let e = JournalEngine::open(device).expect("reopen2");
        let c = statefs::enc::sealed_txn_id(&e.get("/state/app/enc/n3").expect("n3")).unwrap();
        (a, b, c)
    };
    assert_ne!(raw.0, raw.1);
    assert!(raw.2 > raw.0.max(raw.1), "id after reopen must exceed all stored ids");
}

#[test]
fn test_reject_second_chunk_to_enrolled_key_in_txn() {
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.set_enc_context(enc_ctx());
    let id = engine.txn_begin().expect("begin");
    engine.txn_append(id, "/state/app/enc/one-chunk", b"first").expect("first chunk");
    // Sealing forbids concatenation: enrolled values are single-chunk.
    assert_eq!(
        engine.txn_append(id, "/state/app/enc/one-chunk", b"second"),
        Err(StatefsError::ValueTooLarge)
    );
    // Non-enrolled keys keep the v2a concatenation semantics.
    engine.txn_append(id, "/state/test/plain", b"a").expect("plain 1");
    engine.txn_append(id, "/state/test/plain", b"b").expect("plain 2");
    engine.txn_commit(id).expect("commit");
    assert_eq!(engine.get("/state/test/plain").expect("plain"), b"ab");
    assert_eq!(engine.get("/state/app/enc/one-chunk").expect("enc"), b"first");
}

#[test]
fn test_enc_legacy_plaintext_migrates_on_overwrite() {
    // Pre-enable writes stay readable (migration window, envelope
    // discipline); the first overwrite under the context seals them.
    let mut engine = JournalEngine::open(MemBlockDevice::new(BLOCK_SIZE, BLOCKS)).expect("open");
    engine.put("/state/app/enc/legacy", b"pre-enable").expect("put");
    engine.set_enc_context(enc_ctx());
    assert_eq!(engine.get("/state/app/enc/legacy").expect("get"), b"pre-enable");
    engine.reopen().expect("reopen with ctx keeps legacy plaintext");
    assert_eq!(engine.get("/state/app/enc/legacy").expect("get"), b"pre-enable");
    engine.put("/state/app/enc/legacy", b"post-enable").expect("overwrite");
    let device = engine.into_device();
    let image = device_image(&device);
    assert!(find(&image, b"post-enable").is_none(), "overwrite must be sealed");
    assert!(find(&image, b"pre-enable").is_some(), "old plaintext record remains until compaction");
}
