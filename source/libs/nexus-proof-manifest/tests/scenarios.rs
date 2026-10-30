// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: TASK-0333 6f-3 (ADR-0089) — the scenario contract. Every shipped scenario
//! (`proof-manifest/scenarios/*.toml`) is valid against the shipped manifest and targets: every
//! literal a declared marker, every target and word named, every action acknowledged. And each
//! rule refuses what it exists to refuse (`test_reject_*`): an undeclared marker, an unknown
//! target or word, two actions in one step, an action nothing acknowledges, an `acked_by` that
//! points backwards, a `then` with no `expect` to follow, a duplicate step, an unknown profile,
//! out-of-range bounds, a retry that logs in or waits.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: this file (host)

use nexus_proof_manifest::parse;
use nexus_proof_manifest::scenario::{self, Action, ScenarioError, Targets};

const MANIFEST: &str = r#"
[meta]
schema_version = "1"
default_profile = "full"

[phase.end]
order = 1

[profile.full]
[profile.lane]

[marker."SELFTEST: input window open"]
phase = "end"

[marker."apphost: modal open (depth="]
phase = "end"

[marker."apphost: text copy ok"]
phase = "end"
"#;

fn targets() -> Targets {
    scenario::parse_targets(
        r#"
[targets]
pill = [10, 20]
card = [30, 40]
[words]
word = "kiwi"
"#,
    )
    .expect("targets")
}

fn check(body: &str) -> Result<scenario::Scenario, ScenarioError> {
    let manifest = parse(MANIFEST).expect("manifest");
    let src = format!(
        "[scenario]\nprofile = \"lane\"\nstart_after = \"SELFTEST: input window open\"\n{body}"
    );
    scenario::parse("lane", &src, &manifest, &targets())
}

#[test]
fn every_shipped_scenario_is_valid() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/selftest-client/proof-manifest");
    let manifest =
        nexus_proof_manifest::parse_path(&std::path::Path::new(root).join("manifest.toml"))
            .expect("the shipped manifest parses");
    let dir = std::path::Path::new(root).join("scenarios");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("scenarios/") {
        let path = entry.expect("entry").path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if path.extension().is_none_or(|e| e != "toml") || stem == "targets" {
            continue;
        }
        let s = scenario::read(&dir, stem, &manifest).unwrap_or_else(|e| panic!("{stem}: {e}"));
        assert_eq!(
            s.start_after, "SELFTEST: input window open",
            "{stem}: no input before the window"
        );
        checked += 1;
    }
    assert!(checked >= 3, "usb-visible, lowmem and input-flood at least (got {checked})");
}

#[test]
fn a_valid_scenario_resolves_its_points_and_prints_json() {
    let s = check(
        r#"
[[step]]
name = "open"
do = { click = "pill" }
expect = ["apphost: modal open (depth=1)"]
then = ["apphost: text copy ok"]
timeout_s = 40

[[step]]
name = "type"
do = { type = "word" }
acked_by = "copy"

[[step]]
name = "copy"
do = { chord = ["ctrl", "c"] }
expect = ["apphost: text copy ok"]
retry = 1
retry_with = [{ click = "card" }]
"#,
    )
    .expect("valid");
    assert_eq!(s.steps.len(), 3);
    assert_eq!(s.steps[0].action, Action::Click { target: "pill".into(), at: (10, 20) });
    assert_eq!(
        s.steps[1].action,
        Action::Type { word: "word".into(), text: "kiwi".into(), pace_ms: None }
    );
    assert_eq!(s.steps[2].timeout_s, scenario::DEFAULT_TIMEOUT_S);
    let json = s.to_json();
    assert!(json.contains("\"kind\":\"click\",\"target\":\"pill\",\"at\":[10,20]"), "{json}");
    assert!(json.contains("\"acked_by\":\"copy\""), "{json}");
    assert!(
        json.contains(
            "\"expect\":[\"apphost: modal open (depth=1)\"],\"then\":[\"apphost: text copy ok\"]"
        ),
        "{json}"
    );
    assert!(json.contains("\"expect\":[\"apphost: text copy ok\"],\"then\":[]"), "{json}");
    assert!(json.contains("\"retry_with\":[{\"kind\":\"click\""), "{json}");
}

