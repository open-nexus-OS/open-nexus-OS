//! CONTEXT: Process-boundary contract tests for `nx` CLI v1.
//! INTENT: Assert exit codes, JSON envelopes, and deterministic file effects.
//! TESTS: Executed via `cargo test -p nx -- --nocapture`.

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
    let output = run_nx(&["new", "service", "../escape", "--json"], root.path(), None);
    assert_eq!(output.status.code(), Some(3));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["class"], "validation_reject");
    assert_eq!(json["code"], 3);
}

#[test]
fn test_cli_reject_unknown_postflight_json_exit_and_shape() {
    let root = tempdir().expect("tempdir");
    let output = run_nx(&["postflight", "unknown-topic", "--json"], root.path(), None);
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
        json["data"]["missing_required"].as_array().expect("missing_required array").len() >= 5
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
    assert!(root.path().join("source/services/svcz/src/main.rs").exists());
    assert!(root.path().join("source/services/svcz/docs/stubs/README.md").exists());
}
