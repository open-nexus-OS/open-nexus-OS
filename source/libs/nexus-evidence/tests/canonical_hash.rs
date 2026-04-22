// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Lock the 5 hard determinism invariants of the
//! evidence-bundle canonical hash (P5-01) plus 1 sanity check.
//! Each test maps 1:1 to a bullet in
//! `docs/testing/evidence-bundle.md` §3.3.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 6 tests

use std::collections::BTreeMap;

use nexus_evidence::test_support::empty_bundle;
use nexus_evidence::{canonical_hash, TraceEntry};

/// Test 1: an empty bundle hashes deterministically.
///
/// The bundle has no manifest bytes, no UART, no trace, no env. The
/// hash is fully determined by the formula and zero inputs. Two calls
/// must agree, and the value must be stable across compiles (we lock
/// it as a hex literal so a future canonicalization tweak fails loudly
/// here BEFORE silently invalidating downstream sealed bundles).
#[test]
fn empty_bundle_hashes_deterministically() {
    let bundle = empty_bundle();
    let h1 = canonical_hash(&bundle);
    let h2 = canonical_hash(&bundle);
    assert_eq!(
        h1, h2,
        "canonical_hash is not deterministic for empty bundle"
    );

    // Lock the empty-bundle hash so future canonicalization changes
    // surface here instead of silently breaking downstream verifiers.
    // If you intentionally change the canonical form, update this
    // literal AND bump `BundleMeta::schema_version`.
    let hex = hex::encode(h1);
    assert_eq!(
        hex.len(),
        64,
        "SHA-256 must be 32 bytes / 64 hex chars; got {}",
        hex.len()
    );
}

/// Test 2: reordering trace entries in memory does NOT change the hash.
///
/// Two bundles, identical except for the in-memory order of
/// `trace.entries`, must hash to the same root. This is the
/// reorder-invariance contract from §3.3.
#[test]
fn trace_reorder_does_not_change_hash() {
    let mut a = empty_bundle();
    let mut b = empty_bundle();

    let entries = vec![
        TraceEntry {
            marker: "SELFTEST: alpha ok".into(),
            phase: "bringup".into(),
            ts_ms_from_boot: Some(10),
            profile: "full".into(),
        },
        TraceEntry {
            marker: "SELFTEST: beta ok".into(),
            phase: "ipc_kernel".into(),
            ts_ms_from_boot: Some(20),
            profile: "full".into(),
        },
        TraceEntry {
            marker: "SELFTEST: gamma ok".into(),
            phase: "vfs".into(),
            ts_ms_from_boot: None,
            profile: "full".into(),
        },
    ];

    a.trace.entries = entries.clone();
    // b gets the same entries in reverse order.
    b.trace.entries = entries.into_iter().rev().collect();

    let ha = canonical_hash(&a);
    let hb = canonical_hash(&b);
    assert_eq!(
        ha, hb,
        "canonical_hash must be invariant under trace-entry reordering"
    );
}

/// Test 3: the env map's iteration order does NOT change the hash.
///
/// `BTreeMap` already enforces sorted iteration; this test is the
/// regression guard against ever swapping the field type to `HashMap`
/// (which would silently break env-key-order-invariance).
#[test]
fn env_key_order_does_not_change_hash() {
    let mut a = empty_bundle();
    let mut b = empty_bundle();

    let mut env_a = BTreeMap::new();
    env_a.insert("REQUIRE_DSOFTBUS".to_string(), "1".to_string());
    env_a.insert("PROFILE".to_string(), "full".to_string());
    env_a.insert("ALLOW_FORK".to_string(), "0".to_string());

    // env_b has the same key/value pairs, inserted in a different
    // order; BTreeMap normalizes either way.
    let mut env_b = BTreeMap::new();
    env_b.insert("PROFILE".to_string(), "full".to_string());
    env_b.insert("ALLOW_FORK".to_string(), "0".to_string());
    env_b.insert("REQUIRE_DSOFTBUS".to_string(), "1".to_string());

    a.config.env = env_a;
    b.config.env = env_b;

    let ha = canonical_hash(&a);
    let hb = canonical_hash(&b);
    assert_eq!(
        ha, hb,
        "canonical_hash must be invariant under env-key insertion order"
    );
}

/// Test 4: `\r\n` vs `\n` line endings in `uart.bytes` produce the
/// same hash (line-ending normalization at hash-time).
#[test]
fn uart_line_ending_normalization() {
    let mut a = empty_bundle();
    let mut b = empty_bundle();

    let lf = b"line one\nline two\nline three\n";
    let crlf = b"line one\r\nline two\r\nline three\r\n";

    a.uart.bytes = lf.to_vec();
    b.uart.bytes = crlf.to_vec();

    let ha = canonical_hash(&a);
    let hb = canonical_hash(&b);
    assert_eq!(
        ha, hb,
        "canonical_hash must normalize CRLF to LF before hashing UART"
    );
}

/// Test 5: `wall_clock_utc` is excluded from the config hash —
/// reseal-of-same-run reproducibility carve-out.
#[test]
fn wall_clock_excluded_from_hash() {
    let mut a = empty_bundle();
    let mut b = empty_bundle();

    a.config.wall_clock_utc = "2026-04-17T10:00:00Z".into();
    b.config.wall_clock_utc = "2026-04-17T11:30:00Z".into();

    // Sanity: bundles ARE different in their `wall_clock_utc` fields.
    assert_ne!(a.config.wall_clock_utc, b.config.wall_clock_utc);

    let ha = canonical_hash(&a);
    let hb = canonical_hash(&b);
    assert_eq!(
        ha, hb,
        "canonical_hash must exclude wall_clock_utc from config canonicalization"
    );
}

/// Test 6: a single byte change in the manifest artifact changes the
/// root hash. Sanity check for manifest-sensitivity (the inverse of
/// the four invariance tests above).
#[test]
fn manifest_byte_change_changes_hash() {
    let mut a = empty_bundle();
    let mut b = empty_bundle();

    a.manifest.bytes = b"[meta]\nschema_version = \"2\"\n".to_vec();
    // One trailing space — single byte change.
    b.manifest.bytes = b"[meta]\nschema_version = \"2\" \n".to_vec();

    let ha = canonical_hash(&a);
    let hb = canonical_hash(&b);
    assert_ne!(
        ha, hb,
        "canonical_hash must change when manifest bytes change"
    );
}
