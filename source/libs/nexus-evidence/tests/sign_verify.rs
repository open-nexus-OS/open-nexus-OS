// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: P5-03 sign/verify integration tests. Locks the 5
//! tamper-classes (manifest / uart / trace / config / signature
//! swap) plus round-trip sanity for both key labels plus the
//! `--policy=ci` rejection path. These tests are what guarantee
//! that any silent regression in canonical hashing, signature
//! framing, or tamper detection trips CI.
//!
//! Test fixture strategy: rather than read a real bundle from
//! disk, we build minimal in-memory bundles via
//! [`nexus_evidence::test_support::empty_bundle`], populate just
//! enough fields to exercise the relevant artifact, and round-trip
//! through `write_unsigned` + `read_unsigned`. The 5 tamper classes
//! mutate exactly one artifact byte (or the signature byte stream)
//! between `seal` and `verify`; nothing else moves.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-03 surface)

use std::collections::BTreeMap;

use nexus_evidence::{
    canonical_hash, read_unsigned, test_support::empty_bundle, write_unsigned, Bundle,
    EvidenceError, KeyLabel, SigningKey, TraceEntry, VerifyingKey,
};

mod tempdir;
use tempdir::Tempdir;

fn fixed_seed_a() -> [u8; 32] {
    // SECURITY: bring-up test keys; deterministic seed so the
    // signature wire format is reproducible across CI runs.
    let mut seed = [0u8; 32];
    for (i, b) in seed.iter_mut().enumerate() {
        *b = (i as u8) ^ 0xA5;
    }
    seed
}

fn fixed_seed_b() -> [u8; 32] {
    // SECURITY: bring-up test keys; second deterministic seed so
    // we can test cross-key rejection (B's signature must not
    // verify under A's pubkey).
    let mut seed = [0u8; 32];
    for (i, b) in seed.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7) ^ 0x5A;
    }
    seed
}

fn populated_bundle(profile: &str) -> Bundle {
    let mut b = empty_bundle();
    b.meta.profile = profile.to_string();
    b.manifest.bytes = b"manifest-tar-bytes-stand-in".to_vec();
    b.uart.bytes = b"SELFTEST: synthetic ok\n".to_vec();
    b.trace.entries.push(TraceEntry {
        marker: "SELFTEST: synthetic ok".to_string(),
        phase: "bringup".to_string(),
        ts_ms_from_boot: Some(123),
        profile: profile.to_string(),
    });
    b.config.profile = profile.to_string();
    b.config.env = BTreeMap::from([("PROFILE".into(), profile.into())]);
    b.config.kernel_cmdline = "console=ttyS0".into();
    b.config.host_info = "Linux test 6.0".into();
    b.config.build_sha = "0123abc".into();
    b.config.rustc_version = "rustc 1.89.0".into();
    b.config.qemu_version = "QEMU 9.0".into();
    b.config.wall_clock_utc = "2026-04-17T00:00:00Z".into();
    b
}

#[test]
fn round_trip_bringup_label() {
    let dir = Tempdir::new("p5_03_bringup");
    let path = dir.path().join("bundle.tar.gz");

    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    write_unsigned(&sealed, &path).unwrap();
    let read = read_unsigned(&path).unwrap();

    assert!(read.signature.is_some(), "signature.bin must round-trip");
    assert_eq!(read.signature.as_ref().unwrap().label, KeyLabel::Bringup);
    read.verify(&verifying, None).expect("bringup verify");
    read.verify(&verifying, Some(KeyLabel::Bringup))
        .expect("bringup policy ok");
}

#[test]
fn round_trip_ci_label() {
    let dir = Tempdir::new("p5_03_ci");
    let path = dir.path().join("bundle.tar.gz");

    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Ci)
        .expect("seal");
    write_unsigned(&sealed, &path).unwrap();
    let read = read_unsigned(&path).unwrap();

    assert_eq!(read.signature.as_ref().unwrap().label, KeyLabel::Ci);
    read.verify(&verifying, Some(KeyLabel::Ci))
        .expect("ci verify");
}

#[test]
fn tamper_class_a_manifest_bytes_change() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let mut sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    // Tamper: flip one manifest byte after sealing.
    sealed.manifest.bytes[0] ^= 0x01;

    let err = sealed.verify(&verifying, None).unwrap_err();
    assert!(
        matches!(err, EvidenceError::SignatureMismatch { .. }),
        "expected SignatureMismatch, got {:?}",
        err
    );
}

