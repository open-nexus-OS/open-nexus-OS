// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Reject-case proofs for the update-trust bake parser (RFC-0089 §4,
//! TASK-0198 Phase 1). The build script itself cannot be unit-tested, so the
//! shared narrow parser (`userspace/updates/build_trust.rs`, included by
//! build.rs which PANICS on any Err) is exercised here directly — a malformed
//! trust file must fail the build, never yield a permissive anchor.
//! OWNERS: @security @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: this file

#[path = "../../../userspace/updates/build_trust.rs"]
mod build_trust;

use build_trust::parse_update_trust;

const GOOD_KEY: &str = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";

#[test]
fn test_accept_narrow_format_with_comments() {
    let text =
        format!("# comment\n\n[[publisher]]\n# fixture\npubkey = \"{GOOD_KEY}\" # trailing\n");
    let keys = parse_update_trust(&text).expect("valid trust file parses");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0][0], 0xea);
    assert_eq!(keys[0][31], 0x2c);
}

#[test]
fn test_reject_empty_anchor() {
    // An empty anchor would brick every update path silently — build fails.
    assert!(parse_update_trust("# nothing here\n").is_err());
}

#[test]
fn test_reject_block_without_pubkey() {
    assert!(parse_update_trust("[[publisher]]\n").is_err());
}

#[test]
fn test_reject_bad_hex() {
    for bad in [
        "[[publisher]]\npubkey = \"deadbeef\"\n", // too short
        &format!("[[publisher]]\npubkey = \"{}\"\n", GOOD_KEY.to_uppercase()), // uppercase
        &format!("[[publisher]]\npubkey = \"{}zz\"\n", &GOOD_KEY[..62]), // non-hex
    ] {
        assert!(parse_update_trust(bad).is_err(), "must reject: {bad}");
    }
}

#[test]
fn test_reject_duplicate_and_stray_lines() {
    let dup_in_block = format!("[[publisher]]\npubkey = \"{GOOD_KEY}\"\npubkey = \"{GOOD_KEY}\"\n");
    assert!(parse_update_trust(&dup_in_block).is_err());
    let dup_across =
        format!("[[publisher]]\npubkey = \"{GOOD_KEY}\"\n[[publisher]]\npubkey = \"{GOOD_KEY}\"\n");
    assert!(parse_update_trust(&dup_across).is_err());
    let stray = format!("pubkey = \"{GOOD_KEY}\"\n");
    assert!(parse_update_trust(&stray).is_err(), "pubkey outside a block");
    assert!(parse_update_trust("[[publisher]]\nname = \"x\"\n").is_err(), "unknown field");
}
