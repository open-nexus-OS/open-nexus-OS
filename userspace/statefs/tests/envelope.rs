// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Authenticity envelope v1 contract tests (public API, host)
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: Roundtrip + tamper/rollback/truncation/oversize negatives
//!
//! Exercises `statefs::envelope` through the public crate surface only. The
//! journal-roundtrip test (needs the crate-private device handle) lives in
//! `src/envelope.rs` `#[cfg(test)]`.

use statefs::envelope::{
    decode, open, seal, Envelope, EnvelopeKey, EnvelopeMeta, PolicyClass, SeqTracker, WriteBudget,
    ALG_HMAC_SHA256, ALG_NONE, ENVELOPE_HEADER_LEN, ENVELOPE_MAGIC, ENVELOPE_VERSION,
    MAX_META_FIELD_LEN,
};
use statefs::{protocol, StatefsError, MAX_VALUE_SIZE};

const KEY: &str = "/state/settingsd/prefs";
const PAYLOAD: &[u8] = b"theme=dark\nscale=2\n";

fn meta() -> EnvelopeMeta<'static> {
    EnvelopeMeta { subject: "settingsd", purpose: "prefs", ts: 1_723_600_000 }
}

fn mac_key() -> EnvelopeKey {
    EnvelopeKey([0x42; 32])
}

#[test]
fn test_roundtrip_authenticated_same_bytes() {
    let key = mac_key();
    let sealed = seal(PolicyClass::Authenticated, KEY, 7, meta(), PAYLOAD, Some(&key)).unwrap();
    let env: Envelope<'_> = decode(&sealed).unwrap();
    assert_eq!(env.ver, ENVELOPE_VERSION);
    assert_eq!(env.alg, ALG_HMAC_SHA256);
    assert_eq!(env.seq, 7);
    assert_eq!(env.subject, "settingsd");
    assert_eq!(env.purpose, "prefs");
    assert_eq!(env.ts, 1_723_600_000);
    env.verify_mac(KEY, &key).unwrap();

    let tracker = SeqTracker::new();
    let out = open(PolicyClass::Authenticated, KEY, &sealed, Some(&key), &tracker).unwrap();
    assert_eq!(out, PAYLOAD);
}

#[test]
fn test_roundtrip_integrity_same_bytes() {
    let sealed = seal(PolicyClass::Integrity, KEY, 3, meta(), PAYLOAD, None).unwrap();
    let env = decode(&sealed).unwrap();
    assert_eq!(env.alg, ALG_NONE);
    assert_eq!(env.hmac, None);
    let tracker = SeqTracker::new();
    let out = open(PolicyClass::Integrity, KEY, &sealed, None, &tracker).unwrap();
    assert_eq!(out, PAYLOAD);
}

#[test]
fn test_reject_forged_payload_bitflip() {
    let key = mac_key();
    let sealed = seal(PolicyClass::Authenticated, KEY, 1, meta(), PAYLOAD, Some(&key)).unwrap();
    let mut forged = sealed.clone();
    let last = forged.len() - 1;
    forged[last] ^= 0x01; // flip a payload bit; structure stays valid
    let env = decode(&forged).unwrap();
    assert_eq!(env.verify_mac(KEY, &key), Err(StatefsError::IntegrityViolation));
    let tracker = SeqTracker::new();
    assert_eq!(
        open(PolicyClass::Authenticated, KEY, &forged, Some(&key), &tracker),
        Err(StatefsError::IntegrityViolation)
    );
}

#[test]
fn test_reject_tampered_meta() {
    let key = mac_key();
    let sealed = seal(PolicyClass::Authenticated, KEY, 1, meta(), PAYLOAD, Some(&key)).unwrap();
    let mut forged = sealed.clone();
    // Subject starts right after the fixed header; xor keeps it ASCII.
    forged[ENVELOPE_HEADER_LEN] ^= 0x01;
    let env = decode(&forged).unwrap();
    assert_ne!(env.subject, "settingsd");
    assert_eq!(env.verify_mac(KEY, &key), Err(StatefsError::IntegrityViolation));
}

#[test]
fn test_reject_wrong_key_and_wrong_key_path() {
    let key = mac_key();
    let sealed = seal(PolicyClass::Authenticated, KEY, 1, meta(), PAYLOAD, Some(&key)).unwrap();
    let env = decode(&sealed).unwrap();
    let wrong = EnvelopeKey([0x24; 32]);
    assert_eq!(env.verify_mac(KEY, &wrong), Err(StatefsError::IntegrityViolation));
    // The storage key path is part of the MAC input: splicing the value
    // onto another key must fail even with the right MAC key.
    assert_eq!(env.verify_mac("/state/other/key", &key), Err(StatefsError::IntegrityViolation));
}

#[test]
fn test_reject_alg_downgrade_when_authenticated_required() {
    let key = mac_key();
    let sealed = seal(PolicyClass::Integrity, KEY, 1, meta(), PAYLOAD, None).unwrap();
    let tracker = SeqTracker::new();
    assert_eq!(
        open(PolicyClass::Authenticated, KEY, &sealed, Some(&key), &tracker),
        Err(StatefsError::IntegrityViolation)
    );
}

