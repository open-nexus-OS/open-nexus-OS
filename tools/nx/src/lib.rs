// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Canonical host-first `nx` CLI contract implementation.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests in this module plus process-boundary integration tests in `tests/cli_contract.rs`.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

mod cli;
mod commands;
mod error;
mod output;
mod runtime;

extern crate nexus_policy;

pub use commands::run;

#[cfg(test)]
use cli::{Cli, DoctorArgs};
#[cfg(test)]
use commands::config::load_layers_from_repo;
#[cfg(test)]
use commands::doctor::handle_doctor_with_path;
#[cfg(test)]
use commands::execute;
#[cfg(test)]
use configd::Configd;
#[cfg(test)]
use error::ExitClass;
#[cfg(test)]
use nexus_policy::PolicyTree;
#[cfg(test)]
use runtime::RuntimeConfig;
#[cfg(test)]
use serde_json::{json, Value};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::fs;
    use tempfile::TempDir;

    fn test_cfg(root: &Path) -> RuntimeConfig {
        RuntimeConfig {
            repo_root: root.to_path_buf(),
            postflight_dir: root.join("tools"),
            dsl_backend: None,
        }
    }

    #[test]
    fn test_reject_new_service_path_traversal() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "new", "service", "../escape"]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("must reject traversal");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_reject_new_service_absolute_path() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from([
            "nx",
            "new",
            "service",
            "svc",
            "--root",
            "/tmp/absolute-path-reject",
        ]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("must reject absolute root");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_new_service_creates_expected_tree() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "new", "service", "svc-a", "--json"]);
        let (class, _, _, _) = execute(cli, &test_cfg(root.path())).expect("must succeed");
        assert_eq!(class, ExitClass::Success);
        assert!(root
            .path()
            .join("source/services/svc-a/Cargo.toml")
            .exists());
        assert!(root
            .path()
            .join("source/services/svc-a/src/main.rs")
            .exists());
    }

    #[test]
    fn test_reject_unknown_postflight_topic() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "postflight", "unknown-topic"]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("must reject unknown topic");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_postflight_failure_passthrough() {
        let root = TempDir::new().expect("tempdir");
        let tools = root.path().join("tools");
        fs::create_dir_all(&tools).expect("tools dir");
        let script = tools.join("postflight-vfs.sh");
        fs::write(&script, "#!/usr/bin/env sh\nexit 9\n").expect("write script");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("set perms");
        }

        let cli = Cli::parse_from(["nx", "postflight", "vfs", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("must run");
        assert_eq!(class, ExitClass::DelegateFailure);
        assert_eq!(data.expect("data")["delegate_exit"], json!(9));
    }

    #[test]
    fn test_postflight_success_passthrough() {
        let root = TempDir::new().expect("tempdir");
        let tools = root.path().join("tools");
        fs::create_dir_all(&tools).expect("tools dir");
        let script = tools.join("postflight-vfs.sh");
        fs::write(&script, "#!/usr/bin/env sh\nexit 0\n").expect("write script");
        let mut perms = fs::metadata(&script).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("set perms");
        }

        let cli = Cli::parse_from(["nx", "postflight", "vfs", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("must run");
        assert_eq!(class, ExitClass::Success);
        assert_eq!(data.expect("data")["delegate_exit"], json!(0));
    }

    #[test]
    fn test_doctor_exit_nonzero_when_required_missing() {
        let args = DoctorArgs { json: true };
        let result = handle_doctor_with_path(args, Some("".into())).expect("doctor result");
        assert_eq!(result.0, ExitClass::MissingDependency);
    }

    #[test]
    fn test_doctor_reports_missing_required_tools() {
        let args = DoctorArgs { json: true };
        let (_, _, _, data) =
            handle_doctor_with_path(args, Some("".into())).expect("doctor result");
        let data = data.expect("data");
        let missing = data["missing_required"].as_array().expect("missing array");
        assert!(missing.len() >= 5);
    }

    #[test]
    fn test_dsl_wrapper_fail_closed_when_backend_missing() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "dsl", "fmt", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("must classify");
        assert_eq!(class, ExitClass::Unsupported);
        assert_eq!(data.expect("data")["classification"], json!("unsupported"));
    }

    #[test]
    fn test_dsl_wrapper_propagates_delegate_failure() {
        let root = TempDir::new().expect("tempdir");
        let backend = root.path().join("dsl-backend.sh");
        fs::write(&backend, "#!/usr/bin/env sh\nexit 4\n").expect("write backend");
        let mut perms = fs::metadata(&backend).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&backend, perms).expect("set perms");
        }

        let cli = Cli::parse_from(["nx", "dsl", "build", "--json"]);
        let mut cfg = test_cfg(root.path());
        cfg.dsl_backend = Some(backend);
        let (class, _, _, data) = execute(cli, &cfg).expect("must run");
        assert_eq!(class, ExitClass::DelegateFailure);
        assert_eq!(data.expect("data")["delegate_exit"], json!(4));
    }

    #[test]
    fn test_inspect_nxb_json_stable_fixture() {
        let root = TempDir::new().expect("tempdir");
        let nxb_dir = root.path().join("fixture.nxb");
        fs::create_dir_all(nxb_dir.join("meta")).expect("meta dir");
        fs::write(nxb_dir.join("manifest.toml"), "name = 'demo'\n").expect("manifest");
        fs::write(nxb_dir.join("payload.elf"), b"abc").expect("payload");
        fs::write(nxb_dir.join("meta/info.txt"), "ok").expect("meta file");

        let cli = Cli::parse_from([
            "nx",
            "inspect",
            "nxb",
            nxb_dir.to_string_lossy().as_ref(),
            "--json",
        ]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("inspect works");
        assert_eq!(class, ExitClass::Success);
        let data = data.expect("data");
        assert_eq!(data["payload_present"], json!(true));
        assert!(data["payload_sha256"].is_string());
    }

    #[test]
    fn test_config_validate_rejects_unknown_field() {
        let root = TempDir::new().expect("tempdir");
        let input = root.path().join("bad-config.json");
        fs::write(
            &input,
            r#"{
  "dsoftbus": { "transport": "auto", "max_peers": 20, "unknown_knob": true }
}"#,
        )
        .expect("write");
        let cli = Cli::parse_from([
            "nx",
            "config",
            "validate",
            input.to_string_lossy().as_ref(),
            "--json",
        ]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("validation must fail");
        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_config_push_writes_state_config() {
        let root = TempDir::new().expect("tempdir");
        let input = root.path().join("good-config.json");
        fs::write(
            &input,
            r#"{
  "metrics": { "enabled": false, "flush_interval_ms": 1200 }
}"#,
        )
        .expect("write");
        let cli = Cli::parse_from([
            "nx",
            "config",
            "push",
            input.to_string_lossy().as_ref(),
            "--json",
        ]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("push success");
        assert_eq!(class, ExitClass::Success);
        assert!(root.path().join("state/config/90-nx-config.json").exists());
        assert!(data.expect("data")["path"].is_string());
    }

    #[test]
    fn test_config_effective_is_deterministic() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("state/config")).expect("state dir");
        fs::write(
            root.path().join("state/config/90-nx-config.json"),
            r#"{"tracing":{"level":"debug"}}"#,
        )
        .expect("write state");

        let cli_a = Cli::parse_from(["nx", "config", "effective", "--json"]);
        let (_, _, _, data_a) = execute(cli_a, &test_cfg(root.path())).expect("effective a");
        let cli_b = Cli::parse_from(["nx", "config", "effective", "--json"]);
        let (_, _, _, data_b) = execute(cli_b, &test_cfg(root.path())).expect("effective b");
        assert_eq!(data_a, data_b);
    }

    #[test]
    fn test_config_effective_matches_configd_version_and_json() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("system/config")).expect("system dir");
        fs::create_dir_all(root.path().join("state/config")).expect("state dir");
        fs::write(
            root.path().join("system/config/10-base.json"),
            r#"{"metrics":{"enabled":true,"flush_interval_ms":2500}}"#,
        )
        .expect("write system");
        fs::write(
            root.path().join("state/config/90-nx-config.json"),
            r#"{"metrics":{"enabled":false},"tracing":{"level":"debug"}}"#,
        )
        .expect("write state");

        let cfg = test_cfg(root.path());
        let cli = Cli::parse_from(["nx", "config", "effective", "--json"]);
        let (_, _, _, data) = execute(cli, &cfg).expect("effective success");
        let data = data.expect("json data");

        let layers = load_layers_from_repo(&cfg).expect("load repo layers");
        let daemon = Configd::new(layers).expect("configd init");
        let daemon_view = daemon.get_effective_json();

        assert_eq!(data["version"], Value::String(daemon_view.version));
        assert_eq!(data["effective"], daemon_view.derived_json);
    }

    #[test]
    fn test_config_reload_reports_commit_and_active_version() {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("state/config")).expect("state dir");
        fs::write(
            root.path().join("state/config/90-nx-config.json"),
            r#"{"metrics":{"enabled":false}}"#,
        )
        .expect("write state");

        let cli = Cli::parse_from(["nx", "config", "reload", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("reload success");
        let data = data.expect("json data");

        assert_eq!(class, ExitClass::Success);
        assert_eq!(data["committed"], Value::Bool(true));
        assert_eq!(data["candidate_version"], data["active_version"]);
    }

    #[test]
    fn test_config_where_returns_layer_directories() {
        let root = TempDir::new().expect("tempdir");
        let cli = Cli::parse_from(["nx", "config", "where", "--json"]);
        let (_, _, _, data) = execute(cli, &test_cfg(root.path())).expect("where success");
        let data = data.expect("json data");

        assert_eq!(
            data["state"],
            Value::String(root.path().join("state/config").display().to_string())
        );
        assert_eq!(
            data["system"],
            Value::String(root.path().join("system/config").display().to_string())
        );
        assert_eq!(data["env_prefix"], Value::String("NEXUS_CFG_".to_string()));
    }

    fn write_policy_root(root: &Path, caps: &[&str]) {
        fs::create_dir_all(root).expect("policy root");
        fs::write(
            root.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .expect("root");
        let caps = caps
            .iter()
            .map(|cap| format!("'{cap}'"))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(
            root.join("base.toml"),
            format!("[allow]\ndemo = [{caps}]\n"),
        )
        .expect("base");
        let tree = PolicyTree::load_root(root).expect("policy tree");
        tree.write_manifest(root).expect("policy manifest");
    }

    #[test]
    fn test_policy_validate_reports_version() {
        let root = TempDir::new().expect("tempdir");
        write_policy_root(&root.path().join("policies"), &["ipc.core"]);

        let cli = Cli::parse_from(["nx", "policy", "validate", "--json"]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("validate");
        let data = data.expect("json data");

        assert_eq!(class, ExitClass::Success);
        assert!(data["version"].is_string());
        assert_eq!(data["manifest"], Value::Bool(true));
        assert_eq!(data["subjects"], json!(1));
    }

    #[test]
    fn test_policy_explain_returns_bounded_decision() {
        let root = TempDir::new().expect("tempdir");
        write_policy_root(&root.path().join("policies"), &["ipc.core"]);

        let cli = Cli::parse_from([
            "nx",
            "policy",
            "explain",
            "--subject",
            "demo",
            "--cap",
            "ipc.core",
            "--json",
        ]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("explain");
        let data = data.expect("json data");

        assert_eq!(class, ExitClass::Success);
        assert_eq!(data["decision"]["allow"], Value::Bool(true));
        assert_eq!(
            data["decision"]["trace"].as_array().expect("trace").len(),
            1
        );
    }

    #[test]
    fn test_policy_diff_is_deterministic() {
        let root = TempDir::new().expect("tempdir");
        let from = root.path().join("from");
        let to = root.path().join("to");
        write_policy_root(&from, &["ipc.core"]);
        write_policy_root(&to, &["ipc.core", "crypto.sign"]);

        let cli = Cli::parse_from([
            "nx",
            "policy",
            "diff",
            "--from",
            from.to_string_lossy().as_ref(),
            "--to",
            to.to_string_lossy().as_ref(),
            "--json",
        ]);
        let (class, _, _, data) = execute(cli, &test_cfg(root.path())).expect("diff");
        let data = data.expect("json data");

        assert_eq!(class, ExitClass::Success);
        assert_eq!(data["changed"], Value::Bool(true));
        assert_ne!(data["from_version"], data["to_version"]);
    }

    #[test]
    fn test_policy_mode_rejects_unauthorized() {
        let root = TempDir::new().expect("tempdir");
        write_policy_root(&root.path().join("policies"), &["ipc.core"]);

        let cli = Cli::parse_from([
            "nx",
            "policy",
            "mode",
            "--set",
            "learn",
            "--observed-version",
            "irrelevant",
            "--actor-service-id",
            "0",
            "--json",
        ]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("reject");

        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_policy_mode_rejects_stale_version() {
        let root = TempDir::new().expect("tempdir");
        write_policy_root(&root.path().join("policies"), &["ipc.core"]);

        let cli = Cli::parse_from([
            "nx",
            "policy",
            "mode",
            "--set",
            "dry-run",
            "--observed-version",
            "stale",
            "--actor-service-id",
            "42",
            "--authorized",
            "--json",
        ]);
        let err = execute(cli, &test_cfg(root.path())).expect_err("reject");

        assert_eq!(err.class, ExitClass::ValidationReject);
    }

    #[test]
    fn test_policy_mode_success_is_explicit_host_preflight_only() {
        let root = TempDir::new().expect("tempdir");
        let policy_root = root.path().join("policies");
        write_policy_root(&policy_root, &["ipc.core"]);
        let version = PolicyTree::load_root(&policy_root)
            .expect("policy tree")
            .version()
            .as_str()
            .to_string();

        let cli = Cli::parse_from([
            "nx",
            "policy",
            "mode",
            "--set",
            "dry-run",
            "--observed-version",
            version.as_str(),
            "--actor-service-id",
            "42",
            "--authorized",
            "--json",
        ]);
        let (class, message, _, data) = execute(cli, &test_cfg(root.path())).expect("mode");
        let data = data.expect("json data");

        assert_eq!(class, ExitClass::Success);
        assert_eq!(message, "policy mode preflight accepted");
        assert_eq!(data["preflight_only"], Value::Bool(true));
        assert_eq!(data["applied"], Value::Bool(false));
    }
}
