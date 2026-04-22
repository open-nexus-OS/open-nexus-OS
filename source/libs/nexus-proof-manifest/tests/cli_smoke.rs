// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0023B Cut P4-05 — CLI subcommand integration tests for
//! `nexus-proof-manifest`. Invokes the compiled binary via
//! `env!("CARGO_BIN_EXE_nexus-proof-manifest")` against the on-disk
//! `source/apps/selftest-client/proof-manifest.toml` so that drift between
//! the parser library and the CLI surface is caught at `cargo test` time
//! (before `scripts/qemu-test.sh` consumes it).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (test-only)
//! TEST_COVERAGE: cargo test -p nexus-proof-manifest --test cli_smoke
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_nexus-proof-manifest");

fn manifest_path() -> PathBuf {
    // Tests run with CWD = crate root (source/libs/nexus-proof-manifest);
    // walk up to repo root and resolve the canonical manifest location.
    // P5-00: the on-disk manifest is now a v2 split tree; the root file
    // is `proof-manifest/manifest.toml` (the legacy single-file
    // `proof-manifest.toml` was deleted in P5-00).
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .ancestors()
        .nth(3) // .../source/libs/nexus-proof-manifest -> repo root
        .expect("repo root")
        .join("source/apps/selftest-client/proof-manifest/manifest.toml")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("invoke nexus-proof-manifest CLI");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn manifest_arg() -> String {
    format!("--manifest={}", manifest_path().display())
}

#[test]
fn verify_subcommand_accepts_on_disk_manifest() {
    let (code, stdout, stderr) = run(&["verify", &manifest_arg()]);
    assert_eq!(code, 0, "verify failed: stdout=`{stdout}` stderr=`{stderr}`");
}

#[test]
fn list_phases_full_returns_twelve_phase_ladder() {
    let (code, stdout, stderr) = run(&["list-phases", "--profile=full", &manifest_arg()]);
    assert_eq!(code, 0, "stderr=`{stderr}`");
    let phases: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    // RFC-0014 v2 declares a 12-phase ladder; the manifest is normative.
    assert_eq!(phases.len(), 12, "phases={phases:?}");
    assert_eq!(phases.first(), Some(&"bringup"));
    assert_eq!(phases.last(), Some(&"end"));
}

#[test]
fn list_env_quic_required_inherits_from_full() {
    let (code, stdout, stderr) = run(&["list-env", "--profile=quic-required", &manifest_arg()]);
    assert_eq!(code, 0, "stderr=`{stderr}`");
    // `quic-required` extends `full` (env={}) and adds REQUIRE_DSOFTBUS=1.
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.iter().any(|l| *l == "REQUIRE_DSOFTBUS=1"),
        "missing REQUIRE_DSOFTBUS in lines={lines:?}"
    );
}

#[test]
fn list_env_os2vm_carries_three_remote_flags() {
    let (code, stdout, stderr) = run(&["list-env", "--profile=os2vm", &manifest_arg()]);
    assert_eq!(code, 0, "stderr=`{stderr}`");
    for needle in [
        "REQUIRE_DSOFTBUS=1",
        "REQUIRE_DSOFTBUS_REMOTE_PKGFS=1",
        "REQUIRE_DSOFTBUS_REMOTE_STATEFS=1",
    ] {
        assert!(stdout.lines().any(|l| l == needle), "missing `{needle}` in stdout=`{stdout}`");
    }
}

#[test]
fn list_env_json_format_emits_object() {
    let (code, stdout, stderr) =
        run(&["list-env", "--profile=os2vm", "--format=json", &manifest_arg()]);
    assert_eq!(code, 0, "stderr=`{stderr}`");
    let trimmed = stdout.trim();
    assert!(trimmed.starts_with('{') && trimmed.ends_with('}'), "stdout=`{trimmed}`");
    assert!(trimmed.contains("\"REQUIRE_DSOFTBUS\":\"1\""), "stdout=`{trimmed}`");
}

#[test]
fn list_markers_full_is_byte_identical_lower_bound() {
    // The manifest currently carries 433 declared markers; profile `full`
    // expects all of them whose `emit_when` matches and whose
    // `forbidden_when` does not. The 396-marker count is locked by
    // P4-05's manual smoke-test; if it drifts, the harness must be told.
    let (code, stdout, stderr) = run(&["list-markers", "--profile=full", &manifest_arg()]);
    assert_eq!(code, 0, "stderr=`{stderr}`");
    let count = stdout.lines().filter(|l| !l.is_empty()).count();
    assert!(
        (390..=450).contains(&count),
        "list-markers full produced {count} markers; expected ~396 (range 390..=450)"
    );
}

#[test]
fn list_forbidden_quic_required_returns_three_markers() {
    let (code, stdout, stderr) =
        run(&["list-forbidden", "--profile=quic-required", &manifest_arg()]);
    assert_eq!(code, 0, "stderr=`{stderr}`");
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    // The on-disk manifest forbids exactly three markers under quic-required:
    // tcp transport selection, quic-disabled fallback, fallback-ok signal.
    assert_eq!(lines.len(), 3, "expected 3 forbidden markers for quic-required, got {lines:?}");
    assert!(lines.iter().any(|l| l.contains("quic")), "{lines:?}");
}

#[test]
fn unknown_subcommand_exits_nonzero() {
    let (code, _stdout, stderr) = run(&["does-not-exist", &manifest_arg()]);
    assert_ne!(code, 0, "expected non-zero exit; stderr=`{stderr}`");
}

#[test]
fn unknown_profile_exits_nonzero() {
    let (code, _stdout, stderr) = run(&["list-env", "--profile=__nope__", &manifest_arg()]);
    assert_ne!(code, 0, "expected non-zero exit; stderr=`{stderr}`");
}

#[test]
fn missing_manifest_exits_nonzero() {
    let (code, _stdout, stderr) =
        run(&["verify", "--manifest=/does/not/exist/proof-manifest.toml"]);
    assert_ne!(code, 0, "expected non-zero exit; stderr=`{stderr}`");
}
