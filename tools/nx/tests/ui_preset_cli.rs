// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for `nx ui preset list/env` (TASK-0055D):
//! the registered catalog lists deterministically, the baseline preset
//! resolves to the launcher's default mode, `env` output is `eval`-able
//! `KEY=VALUE` lines, and an unknown preset is a `validation_reject` (exit 3)
//! that names the valid ids — the launcher recipe stops BEFORE any build.
//! OWNERS: @tools-team @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 integration tests
//! ADR: docs/adr/0035-systemui-declarative-shell-configuration.md

use std::process::{Command, Output};

fn run_nx(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx")).args(args).output().expect("nx process must run")
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout must be the JSON envelope")
}

#[test]
fn list_is_the_registered_catalog() {
    let out = run_nx(&["ui", "preset", "list", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let v = stdout_json(&out);
    let ids: Vec<&str> = v["data"]["presets"]
        .as_array()
        .expect("presets array")
        .iter()
        .map(|p| p["id"].as_str().expect("id"))
        .collect();
    assert_eq!(
        ids,
        [
            "phone-portrait",
            "phone-landscape",
            "tablet-portrait",
            "tablet-landscape",
            "laptop",
            "laptop-pro",
            "convertible",
        ]
    );
    // Text mode: one row per preset, no JSON envelope.
    let text = run_nx(&["ui", "preset", "list"]);
    assert_eq!(text.status.code(), Some(0));
    let lines = String::from_utf8_lossy(&text.stdout);
    assert_eq!(lines.lines().count(), 7);
    assert!(lines.contains("tablet-portrait"));
}

#[test]
fn env_lines_are_eval_able_and_baseline_matches_default_launch() {
    let out = run_nx(&["ui", "preset", "env", "tablet-landscape"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    let mut kv = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (k, v) = line.split_once('=').expect("KEY=VALUE line");
        assert!(k.chars().all(|c| c.is_ascii_uppercase() || c == '_'), "key {k:?}");
        assert!(
            v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "value {v:?}"
        );
        kv.insert(k.to_string(), v.to_string());
    }
    assert_eq!(kv["QEMU_GPU_XRES"], "1280");
    assert_eq!(kv["QEMU_GPU_YRES"], "800");
    assert_eq!(kv["QEMU_PROOF_POINTER_SOURCE"], "tablet");
    assert_eq!(kv["NEXUS_UI_PRESET"], "tablet-landscape");
    assert_eq!(kv["NEXUS_UI_PRESET_PROFILE"], "tablet");
    assert_eq!(kv["NEXUS_UI_PRESET_SHELL"], "tablet");
    assert_eq!(kv["NEXUS_UI_PRESET_HZ"], "120");
}

#[test]
fn env_json_carries_the_resolved_mode() {
    let out = run_nx(&["ui", "preset", "env", "phone-portrait", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let v = stdout_json(&out);
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["preset"], "phone-portrait");
    assert_eq!(v["data"]["env"]["QEMU_GPU_XRES"], "480");
    assert_eq!(v["data"]["env"]["QEMU_GPU_YRES"], "800");
    assert_eq!(v["data"]["env"]["NEXUS_UI_PRESET_ORIENTATION"], "portrait");
    assert_eq!(v["data"]["env"]["NEXUS_UI_PRESET_SIZE_CLASS"], "compact");
    assert_eq!(v["data"]["env"]["QEMU_PROOF_POINTER_SOURCE"], "tablet");
}

#[test]
fn test_reject_unknown_preset_is_validation_reject() {
    let out = run_nx(&["ui", "preset", "env", "phablet", "--json"]);
    assert_eq!(out.status.code(), Some(3), "validation_reject exit class");
    let v = stdout_json(&out);
    assert_eq!(v["ok"], false);
    assert_eq!(v["class"], "validation_reject");
    let msg = v["message"].as_str().expect("message");
    assert!(msg.contains("phablet"), "{msg}");
    assert!(msg.contains("tablet-landscape"), "valid ids listed: {msg}");
    // Text mode prints the reason only — nothing a shell could eval as an
    // assignment (the recipe redirects stdout into a file it then sources).
    let text = run_nx(&["ui", "preset", "env", "phablet"]);
    assert_eq!(text.status.code(), Some(3));
    assert!(!String::from_utf8_lossy(&text.stdout).contains("QEMU_GPU_XRES="));
}
