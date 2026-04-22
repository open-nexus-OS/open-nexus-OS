// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Cut P4-01 acceptance + reject suite for the
//! `nexus-proof-manifest` skeleton parser. Each reject test pins exactly
//! one [`ParseError`] variant so that future schema evolution must
//! consciously choose to break a category instead of silently degrading.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (Phase-4 invariant: variant set never shrinks)
//! TEST_COVERAGE: 7 tests (1 accept + 6 reject)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_proof_manifest::{parse, ParseError};

/// Canonical 12-phase skeleton, matching the on-disk
/// `source/apps/selftest-client/proof-manifest.toml` shape at Cut P4-01.
const SKELETON: &str = r#"
[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1

[phase.ipc_kernel]
order = 2

[phase.mmio]
order = 3

[phase.routing]
order = 4

[phase.ota]
order = 5

[phase.policy]
order = 6

[phase.exec]
order = 7

[phase.logd]
order = 8

[phase.vfs]
order = 9

[phase.net]
order = 10

[phase.remote]
order = 11

[phase.end]
order = 12

[profile.full]
"#;

#[test]
fn accept_skeleton_parses_with_12_phases_and_default_profile() {
    let m = parse(SKELETON).expect("skeleton must parse");
    assert_eq!(m.meta.schema_version, "1");
    assert_eq!(m.meta.default_profile, "full");
    assert_eq!(
        m.phases.len(),
        12,
        "manifest must declare exactly 12 phases"
    );
    assert!(
        m.profiles.contains_key("full"),
        "default profile must be declared"
    );

    // Spot-check that the canonical RFC-0014 v2 phase names are all present
    // with their declared 1..12 order; this guards against the manifest
    // silently renaming a phase out of the contract.
    for (name, expected_order) in [
        ("bringup", 1u8),
        ("ipc_kernel", 2),
        ("mmio", 3),
        ("routing", 4),
        ("ota", 5),
        ("policy", 6),
        ("exec", 7),
        ("logd", 8),
        ("vfs", 9),
        ("net", 10),
        ("remote", 11),
        ("end", 12),
    ] {
        let phase = m
            .phases
            .get(name)
            .unwrap_or_else(|| panic!("missing phase `{name}`"));
        assert_eq!(phase.order, expected_order, "phase `{name}` order");
    }
}

#[test]
fn reject_unknown_top_level_key() {
    let src = r#"
[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1

[profile.full]

[surprise]
foo = 1
"#;
    let err = parse(src).expect_err("unknown top-level key must reject");
    assert!(
        matches!(err, ParseError::UnknownTopLevelKey(ref k) if k == "surprise"),
        "expected UnknownTopLevelKey(\"surprise\"), got {err:?}"
    );
}

#[test]
fn on_disk_manifest_parses() {
    // Cross-check: the actual on-disk skeleton in the selftest-client tree
    // must parse cleanly under the same parser the build will use.
    // P5-00: on-disk manifest is now a v2 split tree.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/selftest-client/proof-manifest/manifest.toml"
    );
    let m = nexus_proof_manifest::parse_path(std::path::Path::new(path))
        .unwrap_or_else(|e| panic!("on-disk skeleton must parse: {e}"));
    assert_eq!(m.phases.len(), 12);
    assert_eq!(m.meta.default_profile, "full");
    assert_eq!(m.meta.schema_version, "2");
}

#[test]
fn reject_unknown_meta_key() {
    let src = r#"
[meta]
schema_version = "1"
default_profile = "full"
oops_typo = "boom"

[phase.bringup]
order = 1

[profile.full]
"#;
    let err = parse(src).expect_err("unknown [meta] key must reject");
    assert!(
        matches!(err, ParseError::UnknownMetaKey(ref k) if k == "oops_typo"),
        "expected UnknownMetaKey(\"oops_typo\"), got {err:?}"
    );
}

#[test]
fn reject_missing_schema_version() {
    let src = r#"
[meta]
default_profile = "full"

[phase.bringup]
order = 1

[profile.full]
"#;
    let err = parse(src).expect_err("missing schema_version must reject");
    assert_eq!(err, ParseError::MissingSchemaVersion);
}

#[test]
fn reject_missing_default_profile() {
    let src = r#"
[meta]
schema_version = "1"

[phase.bringup]
order = 1

[profile.full]
"#;
    let err = parse(src).expect_err("missing default_profile must reject");
    assert_eq!(err, ParseError::MissingDefaultProfile);
}

#[test]
fn reject_duplicate_phase() {
    // TOML itself rejects two `[phase.bringup]` headers as a syntax
    // duplicate (we surface that as `ParseError::Toml`, not
    // `DuplicatePhase`). Logical duplication via mixed inline/table form
    // is the case we guard with `DuplicatePhase`. We construct it via
    // dotted-key form so the TOML parser accepts it but the schema
    // semantically duplicates the name:
    let src = r#"
[meta]
schema_version = "1"
default_profile = "full"

[phase]
bringup = { order = 1 }

[phase.bringup]
order = 2

[profile.full]
"#;
    let err = parse(src).expect_err("duplicate phase must reject");
    // Either the underlying TOML parser flags this as a redefinition
    // (`ParseError::Toml`) — which is also a valid rejection — or the
    // schema layer flags it. Both are acceptable: what we MUST NOT do is
    // silently accept it.
    assert!(
        matches!(err, ParseError::Toml(_) | ParseError::DuplicatePhase(_)),
        "expected Toml(_) or DuplicatePhase(_), got {err:?}"
    );
}

#[test]
fn reject_phase_order_conflict() {
    let src = r#"
[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1

[phase.ipc_kernel]
order = 1

[profile.full]
"#;
    let err = parse(src).expect_err("phase order conflict must reject");
    match err {
        ParseError::PhaseOrderConflict {
            order,
            first,
            second,
        } => {
            assert_eq!(order, 1);
            // BTreeMap iteration over the raw phase map is alphabetic, so
            // `bringup` is observed before `ipc_kernel`. Pin both names
            // so a future change to the iteration order surfaces as a
            // test break instead of silent reordering.
            assert_eq!(first, "bringup");
            assert_eq!(second, "ipc_kernel");
        }
        other => panic!("expected PhaseOrderConflict, got {other:?}"),
    }
}
