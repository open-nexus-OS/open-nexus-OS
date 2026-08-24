// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host DoD matrix for `.nxra` (TASK-0053 / RFC-0088): the full
//! operator→verifier pipeline over a SIMULATED hwm store — sign/verify
//! happy path, tamper, expiry/not-before, replay across a persisted mark
//! (consume-before-act), and every stable reject label the audit/marker
//! surface consumes. Format/unit truths live in `cargo test -p nxra`;
//! this file pins the end-to-end flow a verifier implements.
//! OWNERS: @security @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file
//! ADR: docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md

use std::collections::HashMap;

use nxra::{keyid8, sign, verify, Action, Reject, TrustEntry, TOKEN_LEN};

const OPERATOR_KEY: [u8; 32] = [42u8; 32];
const ALL: &[Action] = &[Action::SlotSwitch, Action::TargetSet, Action::FsckRepair, Action::Reset];

fn operator_pubkey() -> [u8; 32] {
    *ed25519_dalek::SigningKey::from_bytes(&OPERATOR_KEY).verifying_key().as_bytes()
}

fn trust() -> Vec<TrustEntry> {
    vec![TrustEntry { pubkey: operator_pubkey(), actions: ALL }]
}

/// Simulated per-(verifier, key) hwm store — the statefs record stand-in.
struct HwmStore(HashMap<(&'static str, [u8; 8]), u64>);

impl HwmStore {
    fn new() -> Self {
        Self(HashMap::new())
    }

    fn get(&self, verifier: &'static str, key: &[u8; 32]) -> u64 {
        *self.0.get(&(verifier, keyid8(key))).unwrap_or(&0)
    }

    /// Consume-before-act: persists the mark; the caller acts afterwards.
    fn consume(&mut self, verifier: &'static str, key: &[u8; 32], seq: u64) {
        self.0.insert((verifier, keyid8(key)), seq);
    }
}

#[test]
fn sign_verify_consume_act_happy_path() {
    let mut store = HwmStore::new();
    let token = sign(Action::SlotSwitch, 100, None, 1, &OPERATOR_KEY);
    let hwm = store.get("bootctld", &operator_pubkey());
    let verdict =
        verify(&token, &trust(), Action::SlotSwitch, 1, hwm, None).expect("first use verifies");
    store.consume("bootctld", &verdict.pubkey, verdict.seq);
    assert_eq!(store.get("bootctld", &operator_pubkey()), 100);
}

#[test]
fn test_reject_replay_after_consume_and_across_crash() {
    let mut store = HwmStore::new();
    let token = sign(Action::FsckRepair, 7, None, 0, &OPERATOR_KEY);
    let hwm = store.get("statefsd", &operator_pubkey());
    let verdict = verify(&token, &trust(), Action::FsckRepair, 0, hwm, None).expect("first");
    // Consume persists BEFORE the action — a crash here burns the token
    // (holder re-mints; never a brick) and replay stays dead either way.
    store.consume("statefsd", &verdict.pubkey, verdict.seq);
    let hwm = store.get("statefsd", &operator_pubkey());
    assert_eq!(verify(&token, &trust(), Action::FsckRepair, 0, hwm, None), Err(Reject::Replay));
    // A FRESH token with a higher seq goes through.
    let fresh = sign(Action::FsckRepair, 8, None, 0, &OPERATOR_KEY);
    assert!(verify(&fresh, &trust(), Action::FsckRepair, 0, hwm, None).is_ok());
}

#[test]
fn hwm_is_per_verifier_and_per_key() {
    let mut store = HwmStore::new();
    store.consume("bootctld", &operator_pubkey(), 50);
    // Same key, other verifier: independent mark (actions bind verifiers,
    // so cross-verifier replay dies at action-denied, not here).
    assert_eq!(store.get("statefsd", &operator_pubkey()), 0);
    let other = *ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]).verifying_key().as_bytes();
    assert_eq!(store.get("bootctld", &other), 0);
}

#[test]
fn test_reject_tamper_and_oversize_and_malformed() {
    let token = sign(Action::Reset, 1, None, 0, &OPERATOR_KEY);
    let mut tampered = token;
    tampered[8] ^= 0x01; // seq bit inside the signed span
    assert_eq!(verify(&tampered, &trust(), Action::Reset, 0, 0, None), Err(Reject::BadSignature));
    let mut oversized = token.to_vec();
    oversized.push(0);
    assert_eq!(verify(&oversized, &trust(), Action::Reset, 0, 0, None), Err(Reject::Malformed));
    assert_eq!(
        verify(&[0u8; TOKEN_LEN], &trust(), Action::Reset, 0, 0, None),
        Err(Reject::Malformed)
    );
}

#[test]
fn test_reject_expiry_and_not_before_enforced_with_clock_only() {
    let token = sign(Action::TargetSet, 2, Some((1_000, 2_000)), 1, &OPERATOR_KEY);
    let t = trust();
    assert_eq!(verify(&token, &t, Action::TargetSet, 1, 0, None), Err(Reject::NoClock));
    assert_eq!(verify(&token, &t, Action::TargetSet, 1, 0, Some(999)), Err(Reject::NotYetValid));
    assert_eq!(verify(&token, &t, Action::TargetSet, 1, 0, Some(2_000)), Err(Reject::Expired));
    assert!(verify(&token, &t, Action::TargetSet, 1, 0, Some(1_500)).is_ok());
}

#[test]
fn test_reject_untrusted_key_unknown_version_action_denied() {
    let stranger = sign(Action::Reset, 1, None, 0, &[13u8; 32]);
    assert_eq!(verify(&stranger, &trust(), Action::Reset, 0, 0, None), Err(Reject::UntrustedKey));
    let mut wrong_version = sign(Action::Reset, 1, None, 0, &OPERATOR_KEY);
    wrong_version[4] = 2;
    assert_eq!(
        verify(&wrong_version, &trust(), Action::Reset, 0, 0, None),
        Err(Reject::UnknownVersion)
    );
    // Key trusted for fsck-repair only must not authorize a reset.
    let narrow = vec![TrustEntry { pubkey: operator_pubkey(), actions: &[Action::FsckRepair] }];
    let token = sign(Action::Reset, 1, None, 0, &OPERATOR_KEY);
    assert_eq!(verify(&token, &narrow, Action::Reset, 0, 0, None), Err(Reject::ActionDenied));
}

/// The image-baked anchor (policies/nxra-trust.toml) authorizes the proof
/// key end to end — exactly what the OS selftest relies on.
#[test]
fn baked_trust_carries_the_proof_key() {
    assert!(!nxra::BAKED_TRUST.is_empty());
    let token = sign(Action::TargetSet, 1, None, 0, &OPERATOR_KEY);
    assert!(verify(&token, nxra::BAKED_TRUST, Action::TargetSet, 0, 0, None).is_ok());
}

#[test]
fn reject_labels_are_the_stable_marker_contract() {
    for (reject, label) in [
        (Reject::Malformed, "malformed"),
        (Reject::UnknownVersion, "unknown-version"),
        (Reject::UntrustedKey, "untrusted-key"),
        (Reject::BadSignature, "bad-signature"),
        (Reject::ActionDenied, "action-denied"),
        (Reject::Replay, "replay"),
        (Reject::Expired, "expired"),
        (Reject::NotYetValid, "not-yet-valid"),
        (Reject::NoClock, "no-clock"),
    ] {
        assert_eq!(reject.label(), label);
    }
}
