// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary contract tests for the canonical `nx` CLI, including `nx config` behavior.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 8 integration tests.
//!
//! TEST_SCOPE:
//!   - Process exit-class and JSON-envelope contracts
//!   - Deterministic filesystem effects for scaffold/config commands
//!   - Fail-closed CLI behavior at the real process boundary
//!
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run_nx(args: &[&str], cwd: &Path, path_override: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nx"));
    command.args(args).current_dir(cwd);
    if let Some(path) = path_override {
        command.env("PATH", path);
    }
    command.output().expect("nx process must run")
}

fn stdout_json(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).expect("stdout must be valid json")
}

#[test]
fn test_cli_reject_new_service_json_exit_and_shape() {
    let root = tempdir().expect("tempdir");
    let output = run_nx(
        &["new", "service", "../escape", "--json"],
        root.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["class"], "validation_reject");
    assert_eq!(json["code"], 3);
}

#[test]
fn test_cli_reject_unknown_postflight_json_exit_and_shape() {
    let root = tempdir().expect("tempdir");
    let output = run_nx(
        &["postflight", "unknown-topic", "--json"],
        root.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["class"], "validation_reject");
    assert_eq!(json["code"], 3);
}

#[test]
fn test_cli_doctor_missing_tools_json_exit_and_shape() {
    let root = tempdir().expect("tempdir");
    let output = run_nx(&["doctor", "--json"], root.path(), Some(""));
    assert_eq!(output.status.code(), Some(4));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["class"], "missing_dependency");
    assert_eq!(json["code"], 4);
    assert!(
        json["data"]["missing_required"]
            .as_array()
            .expect("missing_required array")
            .len()
            >= 5
    );
}

#[test]
fn test_cli_new_service_file_effects_and_json() {
    let root = tempdir().expect("tempdir");
    let output = run_nx(&["new", "service", "svcz", "--json"], root.path(), None);
    assert_eq!(output.status.code(), Some(0));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["class"], "success");
    assert_eq!(json["code"], 0);
    assert!(root.path().join("source/services/svcz/Cargo.toml").exists());
    assert!(root
        .path()
        .join("source/services/svcz/src/main.rs")
        .exists());
    assert!(root
        .path()
        .join("source/services/svcz/docs/stubs/README.md")
        .exists());
}

#[test]
fn test_cli_config_validate_rejects_unknown_field() {
    let root = tempdir().expect("tempdir");
    let input = root.path().join("bad.json");
    std::fs::write(
        &input,
        r#"{
  "dsoftbus": { "transport": "auto", "max_peers": 10, "unknown_knob": true }
}"#,
    )
    .expect("write");
    let output = run_nx(
        &[
            "config",
            "validate",
            input.to_string_lossy().as_ref(),
            "--json",
        ],
        root.path(),
        None,
    );
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["class"], "validation_reject");
}

#[test]
fn test_cli_config_push_and_effective_json() {
    let root = tempdir().expect("tempdir");
    let input = root.path().join("good.json");
    std::fs::write(
        &input,
        r#"{
  "metrics": { "enabled": false, "flush_interval_ms": 1500 }
}"#,
    )
    .expect("write");
    let push = run_nx(
        &["config", "push", input.to_string_lossy().as_ref(), "--json"],
        root.path(),
        None,
    );
    assert_eq!(push.status.code(), Some(0));
    assert!(root.path().join("state/config/90-nx-config.json").exists());

    let effective = run_nx(&["config", "effective", "--json"], root.path(), None);
    assert_eq!(effective.status.code(), Some(0));
    let json = stdout_json(&effective);
    assert_eq!(json["class"], "success");
    assert!(json["data"]["version"].is_string());
}

#[test]
fn test_cli_policy_validate_rejects_manifest_mismatch() {
    let root = tempdir().expect("tempdir");
    let policy_root = root.path().join("policies");
    std::fs::create_dir_all(&policy_root).expect("policy root");
    std::fs::write(
        policy_root.join("nexus.policy.toml"),
        "version = 1\ninclude = ['base.toml']\n",
    )
    .expect("root policy");
    std::fs::write(
        policy_root.join("base.toml"),
        "[allow]\ndemo = ['ipc.core']\n",
    )
    .expect("base policy");
    std::fs::write(
        policy_root.join("manifest.json"),
        r#"{"version":1,"tree_sha256":"stale","generated_at_ns":0}"#,
    )
    .expect("manifest");

    let output = run_nx(&["policy", "validate", "--json"], root.path(), None);

    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["class"], "validation_reject");
    assert!(json["message"]
        .as_str()
        .expect("message")
        .contains("policy.manifest_mismatch"));
}

#[test]
fn test_cli_policy_validate_requires_manifest() {
    let root = tempdir().expect("tempdir");
    let policy_root = root.path().join("policies");
    std::fs::create_dir_all(&policy_root).expect("policy root");
    std::fs::write(
        policy_root.join("nexus.policy.toml"),
        "version = 1\ninclude = ['base.toml']\n",
    )
    .expect("root policy");
    std::fs::write(
        policy_root.join("base.toml"),
        "[allow]\ndemo = ['ipc.core']\n",
    )
    .expect("base policy");

    let output = run_nx(&["policy", "validate", "--json"], root.path(), None);

    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["class"], "validation_reject");
    assert!(json["message"]
        .as_str()
        .expect("message")
        .contains("policy.read"));
}
