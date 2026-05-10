// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Cut P4-03 marker-schema acceptance + reject suite. Adds the
//! four `[marker."…"]` reject categories plus an on-disk accept test that
//! pins the populated manifest's marker count, so any future drop / add
//! surfaces as a test break instead of a silent ladder change.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (Phase-4 invariant: variant set never shrinks)
//! TEST_COVERAGE: 5 tests (1 accept + 4 reject)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_proof_manifest::{parse, ParseError};

const PROLOGUE: &str = r#"
[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1

[profile.full]
[profile."quic-required"]
"#;

#[test]
fn accept_on_disk_manifest_marker_count_and_const_keys() {
    // Cross-check: the populated on-disk manifest in `selftest-client/`
    // must parse, declare its full marker set, and produce unique
    // `const_key()` values so that `markers_generated.rs` cannot have
    // colliding `M_*` constants.
    // P5-00: on-disk manifest is now a v2 split tree; parse via parse_path
    // so the CLI dispatch path is exercised end-to-end.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/selftest-client/proof-manifest/manifest.toml"
    );
    let m = nexus_proof_manifest::parse_path(std::path::Path::new(path))
        .unwrap_or_else(|e| panic!("populated on-disk manifest must parse: {e}"));

    // Lower bound: P4-03 declares 179 gating markers; P4-04 back-fills the
    // remaining ~254 diagnostic / fragment / FAIL labels so that
    // `markers_generated.rs` is the SSOT for *every* `SELFTEST: …` /
    // `dsoftbusd: …` / `dsoftbus: …` literal in the source tree (~430+).
    // Allow growth (markers are append-only across cuts) but flag a drop.
    assert!(
        m.markers.len() >= 430,
        "manifest must declare >= 430 markers (got {})",
        m.markers.len()
    );

    // Every marker's const_key must be unique — collision would make two
    // `M_<KEY>` constants overwrite each other in `markers_generated.rs`.
    let mut keys = std::collections::HashSet::new();
    for marker in m.markers() {
        let k = marker.const_key();
        assert!(
            keys.insert(k.clone()),
            "duplicate const_key `{k}` from marker `{}`",
            marker.literal
        );
        assert!(
            k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
            "const_key `{k}` must be UPPER_SNAKE_CASE (from marker `{}`)",
            marker.literal
        );
        assert!(
            !k.is_empty() && !k.starts_with('_') && !k.ends_with('_'),
            "const_key `{k}` must not have leading/trailing/empty underscores"
        );
    }

    // Every marker phase must reference a declared `[phase.X]` (parser
    // already enforces this; double-check via a positive assertion so a
    // future loosening surfaces here too).
    for marker in m.markers() {
        assert!(
            m.phases.contains_key(&marker.phase),
            "marker `{}` phase `{}` not declared in manifest",
            marker.literal,
            marker.phase
        );
    }
}

#[test]
fn reject_marker_missing_phase() {
    let src = format!(
        r#"{PROLOGUE}
[marker."SELFTEST: oops no phase"]
proves = "this should reject"
"#
    );
    let err = parse(&src).expect_err("marker without phase must reject");
    assert!(
        matches!(
            err,
            ParseError::MarkerMissingPhase(ref m) if m == "SELFTEST: oops no phase"
        ),
        "expected MarkerMissingPhase, got {err:?}"
    );
}

#[test]
fn reject_marker_unknown_phase() {
    let src = format!(
        r#"{PROLOGUE}
[marker."SELFTEST: bad phase ref"]
phase = "ghost_phase"
"#
    );
    let err = parse(&src).expect_err("marker referencing undeclared phase must reject");
    match err {
        ParseError::MarkerUnknownPhase { marker, phase } => {
            assert_eq!(marker, "SELFTEST: bad phase ref");
            assert_eq!(phase, "ghost_phase");
        }
        other => panic!("expected MarkerUnknownPhase, got {other:?}"),
    }
}

#[test]
fn reject_marker_unknown_profile_in_emit_when() {
    let src = format!(
        r#"{PROLOGUE}
[marker."SELFTEST: bad profile ref"]
phase = "bringup"
emit_when = {{ profile = "ghost_profile" }}
"#
    );
    let err = parse(&src).expect_err("marker referencing undeclared profile must reject");
    match err {
        ParseError::MarkerUnknownProfile { marker, profile, clause } => {
            assert_eq!(marker, "SELFTEST: bad profile ref");
            assert_eq!(profile, "ghost_profile");
            assert_eq!(clause, "emit_when");
        }
        other => panic!("expected MarkerUnknownProfile(emit_when), got {other:?}"),
    }
}

#[test]
fn reject_marker_unknown_profile_in_forbidden_when() {
    let src = format!(
        r#"{PROLOGUE}
[marker."SELFTEST: bad forbidden profile"]
phase = "bringup"
forbidden_when = {{ profile = "ghost_profile" }}
"#
    );
    let err = parse(&src).expect_err("forbidden_when referencing undeclared profile must reject");
    match err {
        ParseError::MarkerUnknownProfile { marker, profile, clause } => {
            assert_eq!(marker, "SELFTEST: bad forbidden profile");
            assert_eq!(profile, "ghost_profile");
            assert_eq!(clause, "forbidden_when");
        }
        other => panic!("expected MarkerUnknownProfile(forbidden_when), got {other:?}"),
    }
}
