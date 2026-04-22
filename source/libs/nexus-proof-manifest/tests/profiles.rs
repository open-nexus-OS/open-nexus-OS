// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0023B Cut P4-05 — accept + reject suite for harness
//! profile resolution. Pins the inheritance chain semantics
//! (`extends` flattening, child shadowing, cycle rejection) and locks the
//! on-disk manifest's `[profile.full|smp|dhcp|os2vm|quic-required]`
//! catalog so that `qemu-test.sh` / `tools/os2vm.sh` can rely on it.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (test-only)
//! TEST_COVERAGE: cargo test -p nexus-proof-manifest --test profiles
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_proof_manifest::{parse, ParseError};

const PROLOGUE: &str = r#"[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1
"#;

#[test]
fn accept_on_disk_catalog() {
    // P5-00: on-disk manifest is a v2 split tree under `proof-manifest/`.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/selftest-client/proof-manifest/manifest.toml"
    );
    let m = nexus_proof_manifest::parse_path(std::path::Path::new(path))
        .unwrap_or_else(|e| panic!("on-disk manifest must parse: {e}"));

    for required in ["full", "smp", "dhcp", "os2vm", "quic-required"] {
        assert!(
            m.profiles.contains_key(required),
            "manifest must declare profile `{required}`"
        );
    }

    // `quic-required` extends `full` and adds REQUIRE_DSOFTBUS=1.
    let env = m
        .resolve_env_chain("quic-required")
        .expect("quic-required env resolution");
    assert_eq!(env.get("REQUIRE_DSOFTBUS").map(String::as_str), Some("1"));

    // `os2vm` extends `full` and inherits the dsoftbus remote env trio.
    let os2vm_env = m.resolve_env_chain("os2vm").expect("os2vm env resolution");
    for k in [
        "REQUIRE_DSOFTBUS",
        "REQUIRE_DSOFTBUS_REMOTE_PKGFS",
        "REQUIRE_DSOFTBUS_REMOTE_STATEFS",
    ] {
        assert_eq!(
            os2vm_env.get(k).map(String::as_str),
            Some("1"),
            "os2vm env must carry {k}=1"
        );
    }

    // `full` profile expected ladder excludes `forbidden_when=quic-required`
    // markers (they only become forbidden under that profile).
    let quic_expected: Vec<&str> = m
        .expected_markers("quic-required")
        .map(|m| m.literal.as_str())
        .collect();
    let quic_forbidden: Vec<&str> = m
        .forbidden_markers("quic-required")
        .map(|m| m.literal.as_str())
        .collect();
    assert!(
        !quic_forbidden.is_empty(),
        "quic-required must declare forbidden markers"
    );
    for forb in &quic_forbidden {
        assert!(
            !quic_expected.contains(forb),
            "marker `{forb}` is forbidden under quic-required, must not appear in expected list"
        );
    }
}

#[test]
fn accept_extends_chain_flattens_with_child_shadow() {
    let src = format!(
        "{PROLOGUE}\n\
        [profile.parent]\n\
        runner = \"scripts/run.sh\"\n\
        env = {{ A = \"parent\", B = \"parent\" }}\n\
        \n\
        [profile.child]\n\
        extends = \"parent\"\n\
        env = {{ B = \"child\", C = \"child\" }}\n"
    );
    let m = parse(&src).expect("parse");
    let env = m.resolve_env_chain("child").expect("resolve");
    assert_eq!(env.get("A").map(String::as_str), Some("parent"));
    assert_eq!(env.get("B").map(String::as_str), Some("child"));
    assert_eq!(env.get("C").map(String::as_str), Some("child"));
}

#[test]
fn reject_extends_unknown_parent() {
    let src = format!(
        "{PROLOGUE}\n\
        [profile.full]\n\
        \n\
        [profile.child]\n\
        extends = \"missing\"\n"
    );
    let err = parse(&src).expect_err("must reject");
    assert!(
        matches!(err, ParseError::ProfileUnknownParent { .. }),
        "expected ProfileUnknownParent, got {err:?}"
    );
}

#[test]
fn reject_extends_cycle_two_node() {
    let src = format!(
        "{PROLOGUE}\n\
        [profile.full]\n\
        \n\
        [profile.a]\n\
        extends = \"b\"\n\
        \n\
        [profile.b]\n\
        extends = \"a\"\n"
    );
    let err = parse(&src).expect_err("must reject");
    assert!(
        matches!(err, ParseError::ProfileExtendsCycle(_)),
        "expected ProfileExtendsCycle, got {err:?}"
    );
}

#[test]
fn reject_runtime_only_profile_with_runner() {
    let src = format!(
        "{PROLOGUE}\n\
        [profile.full]\n\
        \n\
        [profile.bringup]\n\
        runtime_only = true\n\
        runner = \"scripts/run.sh\"\n"
    );
    let err = parse(&src).expect_err("must reject");
    assert!(
        matches!(err, ParseError::ProfileRuntimeOnlyWithRunner(ref p) if p == "bringup"),
        "expected ProfileRuntimeOnlyWithRunner, got {err:?}"
    );
}

#[test]
fn reject_unknown_profile_body_key() {
    let src = format!(
        "{PROLOGUE}\n\
        [profile.full]\n\
        oops = \"typo\"\n"
    );
    let err = parse(&src).expect_err("must reject");
    assert!(
        matches!(err, ParseError::ProfileBodyInvalid { ref profile, .. } if profile == "full"),
        "expected ProfileBodyInvalid, got {err:?}"
    );
}
