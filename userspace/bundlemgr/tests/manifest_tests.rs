// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// CONTEXT: Manifest parsing tests (canonical manifest.nxb)
// OWNERS: @runtime
//
// NOTE:
// This test file historically validated the TOML manifest parser. The repo is now
// unified on `manifest.nxb` (Cap'n Proto) as the *only* on-disk contract (ADR-0020).
// These tests were migrated accordingly to prevent drift.

use bundlemgr::manifest::{Error, Manifest};
use capnp::message::Builder;
use nexus_idl_runtime::manifest_capnp::bundle_manifest;

fn build_manifest_bytes(
    name: &str,
    semver: &str,
    abilities: &[&str],
    caps: &[&str],
    publisher: &[u8],
    signature: &[u8],
) -> Vec<u8> {
    let mut builder = Builder::new_default();
    {
        let mut msg = builder.init_root::<bundle_manifest::Builder<'_>>();
        msg.set_schema_version(1);
        msg.set_name(name);
        msg.set_semver(semver);
        msg.set_min_sdk("0.1.0");
        msg.set_publisher(publisher);
        msg.set_signature(signature);
        let mut a = msg.reborrow().init_abilities(abilities.len() as u32);
        for (i, v) in abilities.iter().enumerate() {
            a.set(i as u32, v);
        }
        let mut c = msg.reborrow().init_capabilities(caps.len() as u32);
        for (i, v) in caps.iter().enumerate() {
            c.set(i as u32, v);
        }
    }
    let mut out = Vec::new();
    capnp::serialize::write_message(&mut out, &builder).unwrap();
    out
}

#[test]
fn parses_valid_manifest() {
    let bytes =
        build_manifest_bytes("demo.hello", "1.0.0", &["demo"], &["gpu"], &[0u8; 16], &[0u8; 64]);
    let m = Manifest::parse_nxb(&bytes).expect("parse ok");
    assert_eq!(m.name, "demo.hello");
    assert_eq!(m.version.to_string(), "1.0.0");
    assert_eq!(m.abilities, vec!["demo".to_string()]);
    assert_eq!(m.capabilities, vec!["gpu".to_string()]);
    // publisher is stored as hex string for display/debug
    assert_eq!(m.publisher, "00".repeat(16));
}

#[test]
fn test_rejects_empty_name() {
    let bytes = build_manifest_bytes("   ", "1.0.0", &["demo"], &[], &[0u8; 16], &[0u8; 64]);
    let err = Manifest::parse_nxb(&bytes).expect_err("expected failure");
    assert!(matches!(err, Error::InvalidField { field: "name", .. }));
}

#[test]
fn test_rejects_invalid_semver() {
    let bytes =
        build_manifest_bytes("demo.hello", "not-a-semver", &["demo"], &[], &[0u8; 16], &[0u8; 64]);
    let err = Manifest::parse_nxb(&bytes).expect_err("expected failure");
    assert!(matches!(err, Error::InvalidField { field: "semver", .. }));
}

#[test]
fn test_rejects_missing_abilities() {
    let bytes = build_manifest_bytes("demo.hello", "1.0.0", &[], &[], &[0u8; 16], &[0u8; 64]);
    let err = Manifest::parse_nxb(&bytes).expect_err("expected failure");
    assert!(matches!(err, Error::InvalidField { field: "abilities", .. }));
}

#[test]
fn test_rejects_wrong_publisher_len() {
    let bytes = build_manifest_bytes("demo.hello", "1.0.0", &["demo"], &[], &[0u8; 15], &[0u8; 64]);
    let err = Manifest::parse_nxb(&bytes).expect_err("expected failure");
    assert!(matches!(err, Error::InvalidField { field: "publisher", .. }));
}

#[test]
fn test_rejects_wrong_signature_len() {
    let bytes = build_manifest_bytes("demo.hello", "1.0.0", &["demo"], &[], &[0u8; 16], &[0u8; 3]);
    let err = Manifest::parse_nxb(&bytes).expect_err("expected failure");
    assert!(matches!(err, Error::InvalidField { field: "signature", .. }));
}
