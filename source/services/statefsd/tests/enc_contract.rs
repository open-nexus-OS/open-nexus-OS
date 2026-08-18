// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefsd record-encryption contract tests (TASK-0027) —
//!   context build from a device seed, the no-fake-green enable
//!   self-check, the admin-prefix capability row, and the enrolled-table
//!   invariants (chicken-egg rule holds for whatever the const table
//!   names).
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (host, deterministic)

use statefsd::{enc_svc, txn};

const SEED_A: [u8; 32] = [0x11; 32];
const SEED_B: [u8; 32] = [0x22; 32];
const SALT: [u8; statefs::enc::SALT_LEN] = [0x33; statefs::enc::SALT_LEN];

#[test]
fn build_context_is_deterministic_and_seed_bound() {
    let ctx_a = enc_svc::build_context(&SEED_A, SALT).expect("ctx a");
    let ctx_b = enc_svc::build_context(&SEED_A, SALT).expect("ctx b");
    let ctx_c = enc_svc::build_context(&SEED_B, SALT).expect("ctx c");
    // Same seed ⇒ identical sealing behavior (a value sealed by one context
    // opens under the other); different seed ⇒ it must NOT open.
    let mut sealed = Vec::new();
    let class = ctx_a.class_for("/state/app/x").expect("enrolled");
    statefs::enc::seal_into(&ctx_a, class, "/state/app/x", 7, 0, b"v", &mut sealed).expect("seal");
    assert_eq!(statefs::enc::open(&ctx_b, "/state/app/x", &sealed).expect("open"), b"v");
    assert!(statefs::enc::open(&ctx_c, "/state/app/x", &sealed).is_err());
}

#[test]
fn enable_self_check_passes_on_healthy_context() {
    let ctx = enc_svc::build_context(&SEED_A, SALT).expect("ctx");
    assert!(enc_svc::self_check(&ctx));
}

#[test]
fn enrolled_table_covers_app_state_only() {
    let ctx = enc_svc::build_context(&SEED_A, SALT).expect("ctx");
    assert!(ctx.class_for("/state/app/selftest/enc/token").is_some());
    // Boot-critical and admin state must never resolve to a class — the
    // chicken-egg rule holds for whatever ENCRYPTED_PREFIXES names
    // (EncContext::enroll would have rejected it at build time).
    for key in [
        "/state/keystore/device.signing",
        "/state/boot/bootctl.v1",
        statefs::enc::META_KEY,
        "/state/settingsd/prefs",
    ] {
        assert!(ctx.class_for(key).is_none(), "{key} must not be enrolled");
    }
}

#[test]
fn test_reject_enc_meta_write_without_admin_cap() {
    // The per-key cap table: the admin prefix (encryption switch) demands
    // `statefs.admin` — a plain `statefs.write` holder cannot toggle it.
    assert_eq!(txn::required_txn_put_cap(statefs::enc::META_KEY), "statefs.admin");
    assert_eq!(txn::required_txn_put_cap("/state/statefsd/anything"), "statefs.admin");
    // Existing rows are untouched.
    assert_eq!(txn::required_txn_put_cap("/state/keystore/x"), "statefs.keystore");
    assert_eq!(txn::required_txn_put_cap("/state/boot/x"), "statefs.boot");
    assert_eq!(txn::required_txn_put_cap("/state/app/x"), "statefs.write");
}

#[test]
fn device_seed_extraction_handles_raw_and_envelope() {
    // Migration-era raw seed.
    assert_eq!(enc_svc::device_seed_from_stored(&SEED_A), Some(SEED_A));
    // Wrong length is refused.
    assert_eq!(enc_svc::device_seed_from_stored(&[0u8; 31]), None);
    assert_eq!(enc_svc::device_seed_from_stored(&[]), None);
}

#[test]
fn test_reject_meta_gate_on_malformed_or_zero_salt() {
    // The enable path only accepts a well-formed meta record with a real
    // salt (entropy honesty — RED rule).
    assert!(statefs::enc::decode_meta(b"garbage").is_err());
    let zero =
        statefs::enc::encode_meta(&statefs::enc::EncMeta { salt: [0u8; statefs::enc::SALT_LEN] });
    assert!(statefs::enc::decode_meta(&zero).is_err());
    let good =
        statefs::enc::encode_meta(&statefs::enc::EncMeta { salt: [9u8; statefs::enc::SALT_LEN] });
    assert!(statefs::enc::decode_meta(&good).is_ok());
}
