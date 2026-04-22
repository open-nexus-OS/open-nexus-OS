// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0023B Cut P4-08 — runtime-only profile catalog.
//! Locks the on-disk `proof-manifest.toml` declarations of the five
//! `runtime_only = true` profiles (`bringup|quick|ota|net|none`) and the
//! `phases = [...]` schema extension. Mirrors the Rust dispatcher in
//! `source/apps/selftest-client/src/os_lite/profile.rs`; if either side
//! drifts (added/removed phase, renamed profile) the test must scream.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (test-only)
//! TEST_COVERAGE: cargo test -p nexus-proof-manifest --test runtime_profiles
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_proof_manifest::{parse, ParseError};

const PROLOGUE: &str = r#"[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1
[phase.ipc_kernel]
order = 2
[phase.mmio]
order = 3
[phase.end]
order = 4

[profile.full]
runner = "scripts/qemu-test.sh"
env = {}
"#;

#[test]
fn accept_runtime_only_profile_with_phase_subset() {
    let toml = format!(
        "{PROLOGUE}\n\
         [profile.quick]\n\
         runtime_only = true\n\
         phases = [\"bringup\", \"ipc_kernel\", \"end\"]\n"
    );
    let m = parse(&toml).expect("manifest parses");
    let p = m.profiles.get("quick").expect("quick profile present");
    assert!(p.runtime_only);
    assert!(p.runner.is_none());
    assert_eq!(p.phases, vec!["bringup".to_string(), "ipc_kernel".to_string(), "end".to_string()]);
}

#[test]
fn reject_phases_field_on_harness_profile() {
    let toml = format!(
        "{PROLOGUE}\n\
         [profile.busted]\n\
         runner = \"scripts/qemu-test.sh\"\n\
         phases = [\"bringup\", \"end\"]\n"
    );
    let err = parse(&toml).expect_err("harness profile with phases must reject");
    match err {
        ParseError::ProfileBodyInvalid { profile, detail } => {
            assert_eq!(profile, "busted");
            assert!(detail.contains("phases"), "detail should mention `phases`: {detail}");
        }
        other => panic!("expected ProfileBodyInvalid, got {other:?}"),
    }
}

#[test]
fn reject_runtime_profile_with_unknown_phase_reference() {
    let toml = format!(
        "{PROLOGUE}\n\
         [profile.broken]\n\
         runtime_only = true\n\
         phases = [\"bringup\", \"does_not_exist\", \"end\"]\n"
    );
    let err = parse(&toml).expect_err("unknown phase ref must reject");
    match err {
        ParseError::ProfileBodyInvalid { profile, detail } => {
            assert_eq!(profile, "broken");
            assert!(detail.contains("does_not_exist"), "detail=`{detail}`");
        }
        other => panic!("expected ProfileBodyInvalid, got {other:?}"),
    }
}

#[test]
fn on_disk_manifest_declares_all_five_runtime_profiles() {
    // The on-disk manifest is the SSOT consumed by both the harness
    // (`qemu-test.sh`) and the runtime dispatcher (`os_lite::profile`).
    // If a profile name disappears, every consumer must update in lockstep.
    // P5-00: on-disk manifest is now a v2 split tree.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root")
        .join("source/apps/selftest-client/proof-manifest/manifest.toml");
    let m = nexus_proof_manifest::parse_path(&path).expect("on-disk manifest parses");

    for name in ["bringup", "quick", "ota", "net", "none"] {
        let p = m.profiles.get(name).unwrap_or_else(|| panic!("missing runtime profile `{name}`"));
        assert!(p.runtime_only, "{name} must be runtime_only");
        assert!(p.runner.is_none(), "{name} must NOT carry a runner");
        assert!(!p.phases.is_empty(), "{name} must declare a non-empty phases subset");
        for ph in &p.phases {
            assert!(
                m.phases.contains_key(ph),
                "{name}: phase ref `{ph}` does not match any [phase.X]"
            );
        }
    }

    // Sanity: harness profiles must NOT carry `phases`.
    for name in ["full", "smp", "dhcp", "os2vm", "quic-required"] {
        let p = m.profiles.get(name).expect("harness profile present");
        assert!(p.phases.is_empty(), "{name}: harness profile must not carry `phases`");
        assert!(!p.runtime_only, "{name}: harness profile must not be runtime_only");
    }
}

#[test]
fn on_disk_manifest_declares_all_twelve_skip_markers() {
    // P4-08 contract: the manifest declares one `dbg: phase X skipped`
    // marker per [phase.X] (12 in total). Generated as `M_DBG_PHASE_*`
    // constants in `markers_generated.rs` and consumed by
    // `os_lite::profile::Profile::skip_marker`.
    // P5-00: on-disk manifest is now a v2 split tree.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root")
        .join("source/apps/selftest-client/proof-manifest/manifest.toml");
    let m = nexus_proof_manifest::parse_path(&path).expect("on-disk manifest parses");

    for ph in [
        "bringup",
        "ipc_kernel",
        "mmio",
        "routing",
        "ota",
        "policy",
        "exec",
        "logd",
        "vfs",
        "net",
        "remote",
        "end",
    ] {
        let lit = format!("dbg: phase {ph} skipped");
        let found = m.markers.iter().find(|mk| mk.literal == lit);
        let mk = found.unwrap_or_else(|| panic!("missing skip marker `{lit}`"));
        assert_eq!(mk.phase, ph, "skip marker phase must match owning phase");
    }
}
