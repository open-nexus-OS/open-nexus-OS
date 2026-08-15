// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract tests for keystored's statefs record codecs
//!   (Integrity envelopes, TASK-0025 step 4)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Test-only
//! TEST_COVERAGE: Envelope roundtrip, legacy migration read, stale-seq
//!   rejection (server contract), malformed-envelope determinism
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use keystored::state_record;
use statefs::envelope::{SeqTracker, ENVELOPE_MAGIC};
use statefs::{writer, StatefsError};

const SEED: [u8; 32] = [0x5a; 32];

#[test]
fn test_device_key_envelope_roundtrip() {
    let sealed = state_record::seal_device_key(1, 42, &SEED).expect("seal");
    assert_eq!(&sealed[..4], &ENVELOPE_MAGIC, "sealed record carries envelope magic");
    let (seed, seq) = state_record::open_device_key(&sealed).expect("open");
    assert_eq!(seed, SEED);
    assert_eq!(seq, Some(1));
}

#[test]
fn test_device_key_legacy_raw_still_readable() {
    // Pre-migration journals hold the raw 32-byte seed; reads must keep
    // working and report "no seq" (next write starts at seq = 1).
    let (seed, seq) = state_record::open_device_key(&SEED).expect("legacy open");
    assert_eq!(seed, SEED);
    assert_eq!(seq, None);
    assert_eq!(writer::next_seq(seq), 1);
}

#[test]
fn test_reject_stale_seq_device_key_rewrite() {
    // Server contract (statefsd SeqTracker): a rewrite whose seq does not
    // exceed the max-seen one is a rollback. The client-side discipline
    // (seq = last_seen + 1) must clear it; a stale seq must not.
    let mut tracker = SeqTracker::new();
    tracker.observe(state_record::DEVICE_KEY_PATH, 3).expect("observe");
    // A writer that ignored the stored envelope (first-write assumption):
    let stale = writer::next_seq(None);
    assert_eq!(
        tracker.check_put(state_record::DEVICE_KEY_PATH, stale),
        Err(StatefsError::RollbackDetected)
    );
    // Read-modify-write discipline passes:
    let fresh = writer::next_seq(Some(3));
    assert_eq!(tracker.check_put(state_record::DEVICE_KEY_PATH, fresh), Ok(()));
}

#[test]
fn test_reject_malformed_envelope_deterministic_no_panic() {
    // Magic-bearing garbage must be a deterministic error, never a legacy
    // fallback (that would mask tampering) and never a panic.
    let mut truncated = Vec::from(&ENVELOPE_MAGIC[..]);
    truncated.extend_from_slice(&[0u8; 5]);
    assert_eq!(state_record::open_device_key(&truncated), Err(StatefsError::Corrupted));

    // A valid envelope whose payload is not a 32-byte seed is corrupt too.
    let wrong_len = writer::seal_integrity(
        state_record::DEVICE_KEY_PATH,
        1,
        state_record::SUBJECT,
        state_record::PURPOSE_DEVICE_KEY,
        0,
        &[0u8; 31],
    )
    .expect("seal");
    assert_eq!(state_record::open_device_key(&wrong_len), Err(StatefsError::Corrupted));
}

#[test]
fn test_scoped_kv_put_delete_reput_seq_progression() {
    // QEMU regression (2026-08-15): put k1 -> delete -> re-put in the same
    // boot. The server tracker keeps the high-water mark across the DELETE,
    // while the stored value disappears — so the writer must remember the
    // seq it wrote (SeqCache) instead of re-learning "no value -> seq 1".
    let path = "/state/keystore/52c6c4a34ffb3f69/6b31";
    let mut tracker = SeqTracker::new(); // statefsd side
    let mut cache = writer::SeqCache::new(); // keystored side

    // First put: nothing stored, nothing cached -> seq 1, accepted.
    let seq1 = cache.next_for(path, None);
    assert_eq!(seq1, 1);
    tracker.check_put(path, seq1).expect("first put accepted");
    cache.note_written(path, seq1);

    // DELETE: stored value gone; tracker keeps max = 1; cache keeps 1.

    // Re-put (stored = None): a stored-only learner would seal seq 1 again
    // and be denied forever; the cache produces the required seq 2.
    let stale = writer::next_seq(None);
    assert_eq!(tracker.check_put(path, stale), Err(StatefsError::RollbackDetected));
    let seq2 = cache.next_for(path, None);
    assert_eq!(seq2, 2);
    tracker.check_put(path, seq2).expect("re-put after delete accepted");
}

#[test]
fn test_scoped_record_roundtrip_and_legacy() {
    let path = "/state/keystore/00000000deadbeef/6b6579";
    let sealed = state_record::seal_scoped(path, 2, 7, b"value-bytes").expect("seal");
    let (payload, seq) = state_record::open_scoped(&sealed).expect("open");
    assert_eq!(payload, b"value-bytes");
    assert_eq!(seq, Some(2));

    let (legacy, legacy_seq) = state_record::open_scoped(b"plain").expect("legacy");
    assert_eq!(legacy, b"plain");
    assert_eq!(legacy_seq, None);
}
