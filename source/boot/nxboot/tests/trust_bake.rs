// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Trust-bake integration proof (TASK-0289 A1). Asserts the
//! anchor nxboot was BUILT with is exactly what `policies/os-trust.toml`
//! says, and that the anchor actually matches the dev OS-image signing
//! seed `nx image build --sign` uses — a drift between the three would
//! surface here on the host, long before a QEMU lane refuses to boot.
//! OWNERS: @security
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

use std::path::PathBuf;

// The same narrow parser build.rs bakes with (single authority).
include!("../../../../userspace/updates/build_trust.rs");

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").join(rel)
}

#[test]
fn baked_anchor_matches_the_policy_file() {
    let text = std::fs::read_to_string(repo_path("policies/os-trust.toml")).expect("os-trust.toml");
    let parsed = parse_update_trust(&text).expect("os-trust.toml parses");
    assert_eq!(parsed.as_slice(), nxboot::trust::BAKED_OS_KEYS, "bake drifted from policy file");
}

#[test]
fn baked_anchor_matches_the_dev_signing_seed() {
    let seed_hex = std::fs::read_to_string(repo_path("keys/dev-os-image.ed25519.seed"))
        .expect("dev-os-image seed");
    let seed_hex = seed_hex.trim();
    assert_eq!(seed_hex.len(), 64, "seed file must hold 64 hex chars");
    let mut seed = [0u8; 32];
    for (i, slot) in seed.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&seed_hex[i * 2..i * 2 + 2], 16).expect("hex");
    }
    let (pubkey, _id) = bootfmt::nxbd::pubkey_id_for_seed(&seed);
    assert!(
        nxboot::trust::BAKED_OS_KEYS.contains(&pubkey),
        "nx image signs with a key the loader would not accept"
    );
}

#[test]
fn signed_nxbd_verifies_against_the_baked_anchor_and_tamper_fails() {
    let seed_hex = std::fs::read_to_string(repo_path("keys/dev-os-image.ed25519.seed"))
        .expect("dev-os-image seed");
    let mut seed = [0u8; 32];
    for (i, slot) in seed.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&seed_hex.trim()[i * 2..i * 2 + 2], 16).expect("hex");
    }
    let (_pk, id) = bootfmt::nxbd::pubkey_id_for_seed(&seed);
    let desc = bootfmt::nxbd::Nxbd {
        rollback_index: 1,
        image_size: 1024,
        image_sha256: [0x11; 32],
        build_id: bootfmt::nxbd::Nxbd::build_id_from("a1-test"),
        load_addr: 0x8020_0000,
        pubkey_id: id,
    };
    let sector = bootfmt::nxbd::sign(&desc, &seed);
    assert_eq!(nxboot::trust::verify_nxbd(&sector).expect("verify"), desc);

    let mut bad = sector;
    bad[12] ^= 1;
    assert!(nxboot::trust::verify_nxbd(&bad).is_err(), "tampered NXBD must not verify");
    // A key OUTSIDE the anchor set signs a well-formed sector: still `sig`.
    let stranger = bootfmt::nxbd::sign(&desc, &[0x42; 32]);
    assert!(nxboot::trust::verify_nxbd(&stranger).is_err(), "stranger key must not verify");
}

#[test]
fn parser_fails_closed_on_malformed_trust_files() {
    // These are the exact inputs that make the BUILD fail (build.rs panics
    // on Err) — asserted here at the shared-function level.
    assert!(parse_update_trust("[[publisher]]\n").is_err(), "block without pubkey");
    assert!(parse_update_trust("[[publisher]]\npubkey = \"AB\"\n").is_err(), "short/uppercase hex");
    assert!(parse_update_trust("pubkey = \"aa\"\n").is_err(), "pubkey outside block");
    let dup = "[[publisher]]\npubkey = \"".to_string()
        + &"ab".repeat(32)
        + "\"\n[[publisher]]\npubkey = \""
        + &"ab".repeat(32)
        + "\"\n";
    assert!(parse_update_trust(&dup).is_err(), "duplicate key");
}
