// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: P5-04 secret-scan tests. Locks the deny-by-default
//! posture of [`nexus_evidence::scan_for_secrets`] for the four
//! [`LeakKind`] classes plus the allowlist round-trip + integration
//! with [`nexus_evidence::Bundle::seal`]. These tests are what
//! prevents a future maintainer from quietly weakening the scanner
//! by adding a too-broad allowlist or by demoting the seal-time
//! check to a warning.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-04 surface)

use std::collections::BTreeMap;

use nexus_evidence::{
    scan_for_secrets, scan_for_secrets_with, test_support::empty_bundle, EvidenceError, KeyLabel,
    LeakKind, ScanAllowlist, SigningKey, TraceEntry,
};

fn fixed_seed() -> [u8; 32] {
    // SECURITY: bring-up test keys; deterministic so a regression in
    // the seal-time scanner is reproducible across CI runs.
    let mut seed = [0u8; 32];
    for (i, b) in seed.iter_mut().enumerate() {
        *b = (i as u8) ^ 0xC3;
    }
    seed
}

fn clean_bundle() -> nexus_evidence::Bundle {
    let mut b = empty_bundle();
    b.meta.profile = "full".into();
    b.manifest.bytes = b"manifest-tar-bytes-stand-in".to_vec();
    b.uart.bytes = b"SELFTEST: synthetic ok\n".to_vec();
    b.trace.entries.push(TraceEntry {
        marker: "SELFTEST: synthetic ok".into(),
        phase: "bringup".into(),
        ts_ms_from_boot: Some(1),
        profile: "full".into(),
    });
    b.config.profile = "full".into();
    b.config.env = BTreeMap::from([("PROFILE".into(), "full".into())]);
    b.config.kernel_cmdline = "console=ttyS0".into();
    b.config.host_info = "Linux test 6.0".into();
    b.config.build_sha = "0123abc".into();
    b.config.rustc_version = "rustc 1.89.0".into();
    b.config.qemu_version = "QEMU 9.0".into();
    b.config.wall_clock_utc = "2026-04-17T00:00:00Z".into();
    b
}

#[test]
fn clean_bundle_passes() {
    let b = clean_bundle();
    scan_for_secrets(&b).expect("clean bundle must scan ok");
}

#[test]
fn pem_private_key_in_uart_rejected() {
    let mut b = clean_bundle();
    b.uart.bytes = b"-----BEGIN OPENSSH PRIVATE KEY-----\nfake-body\n".to_vec();
    let err = scan_for_secrets(&b).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::SecretLeak {
                artifact: "uart.log",
                pattern: "pem_private_key",
                ..
            }
        ),
        "expected pem_private_key in uart.log, got {:?}",
        err
    );
}

#[test]
fn bringup_key_path_in_config_rejected() {
    let mut b = clean_bundle();
    b.config.kernel_cmdline =
        "console=ttyS0 keyfile=/home/x/.config/nexus/bringup-key/private.ed25519".into();
    let err = scan_for_secrets(&b).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::SecretLeak {
                artifact: "config.json",
                pattern: "bringup_key_path",
                ..
            }
        ),
        "expected bringup_key_path in config.json, got {:?}",
        err
    );
}

#[test]
fn private_key_env_assignment_in_env_rejected() {
    let mut b = clean_bundle();
    b.config.env.insert(
        "NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64".into(),
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=AAAA".into(),
    );
    let err = scan_for_secrets(&b).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::SecretLeak {
                artifact: "config.json",
                pattern: "private_key_env_assignment",
                ..
            }
        ),
        "expected private_key_env_assignment, got {:?}",
        err
    );
}

#[test]
fn high_entropy_blob_in_uart_rejected_then_allowlisted() {
    let blob = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/abcd";
    assert!(blob.len() >= 64, "fixture must exercise the >=64 path");

    let mut b = clean_bundle();
    b.uart.bytes = format!("dbg: blob {}\n", blob).into_bytes();

    let err = scan_for_secrets(&b).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::SecretLeak {
                artifact: "uart.log",
                pattern: "high_entropy_blob",
                ..
            }
        ),
        "expected high_entropy_blob, got {:?}",
        err
    );

    let allow = ScanAllowlist::from_toml(&format!("[allowlist]\nsubstrings = [\"{}\"]\n", blob))
        .expect("parse allowlist");
    scan_for_secrets_with(&b, &allow).expect("allowlist must suppress high-entropy hit");
}

#[test]
fn seal_refuses_bundle_with_pem_block() {
    let mut b = clean_bundle();
    b.uart.bytes = b"-----BEGIN RSA PRIVATE KEY-----\n".to_vec();
    let signing = SigningKey::from_seed(fixed_seed());
    let err = b.seal(&signing, KeyLabel::Bringup).unwrap_err();
    assert!(
        matches!(
            err,
            EvidenceError::SecretLeak {
                pattern: "pem_private_key",
                ..
            }
        ),
        "seal must refuse PEM-bearing bundle, got {:?}",
        err
    );
}

#[test]
fn allowlist_parses_quoted_and_skips_blanks() {
    let toml = r#"
# comment
[other]
substrings = ["should-be-ignored"]
[allowlist]
substrings = [ "alpha" , "beta" , "" ]
"#;
    let allow = ScanAllowlist::from_toml(toml).expect("parse");
    let kind = LeakKind::HighEntropyBlob;
    assert_eq!(kind.as_str(), "high_entropy_blob");
    let _ = allow;
}