#[test]
fn test_reject_stale_seq_rollback() {
    let mut tracker = SeqTracker::new();
    tracker.check_put(KEY, 5).unwrap();
    assert_eq!(tracker.check_put(KEY, 5), Err(StatefsError::RollbackDetected));
    assert_eq!(tracker.check_put(KEY, 4), Err(StatefsError::RollbackDetected));
    tracker.check_put(KEY, 6).unwrap();

    // Read path: a stored value below the high-water mark is a rollback.
    let key = mac_key();
    let stale = seal(PolicyClass::Authenticated, KEY, 2, meta(), PAYLOAD, Some(&key)).unwrap();
    assert_eq!(
        open(PolicyClass::Authenticated, KEY, &stale, Some(&key), &tracker),
        Err(StatefsError::RollbackDetected)
    );
    // Equal to max-seen is the expected steady state.
    let fresh = seal(PolicyClass::Authenticated, KEY, 6, meta(), PAYLOAD, Some(&key)).unwrap();
    assert_eq!(
        open(PolicyClass::Authenticated, KEY, &fresh, Some(&key), &tracker).unwrap(),
        PAYLOAD
    );
}

#[test]
fn test_reject_oversize_meta_and_payload() {
    let big = "x".repeat(MAX_META_FIELD_LEN + 1);
    let m = EnvelopeMeta { subject: &big, purpose: "p", ts: 0 };
    assert_eq!(
        seal(PolicyClass::Integrity, KEY, 1, m, PAYLOAD, None),
        Err(StatefsError::Corrupted)
    );
    let huge = vec![0u8; MAX_VALUE_SIZE];
    assert_eq!(
        seal(PolicyClass::Integrity, KEY, 1, meta(), &huge, None),
        Err(StatefsError::ValueTooLarge)
    );
    // Oversize length fields in the header reject deterministically.
    let mut sealed = seal(PolicyClass::Integrity, KEY, 1, meta(), PAYLOAD, None).unwrap();
    sealed[22] = (MAX_META_FIELD_LEN + 1) as u8;
    assert_eq!(decode(&sealed), Err(StatefsError::Corrupted));
}

#[test]
fn test_reject_truncation_at_every_length() {
    let key = mac_key();
    let sealed = seal(PolicyClass::Authenticated, KEY, 9, meta(), PAYLOAD, Some(&key)).unwrap();
    for len in 0..sealed.len() {
        assert_eq!(
            decode(&sealed[..len]),
            Err(StatefsError::Corrupted),
            "truncated to {len} bytes must reject"
        );
    }
    assert!(decode(&sealed).is_ok());
    // Bad magic / version / algorithm reject deterministically.
    let mut bad = sealed.clone();
    bad[0] ^= 0xFF;
    assert_eq!(decode(&bad), Err(StatefsError::Corrupted));
    let mut bad = sealed.clone();
    bad[4] = 2;
    assert_eq!(decode(&bad), Err(StatefsError::Corrupted));
    let mut bad = sealed;
    bad[5] = 7;
    assert_eq!(decode(&bad), Err(StatefsError::Corrupted));
}

#[test]
fn test_policy_off_ignores_magic_prefix() {
    // Non-enrolled bytes that happen to start with "NXEV" must pass
    // through untouched when the policy class is Off.
    let mut bytes = Vec::from(&ENVELOPE_MAGIC[..]);
    bytes.extend_from_slice(b"not an envelope at all");
    let tracker = SeqTracker::new();
    let out = open(PolicyClass::Off, KEY, &bytes, None, &tracker).unwrap();
    assert_eq!(out, &bytes[..]);
}

#[test]
fn test_seq_tracker_replay_idempotent() {
    let events = [("/state/a", 1u64), ("/state/b", 4), ("/state/a", 3), ("/state/b", 2)];
    let mut tracker = SeqTracker::new();
    for _ in 0..2 {
        for (k, s) in events {
            tracker.observe(k, s).unwrap();
        }
    }
    assert_eq!(tracker.max_seen("/state/a"), Some(3));
    assert_eq!(tracker.max_seen("/state/b"), Some(4));
    assert_eq!(tracker.len(), 2);
}

#[test]
fn test_write_budget_warn_accounting() {
    let mut budget = WriteBudget::new(1_000);
    assert!(!budget.record(999));
    assert!(!budget.record(1_000)); // exactly on budget: no warning
    assert!(budget.record(1_001));
    assert!(budget.record(50_000));
    assert_eq!(budget.warn_count(), 2);
    assert_eq!(budget.budget_ns(), 1_000);
}

#[test]
fn test_default_policy_class_boot_critical() {
    assert_eq!(
        PolicyClass::default_for_key("/state/keystore/device.signing"),
        PolicyClass::Integrity
    );
    assert_eq!(PolicyClass::default_for_key("/state/boot/bootctl.v1"), PolicyClass::Integrity);
    assert_eq!(PolicyClass::default_for_key("/state/settingsd/prefs"), PolicyClass::Off);
}

#[test]
fn test_new_status_codes_roundtrip() {
    assert_eq!(
        protocol::status_from_error(StatefsError::IntegrityViolation),
        protocol::STATUS_INTEGRITY_VIOLATION
    );
    assert_eq!(
        protocol::status_from_error(StatefsError::RollbackDetected),
        protocol::STATUS_ROLLBACK_DETECTED
    );
    assert_eq!(
        protocol::error_from_status(protocol::STATUS_INTEGRITY_VIOLATION),
        StatefsError::IntegrityViolation
    );
    assert_eq!(
        protocol::error_from_status(protocol::STATUS_ROLLBACK_DETECTED),
        StatefsError::RollbackDetected
    );
}