#[test]
fn test_reject_an_undeclared_marker() {
    let err = check("[[step]]\nname = \"x\"\ndo = { wait = true }\nexpect = [\"memd: made up\"]\n")
        .expect_err("undeclared");
    assert!(matches!(err, ScenarioError::UndeclaredMarker { .. }), "{err}");
    // A specialisation of a declared marker is accounted for; a bare prefix of one too.
    check("[[step]]\nname = \"x\"\ndo = { wait = true }\nexpect = [\"apphost: modal open (depth=1)\"]\n")
        .expect("specialised");
    check("[[step]]\nname = \"x\"\ndo = { wait = true }\nexpect = [\"apphost: text copy\"]\n")
        .expect("prefix");
    let err = check("[[step]]\nname = \"x\"\ndo = { wait = true }\nexpect = [\"\"]\n")
        .expect_err("empty");
    assert!(matches!(err, ScenarioError::UndeclaredMarker { .. }), "{err}");
}

#[test]
fn test_reject_an_unknown_target_or_word() {
    let err = check("[[step]]\nname = \"x\"\ndo = { click = \"nowhere\" }\n").expect_err("target");
    assert!(matches!(err, ScenarioError::UnknownTarget { .. }), "{err}");
    let err = check("[[step]]\nname = \"x\"\ndo = { type = \"nothing\" }\nacked_by = \"y\"\n[[step]]\nname = \"y\"\ndo = { wait = true }\nexpect = [\"apphost: text copy ok\"]\n")
        .expect_err("word");
    assert!(matches!(err, ScenarioError::UnknownTarget { .. }), "{err}");
}

#[test]
fn test_reject_two_actions_or_none() {
    let err = check("[[step]]\nname = \"x\"\ndo = { click = \"pill\", key = \"esc\" }\n")
        .expect_err("two");
    assert!(matches!(err, ScenarioError::NotOneAction(_)), "{err}");
    let err = check("[[step]]\nname = \"x\"\ndo = {}\nexpect = [\"apphost: text copy ok\"]\n")
        .expect_err("none");
    assert!(matches!(err, ScenarioError::NotOneAction(_)), "{err}");
}

#[test]
fn test_reject_an_unacknowledged_action() {
    // A key writes no readback: something must acknowledge it.
    let err = check("[[step]]\nname = \"x\"\ndo = { key = \"esc\" }\n").expect_err("key");
    assert!(matches!(err, ScenarioError::Unacknowledged(_)), "{err}");
    // `acked_by` must point FORWARD to a step that expects something.
    let err = check("[[step]]\nname = \"y\"\ndo = { wait = true }\nexpect = [\"apphost: text copy ok\"]\n[[step]]\nname = \"x\"\ndo = { key = \"esc\" }\nacked_by = \"y\"\n")
        .expect_err("backwards");
    assert!(matches!(err, ScenarioError::Unacknowledged(_)), "{err}");
    let err = check("[[step]]\nname = \"x\"\ndo = { key = \"esc\" }\nacked_by = \"nobody\"\n")
        .expect_err("nobody");
    assert!(matches!(err, ScenarioError::Unacknowledged(_)), "{err}");
    // A click's own readback acknowledges it.
    check("[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\n").expect("click");
}

