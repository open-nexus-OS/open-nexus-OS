// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract tests for statefsd write-path hardening v1
//!   (TASK-0025): verify-on-put/get, anti-rollback across restart/replay,
//!   migration acceptance, oversize-meta reject, budget warn accounting and
//!   derivation determinism vs the keystored sign oracle.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal contract)
//! TEST_COVERAGE: This file IS the coverage for `statefsd::hardening`.
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use statefs::envelope::{
    decode, seal, EnvelopeKey, EnvelopeMeta, PolicyClass, SeqTracker, WriteBudget,
    ENVELOPE_HEADER_LEN,
};
use statefs::{JournalEngine, StatefsError};
use statefsd::hardening::{
    self, derive_key_from_device_seed, observe_replayed, policy_class_for, verify_get, verify_put,
    PutCheck, ReplayObserve,
};
use storage::MemBlockDevice;

const AUTH_KEY: &str = "/state/selftest/secure/token";
const PAYLOAD: &[u8] = b"hardening-payload-v1";
const SEED: [u8; 32] = [0x42; 32];

fn mac_key() -> EnvelopeKey {
    derive_key_from_device_seed(&SEED).expect("test derivation")
}

fn meta() -> EnvelopeMeta<'static> {
    EnvelopeMeta { subject: "selftest-client", purpose: "hardening", ts: 1_723_600_000 }
}

fn sealed(seq: u64) -> Vec<u8> {
    seal(PolicyClass::Authenticated, AUTH_KEY, seq, meta(), PAYLOAD, Some(&mac_key()))
        .expect("seal")
}

#[test]
fn test_policy_table_floor_and_authenticated_prefixes() {
    assert_eq!(policy_class_for(AUTH_KEY), PolicyClass::Authenticated);
    // Boot-critical floor (chicken-egg rule) stays Integrity.
    assert_eq!(policy_class_for("/state/keystore/device.signing"), PolicyClass::Integrity);
    assert_eq!(policy_class_for("/state/boot/bootctl.v1"), PolicyClass::Integrity);
    // Unenrolled prefixes stay Off during migration.
    assert_eq!(policy_class_for("/state/settingsd/prefs"), PolicyClass::Off);
    assert_eq!(policy_class_for("/state/selftest/ping"), PolicyClass::Off);
}

#[test]
fn test_wrap_put_restart_replay_verify_ok_same_bytes() {
    // Authenticated wrap survives journal restart/replay byte-identically and
    // verifies against a replay-fed tracker.
    let key = mac_key();
    let value = sealed(7);
    let mut tracker = SeqTracker::new();
    assert_eq!(verify_put(AUTH_KEY, &value, Some(&key), &mut tracker), Ok(PutCheck::Verified));

    let device = MemBlockDevice::new(512, 100);
    let mut engine = JournalEngine::open(device).expect("open");
    engine.put(AUTH_KEY, &value).expect("put");
    engine.sync().expect("sync");

    // "Restart": drop the in-memory map and re-replay the journal from the
    // same device (the same path the OS persist selftest proves).
    engine.reopen().expect("reopen");
    let replayed = engine.get(AUTH_KEY).expect("get");
    assert_eq!(replayed, value);
    let mut tracker2 = SeqTracker::new();
    assert_eq!(observe_replayed(AUTH_KEY, &replayed, &mut tracker2), Ok(ReplayObserve::Observed));
    assert_eq!(tracker2.max_seen(AUTH_KEY), Some(7));
    assert_eq!(verify_get(AUTH_KEY, &replayed, Some(&key), &tracker2), Ok(()));
}

#[test]
fn test_reject_forged_value_integrity_violation() {
    // Bit-flip inside the payload keeps the structure valid; the MAC check
    // must reject with the integrity status, without advancing the tracker.
    let key = mac_key();
    let mut forged = sealed(3);
    let last = forged.len() - 1;
    forged[last] ^= 0x01;
    let mut tracker = SeqTracker::new();
    assert_eq!(
        verify_put(AUTH_KEY, &forged, Some(&key), &mut tracker),
        Err(StatefsError::IntegrityViolation)
    );
    // A forged value must never advance the seq high-water mark.
    assert_eq!(tracker.max_seen(AUTH_KEY), None);
    assert_eq!(
        verify_get(AUTH_KEY, &forged, Some(&key), &tracker),
        Err(StatefsError::IntegrityViolation)
    );
}

#[test]
fn test_reject_stale_seq_rollback_detected() {
    let key = mac_key();
    let mut tracker = SeqTracker::new();
    assert_eq!(verify_put(AUTH_KEY, &sealed(5), Some(&key), &mut tracker), Ok(PutCheck::Verified));
    // Equal seq re-applied is a rollback...
    assert_eq!(
        verify_put(AUTH_KEY, &sealed(5), Some(&key), &mut tracker),
        Err(StatefsError::RollbackDetected)
    );
    // ...and so is any older seq.
    assert_eq!(
        verify_put(AUTH_KEY, &sealed(4), Some(&key), &mut tracker),
        Err(StatefsError::RollbackDetected)
    );
    // The read path flags a rolled-back stored value too.
    assert_eq!(
        verify_get(AUTH_KEY, &sealed(4), Some(&key), &tracker),
        Err(StatefsError::RollbackDetected)
    );
}