#[test]
fn tamper_class_b_uart_bytes_change() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let mut sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    sealed.uart.bytes.extend_from_slice(b"injected line\n");

    let err = sealed.verify(&verifying, None).unwrap_err();
    assert!(matches!(err, EvidenceError::SignatureMismatch { .. }));
}

#[test]
fn tamper_class_c_trace_entry_added() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let mut sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    sealed.trace.entries.push(TraceEntry {
        marker: "SELFTEST: forged ok".into(),
        phase: "bringup".into(),
        ts_ms_from_boot: Some(999),
        profile: "full".into(),
    });

    let err = sealed.verify(&verifying, None).unwrap_err();
    assert!(matches!(err, EvidenceError::SignatureMismatch { .. }));
}

#[test]
fn tamper_class_d_config_field_change() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let mut sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    // Mutate a hashed config field (not wall_clock_utc, which is
    // intentionally excluded from the canonical hash).
    sealed.config.kernel_cmdline.push_str(" hacker=1");

    let err = sealed.verify(&verifying, None).unwrap_err();
    assert!(matches!(err, EvidenceError::SignatureMismatch { .. }));
}

#[test]
fn tamper_class_e_signature_swap_between_bundles() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let bundle_a = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    let mut bundle_b = populated_bundle("smp")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    // Swap A's signature into B. The signature is structurally
    // valid (same key, same label) but commits to A's hash, not
    // B's.
    bundle_b.signature = bundle_a.signature.clone();

    assert_ne!(canonical_hash(&bundle_a), canonical_hash(&bundle_b));
    let err = bundle_b.verify(&verifying, None).unwrap_err();
    assert!(matches!(err, EvidenceError::SignatureMismatch { .. }));
}

#[test]
fn policy_ci_rejects_bringup_signed_bundle() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let sealed = populated_bundle("full")
        .seal(&signing, KeyLabel::Bringup)
        .expect("seal");
    let err = sealed.verify(&verifying, Some(KeyLabel::Ci)).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::KeyLabelMismatch {
                expected: "ci",
                got: "bringup"
            }
        ),
        "expected KeyLabelMismatch ci<-bringup, got {:?}",
        err
    );
}

#[test]
fn cross_key_rejection_bundle_signed_by_b_verified_by_a() {
    let signing_a = SigningKey::from_seed(fixed_seed_a());
    let signing_b = SigningKey::from_seed(fixed_seed_b());
    let verifying_a = signing_a.verifying_key();

    let sealed = populated_bundle("full")
        .seal(&signing_b, KeyLabel::Ci)
        .expect("seal");
    let err = sealed.verify(&verifying_a, None).unwrap_err();
    assert!(
        matches!(err, EvidenceError::SignatureMismatch { .. }),
        "expected SignatureMismatch (cross-key), got {:?}",
        err
    );
}

#[test]
fn signature_unsupported_version_rejected() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let sig = signing.sign_hash([0u8; 32], KeyLabel::Bringup);
    let mut bytes = sig.to_bytes();
    // Flip the version byte. Spec: P5-03 understands only 0x01.
    bytes[4] = 0xFE;
    let err = nexus_evidence::Signature::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::UnsupportedSignatureVersion {
                got: 0xFE,
                supported: 0x01
            }
        ),
        "expected UnsupportedSignatureVersion 0xFE -> 0x01, got {:?}",
        err
    );
}

#[test]
fn signature_bad_magic_rejected() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let sig = signing.sign_hash([0u8; 32], KeyLabel::Bringup);
    let mut bytes = sig.to_bytes();
    bytes[0] = b'X'; // corrupt magic
    let err = nexus_evidence::Signature::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(err, EvidenceError::SignatureMalformed { .. }),
        "expected SignatureMalformed (bad magic), got {:?}",
        err
    );
}

#[test]
fn unsigned_bundle_verify_returns_signature_missing() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let verifying = signing.verifying_key();

    let unsigned = populated_bundle("full");
    let err = unsigned.verify(&verifying, None).unwrap_err();
    assert!(
        matches!(err, EvidenceError::SignatureMissing),
        "expected SignatureMissing, got {:?}",
        err
    );
}

#[test]
fn pubkey_round_trips_through_bytes() {
    let signing = SigningKey::from_seed(fixed_seed_a());
    let v1 = signing.verifying_key();
    let bytes = v1.to_bytes();
    let v2 = VerifyingKey::from_bytes(bytes).unwrap();
    assert_eq!(bytes, v2.to_bytes());
}