#[test]
fn test_reject_a_then_without_an_expect() {
    // `then` follows the last `expect` line: with none, it has nothing to follow.
    let err = check(
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\nthen = [\"apphost: text copy ok\"]\n",
    )
    .expect_err("then alone");
    assert!(matches!(err, ScenarioError::OutOfRange { .. }), "{err}");
    // Its literals are checked like `expect`'s.
    let err = check("[[step]]\nname = \"x\"\ndo = { wait = true }\nexpect = [\"apphost: text copy ok\"]\nthen = [\"memd: made up\"]\n")
        .expect_err("undeclared then");
    assert!(matches!(err, ScenarioError::UndeclaredMarker { .. }), "{err}");
}

#[test]
fn test_reject_a_duplicate_step_or_an_unknown_profile() {
    let err = check("[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\n[[step]]\nname = \"x\"\ndo = { click = \"card\" }\n")
        .expect_err("duplicate");
    assert!(matches!(err, ScenarioError::DuplicateStep(_)), "{err}");
    let manifest = parse(MANIFEST).expect("manifest");
    let src = "[scenario]\nprofile = \"nope\"\nstart_after = \"SELFTEST: input window open\"\n[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\n";
    let err = scenario::parse("x", src, &manifest, &targets()).expect_err("profile");
    assert!(matches!(err, ScenarioError::UnknownProfile(_)), "{err}");
}

#[test]
fn test_reject_out_of_range_bounds_and_retries() {
    for body in [
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\ntimeout_s = 0.0\n",
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\ntimeout_s = 601.0\n",
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\nsettle_s = 11.0\n",
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\nretry = 4\n",
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\nretry_with = [{ click = \"card\" }]\n",
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\nretry = 1\nretry_with = [{ login = true }]\n",
        "[[step]]\nname = \"x\"\ndo = { chord = [\"ctrl\"] }\nexpect = [\"apphost: text copy ok\"]\n",
        "[[step]]\nname = \"x\"\ndo = { key = \"Esc!\" }\nexpect = [\"apphost: text copy ok\"]\n",
    ] {
        let err = check(body).expect_err(body);
        assert!(matches!(err, ScenarioError::OutOfRange { .. }), "{body}: {err}");
    }
}

/// RFC-0113: a word typed at a declared pace — the runner holds each key that long.
#[test]
fn a_typing_pace_is_declared_on_the_word_and_printed() {
    let s = check(
        "[[step]]\nname = \"type\"\ndo = { type = \"word\", pace_ms = 30 }\nacked_by = \"copy\"\n[[step]]\nname = \"copy\"\ndo = { wait = true }\nexpect = [\"apphost: text copy ok\"]\n",
    )
    .expect("paced");
    assert_eq!(
        s.steps[0].action,
        Action::Type { word: "word".into(), text: "kiwi".into(), pace_ms: Some(30) }
    );
    assert!(s.to_json().contains("\"text\":\"kiwi\",\"pace_ms\":30}"), "{}", s.to_json());
}

#[test]
fn test_reject_a_pace_without_typing_or_out_of_range() {
    for body in [
        "[[step]]\nname = \"x\"\ndo = { click = \"pill\", pace_ms = 30 }\n",
        "[[step]]\nname = \"x\"\ndo = { type = \"word\", pace_ms = 5 }\nacked_by = \"y\"\n[[step]]\nname = \"y\"\ndo = { wait = true }\nexpect = [\"apphost: text copy ok\"]\n",
        "[[step]]\nname = \"x\"\ndo = { type = \"word\", pace_ms = 1001 }\nacked_by = \"y\"\n[[step]]\nname = \"y\"\ndo = { wait = true }\nexpect = [\"apphost: text copy ok\"]\n",
    ] {
        let err = check(body).expect_err(body);
        assert!(matches!(err, ScenarioError::OutOfRange { .. }), "{body}: {err}");
    }
}

#[test]
fn test_reject_unknown_fields() {
    let err = check("[[step]]\nname = \"x\"\ndo = { click = \"pill\" }\nsleep_s = 3\n")
        .expect_err("field");
    assert!(matches!(err, ScenarioError::Malformed(_)), "{err}");
    let err = check("").expect_err("no step");
    assert!(matches!(err, ScenarioError::Malformed(_)), "{err}");
}
