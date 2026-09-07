// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: process-boundary tests of `nx policy learn-gen` (RFC-0091 §5):
//! a learn log becomes a skeleton the real policy loader accepts, JSON
//! stats are honest, `any` binds stay held without `--allow-any`, and the
//! input bounds reject.
//! OWNERS: @tools-team @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0028 P2

use std::path::Path;
use std::process::{Command, Output};

use nexus_policy::learn_record::{service_id_from_name, LearnAddr, LearnArg, LearnRecord, Would};
use nexus_policy::schema::{AddressClass, Rule};
use nexus_policy::PolicyTree;
use serde_json::Value;
use tempfile::tempdir;

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx")).args(args).current_dir(cwd).output().expect("nx runs")
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("json stdout")
}

fn record(subject: &str, arg: LearnArg<'_>) -> String {
    nexus_policy::learn_gen::format_record(&LearnRecord {
        epoch: 4,
        subject: service_id_from_name(subject.as_bytes()),
        arg,
        would: Would::Deny,
    })
    .unwrap()
}

fn learn_log() -> String {
    [
        "init: ready".to_string(),
        record("demo.app", LearnArg::Statefs("/state/app/demo/cache/")),
        record("demo.app", LearnArg::Statefs("/state/app/demo/cache/")),
        record("demo.app", LearnArg::NetBind(9000, LearnAddr::Loopback)),
        record("demo.app", LearnArg::NetBind(8080, LearnAddr::Any)),
        record("demo.app", LearnArg::NetConnect([10, 0, 2, 9], 443)),
        record("someone.else", LearnArg::Statefs("/state/other/")),
        "policyd: audit emit ok".to_string(),
    ]
    .join("\n")
}

/// Loads the generated file through the real policy root (manifest included).
fn load_generated(root: &Path, generated: &str) -> PolicyTree {
    let policies = root.join("policies");
    std::fs::create_dir_all(&policies).unwrap();
    std::fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['base.toml']\n")
        .unwrap();
    std::fs::write(
        policies.join("base.toml"),
        format!("[allow]\n\"demo.app\" = ['ipc.core']\n\n{generated}"),
    )
    .unwrap();
    let tree = PolicyTree::load_root(&policies).unwrap();
    tree.write_manifest(&policies).unwrap();
    let validate = run_nx(&["policy", "validate", "--json"], root);
    assert_eq!(validate.status.code(), Some(0), "{}", String::from_utf8_lossy(&validate.stdout));
    tree
}

#[test]
fn test_cli_policy_learn_gen_roundtrip() {
    let dir = tempdir().unwrap();
    let log = dir.path().join("learn.log");
    std::fs::write(&log, learn_log()).unwrap();
    let out = dir.path().join("demo.app.abi.toml");
    let output = run_nx(
        &[
            "policy",
            "learn-gen",
            log.to_str().unwrap(),
            "--subject",
            "demo.app",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let json = stdout_json(&output);
    assert_eq!(json["ok"], true);
    let d = &json["data"];
    assert_eq!(d["records_parsed"], 6);
    assert_eq!(d["lines_ignored"], 2);
    assert_eq!(d["records_for_subject"], 5);
    assert_eq!(d["unique_args"], 4);
    assert_eq!(d["rules_emitted"], 3);
    assert_eq!(d["rules_held"], 1);
    assert_eq!(d["rules_capped"], 0);
    assert_eq!(d["epoch_observed"], 4);
    assert_eq!(d["subject_id"], format!("{:016x}", service_id_from_name(b"demo.app")));

    let generated = std::fs::read_to_string(&out).unwrap();
    assert!(generated.starts_with("# generated — review before enabling"));
    let tree = load_generated(dir.path(), &generated);
    let profile = tree.policy().abi_profile("demo.app").expect("skeleton profile");
    assert_eq!(profile.epoch, 5);
    assert_eq!(profile.rules.len(), 3);
    assert!(profile
        .rules
        .iter()
        .all(|r| !matches!(r, Rule::NetBind { address: AddressClass::Any, .. })));

    // --allow-any lifts the held bind; output is deterministic otherwise.
    let out2 = dir.path().join("demo.app.any.toml");
    let output = run_nx(
        &[
            "policy",
            "learn-gen",
            log.to_str().unwrap(),
            "--subject",
            "demo.app",
            "--out",
            out2.to_str().unwrap(),
            "--allow-any",
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(0));
    let d = stdout_json(&output)["data"].clone();
    assert_eq!((d["rules_emitted"].as_u64(), d["rules_held"].as_u64()), (Some(4), Some(0)));
    let generated_any = std::fs::read_to_string(&out2).unwrap();
    assert!(generated_any.contains("\naddress = \"any\""));
    let again = run_nx(
        &[
            "policy",
            "learn-gen",
            log.to_str().unwrap(),
            "--subject",
            "demo.app",
            "--out",
            out.to_str().unwrap(),
        ],
        dir.path(),
    );
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), generated);
}

#[test]
fn test_cli_policy_learn_gen_rejects_bad_input() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("x.toml");
    // Missing log.
    let output = run_nx(
        &[
            "policy",
            "learn-gen",
            "missing.log",
            "--subject",
            "demo.app",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(stdout_json(&output)["class"], "validation_reject");
    // Oversized log.
    let big = dir.path().join("big.log");
    std::fs::write(&big, vec![b'\n'; 4 * 1024 * 1024 + 1]).unwrap();
    let output = run_nx(
        &[
            "policy",
            "learn-gen",
            big.to_str().unwrap(),
            "--subject",
            "demo.app",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(stdout_json(&output)["message"].as_str().unwrap().contains("too large"));
    // Subject that could break the TOML key.
    let log = dir.path().join("learn.log");
    std::fs::write(&log, learn_log()).unwrap();
    let output = run_nx(
        &[
            "policy",
            "learn-gen",
            log.to_str().unwrap(),
            "--subject",
            "demo\"app",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(!out.exists());
    // A subject with no records yields the explicit deny-all skeleton (not an error).
    let output = run_nx(
        &[
            "policy",
            "learn-gen",
            log.to_str().unwrap(),
            "--subject",
            "nobody",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout_json(&output)["data"]["rules_emitted"], 0);
    assert!(std::fs::read_to_string(&out).unwrap().contains("epoch = 1"));
}
