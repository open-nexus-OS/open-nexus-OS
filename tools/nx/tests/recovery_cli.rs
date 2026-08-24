// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for `nx recovery token-make/-show`
//! (TASK-0053, RFC-0088): a token minted from the proof seed verifies as
//! trusted against the image-baked anchor, a stranger key reports its
//! reject verdict as DATA (nx exit stays success for a decodable token),
//! and malformed inputs map to the stable exit classes.
//! OWNERS: @security @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 integration tests
//! ADR: docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md

use std::path::Path;
use std::process::{Command, Output};

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout must be valid json")
}

#[test]
fn make_then_show_reports_trusted_for_the_proof_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Proof seed [42u8; 32] — its verifying key sits in the baked anchor.
    std::fs::write(dir.path().join("key.hex"), "2a".repeat(32)).expect("write key");
    let make = run_nx(
        &[
            "recovery",
            "token-make",
            "--key",
            "key.hex",
            "--action",
            "target-set",
            "--seq",
            "7",
            "--arg",
            "1",
            "--json",
        ],
        dir.path(),
    );
    assert!(make.status.success(), "stderr: {}", String::from_utf8_lossy(&make.stderr));
    let made = stdout_json(&make);
    assert_eq!(made["data"]["keyid"], "197f6b23");
    assert_eq!(std::fs::metadata(dir.path().join("token.nxra")).expect("token").len(), 136);

    let show = run_nx(&["recovery", "token-show", "token.nxra", "--json"], dir.path());
    assert!(show.status.success());
    let shown = stdout_json(&show);
    assert_eq!(shown["data"]["verdict"], "trusted");
    assert_eq!(shown["data"]["action"], "target-set");
    assert_eq!(shown["data"]["seq"], 7);
}

#[test]
fn show_reports_untrusted_key_as_data() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("stranger.hex"), "0d".repeat(32)).expect("write key");
    let make = run_nx(
        &["recovery", "token-make", "--key", "stranger.hex", "--action", "reset", "--seq", "1"],
        dir.path(),
    );
    assert!(make.status.success());
    let show = run_nx(&["recovery", "token-show", "token.nxra", "--json"], dir.path());
    assert!(show.status.success(), "decodable token = success; verdict is data");
    assert_eq!(stdout_json(&show)["data"]["verdict"], "untrusted-key");
}

#[test]
fn test_reject_malformed_token_maps_validation_reject() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("junk.nxra"), b"not a token").expect("write");
    let out = run_nx(&["recovery", "token-show", "junk.nxra"], dir.path());
    assert_eq!(out.status.code(), Some(3), "malformed token = validation_reject");
}

#[test]
fn test_reject_bad_key_file_and_action() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("short.hex"), "abcd").expect("write");
    let out = run_nx(
        &["recovery", "token-make", "--key", "short.hex", "--action", "reset", "--seq", "1"],
        dir.path(),
    );
    assert_eq!(out.status.code(), Some(3), "bad seed = validation_reject");
    std::fs::write(dir.path().join("key.hex"), "2a".repeat(32)).expect("write");
    let out = run_nx(
        &["recovery", "token-make", "--key", "key.hex", "--action", "fly", "--seq", "1"],
        dir.path(),
    );
    assert_eq!(out.status.code(), Some(2), "unknown action = usage");
}