#[test]
fn test_reject_oversize_envelope_meta() {
    // Craft raw envelope bytes claiming a 65-byte subject (cap is 64): the
    // bounded decoder must reject deterministically, never panic.
    let mut raw = Vec::new();
    raw.extend_from_slice(b"NXEV");
    raw.push(1); // ver
    raw.push(0); // alg = none
    raw.extend_from_slice(&1u64.to_le_bytes()); // seq
    raw.extend_from_slice(&0u64.to_le_bytes()); // ts
    raw.push(65); // subject_len > MAX_META_FIELD_LEN
    raw.push(0); // purpose_len
    raw.extend_from_slice(&0u32.to_le_bytes()); // payload_len
    raw.resize(ENVELOPE_HEADER_LEN + 65, b'a');
    assert_eq!(decode(&raw), Err(StatefsError::Corrupted));
    let mut tracker = SeqTracker::new();
    assert_eq!(
        verify_put("/state/boot/bootctl.v1", &raw, None, &mut tracker),
        Err(StatefsError::Corrupted)
    );
}

#[test]
fn test_reject_authenticated_without_key_fail_closed() {
    // Chicken-egg boundary: before keystored generated the device key, an
    // Authenticated put cannot be verified — refuse, never accept blindly.
    let value = sealed(1);
    let mut tracker = SeqTracker::new();
    assert_eq!(
        verify_put(AUTH_KEY, &value, None, &mut tracker),
        Err(StatefsError::IntegrityViolation)
    );
}

#[test]
fn test_reject_downgrade_integrity_envelope_on_authenticated_prefix() {
    // alg=none under an Authenticated-mandatory prefix is a downgrade attempt.
    let value =
        seal(PolicyClass::Integrity, AUTH_KEY, 1, meta(), PAYLOAD, None).expect("seal integrity");
    let mut tracker = SeqTracker::new();
    assert_eq!(
        verify_put(AUTH_KEY, &value, Some(&mac_key()), &mut tracker),
        Err(StatefsError::IntegrityViolation)
    );
}

#[test]
fn test_reject_raw_bytes_on_authenticated_prefix_no_migration_window() {
    let mut tracker = SeqTracker::new();
    assert_eq!(
        verify_put(AUTH_KEY, b"raw-legacy-bytes", Some(&mac_key()), &mut tracker),
        Err(StatefsError::IntegrityViolation)
    );
}

#[test]
fn test_migration_accepts_raw_bytes_under_integrity_floor() {
    // keystored/updated still write raw bytes (plan step 4 migrates them):
    // accepted as the documented migration case, and readable.
    let mut tracker = SeqTracker::new();
    let raw = [0x42u8; 32];
    assert_eq!(
        verify_put("/state/keystore/device.signing", &raw, None, &mut tracker),
        Ok(PutCheck::Migration)
    );
    assert_eq!(verify_get("/state/keystore/device.signing", &raw, None, &tracker), Ok(()));
    assert_eq!(
        observe_replayed("/state/keystore/device.signing", &raw, &mut tracker),
        Ok(ReplayObserve::Skipped)
    );
}

#[test]
fn test_off_prefix_passes_through_untouched() {
    let mut tracker = SeqTracker::new();
    assert_eq!(verify_put("/state/selftest/ping", b"ok", None, &mut tracker), Ok(PutCheck::Off));
    assert_eq!(
        observe_replayed("/state/selftest/ping", b"ok", &mut tracker),
        Ok(ReplayObserve::NotEnrolled)
    );
}

#[test]
fn test_replay_observe_never_panics_on_malformed_old_values() {
    // Truncated/garbage bytes that happen to carry the magic under an
    // enrolled prefix are skipped (bounded, total) — not a crash, not a stop.
    let mut tracker = SeqTracker::new();
    for garbage in [&b"NXEV"[..], b"NXEV\x01\x00garbage", &[0x4e, 0x58, 0x45, 0x56, 0xff]] {
        assert_eq!(
            observe_replayed("/state/boot/bootctl.v1", garbage, &mut tracker),
            Ok(ReplayObserve::Skipped)
        );
    }
}

#[test]
fn test_budget_warn_accounting_visible() {
    // Deterministic latency-budget hook: injected elapsed time, no wall clock.
    let mut budget = WriteBudget::new(hardening::WRITE_BUDGET_NS);
    assert!(!budget.record(hardening::WRITE_BUDGET_NS)); // at budget: no warn
    assert!(budget.record(hardening::WRITE_BUDGET_NS + 1)); // over: warn
    assert!(budget.record(u64::MAX)); // saturates, still warns
    assert_eq!(budget.warn_count(), 2);
}

#[test]
fn test_derivation_matches_keystored_sign_oracle() {
    // The statefsd-local derivation (seed -> deterministic Ed25519 signature
    // -> HKDF) must equal what a writer derives from keystored's
    // OP_DEVICE_SIGN response over the same label (mirrored here with the
    // same primitive keystored uses).
    use ed25519_dalek::Signer;
    let local = derive_key_from_device_seed(&SEED).expect("derive");
    let oracle_sig =
        ed25519_dalek::SigningKey::from_bytes(&SEED).sign(statefs::derive::ENVELOPE_KEY_LABEL_V1);
    let writer =
        statefs::derive::envelope_key_from_ikm(&oracle_sig.to_bytes()).expect("writer derive");
    assert_eq!(local.0, writer.0);
    // And the raw seed must never be the MAC key.
    assert_ne!(local.0, SEED);
}
