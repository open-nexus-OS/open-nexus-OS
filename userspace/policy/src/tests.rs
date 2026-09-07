// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of the host policy crate (split out of lib.rs to
//! keep the module under the structure ratchet).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: this file

use super::*;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn check_allows_and_denies() {
    let mut doc = PolicyDoc::default();
    doc.merge(
        Path::new("inline"),
        RawPolicy {
            allow: BTreeMap::from([(
                "Example".to_string(),
                vec!["IPC.Core".to_string(), "time.read".to_string()],
            )]),
            abi_profile: BTreeMap::new(),
            quota: BTreeMap::new(),
        },
    )
    .unwrap();

    assert!(doc.check(&["ipc.core"], "EXAMPLE").is_ok());
    let err = doc.check(&["fs.write"], "example").unwrap_err();
    assert_eq!(err.missing, vec!["fs.write".to_string()]);
}

#[test]
fn load_dir_merges_files_with_override() {
    let temp = TempDir::new().unwrap();
    let path = temp.path();
    let mut file_a = std::fs::File::create(path.join("a.toml")).unwrap();
    writeln!(file_a, "[allow]\nfoo = ['cap.a']\nbar = ['cap.b']").unwrap();
    let mut file_b = std::fs::File::create(path.join("b.toml")).unwrap();
    writeln!(file_b, "[allow]\nbar = ['cap.c']").unwrap();

    let doc = PolicyDoc::load_dir(path).unwrap();
    assert!(doc.check(&["cap.a"], "foo").is_ok());
    let err = doc.check(&["cap.b"], "bar").unwrap_err();
    assert_eq!(err.missing, vec!["cap.b".to_string()]);
    assert!(doc.check(&["cap.c"], "bar").is_ok());
}

#[test]
fn policy_tree_version_is_deterministic_for_same_inputs() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['base.toml']\n")
        .unwrap();
    fs::write(policies.join("base.toml"), "[allow]\nExample = ['IPC.Core', 'time.read']\n")
        .unwrap();

    let first = PolicyTree::load_root(&policies).unwrap();
    let second = PolicyTree::load_root(&policies).unwrap();

    assert_eq!(first.version(), second.version());
    assert!(first.policy().check(&["ipc.core"], "example").is_ok());
}

#[test]
fn test_reject_invalid_policy_tree() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 2\ninclude = ['base.toml']\n")
        .unwrap();
    fs::write(policies.join("base.toml"), "[allow]\nsvc = ['ipc.core']\n").unwrap();

    let err = PolicyTree::load_root(&policies).unwrap_err();
    assert_eq!(err.code(), "policy.invalid_root");
}

#[test]
fn test_reject_oversize_policy_tree() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['base.toml']\n")
        .unwrap();
    fs::write(policies.join("base.toml"), "x".repeat(MAX_POLICY_FILE_BYTES + 1)).unwrap();

    let err = PolicyTree::load_root(&policies).unwrap_err();
    assert_eq!(err.code(), "policy.oversize");
}

#[test]
fn test_reject_policy_include_traversal() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['../base.toml']\n")
        .unwrap();

    let err = PolicyTree::load_root(&policies).unwrap_err();
    assert_eq!(err.code(), "policy.include_traversal");
}

#[test]
fn test_reject_ambiguous_policy_root() {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("policies")).unwrap();
    fs::create_dir_all(temp.path().join("recipes/policy")).unwrap();
    fs::write(temp.path().join("recipes/policy/base.toml"), "[allow]\nsvc = ['ipc.core']\n")
        .unwrap();

    let err = PolicyTree::load_single_authority(temp.path()).unwrap_err();
    assert_eq!(err.code(), "policy.ambiguous_root");
}

#[test]
fn test_reject_unknown_policy_section() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['base.toml']\n")
        .unwrap();
    fs::write(policies.join("base.toml"), "[unknown]\nsvc = true\n").unwrap();

    let err = PolicyTree::load_root(&policies).unwrap_err();
    assert_eq!(err.code(), "policy.unknown_section");
}

#[test]
fn evaluator_returns_bounded_explain_trace_and_stable_reason() {
    let mut doc = PolicyDoc::default();
    doc.merge(
        Path::new("inline"),
        RawPolicy {
            allow: BTreeMap::from([(
                "Example".to_string(),
                vec!["IPC.Core".to_string(), "time.read".to_string()],
            )]),
            abi_profile: BTreeMap::new(),
            quota: BTreeMap::new(),
        },
    )
    .unwrap();

    let decision = doc
        .evaluate(&["ipc.core", "time.read"], "EXAMPLE", PolicyMode::Enforce)
        .expect("evaluation");

    assert!(decision.allow);
    assert!(!decision.would_deny);
    assert_eq!(decision.reason_code.as_str(), "explicit_allow");
    assert_eq!(decision.trace.len(), 2);
    assert!(decision.trace.iter().all(|step| step.matched));
}

#[test]
fn evaluator_is_deny_by_default_with_stable_missing_reason() {
    let doc = PolicyDoc::default();
    let decision = doc.evaluate(&["fs.write"], "unknown", PolicyMode::Enforce).expect("evaluation");

    assert!(!decision.allow);
    assert!(decision.would_deny);
    assert_eq!(decision.reason_code.as_str(), "missing_capabilities");
    assert_eq!(decision.trace[0].capability, "fs.write");
    assert!(!decision.trace[0].matched);
}

#[test]
fn dry_run_and_learn_do_not_bypass_enforce_denies() {
    let doc = PolicyDoc::default();

    for mode in [PolicyMode::DryRun, PolicyMode::Learn] {
        let decision = doc.evaluate(&["crypto.sign"], "demo", mode).expect("evaluation");
        assert!(!decision.allow);
        assert!(decision.would_deny);
        assert_eq!(decision.reason_code, ReasonCode::MissingCapabilities);
        assert_eq!(decision.mode, mode);
    }
}

#[test]
fn test_reject_unbounded_explain_trace() {
    let doc = PolicyDoc::default();
    let err = doc
        .evaluate_with_trace_limit(&["cap.a", "cap.b"], "demo", PolicyMode::Enforce, 1)
        .unwrap_err();

    assert_eq!(err.code(), "policy.explain_trace_over_budget");
}

#[test]
fn evaluator_covers_abi_egress_and_signing_domain_shapes() {
    let mut doc = PolicyDoc::default();
    doc.merge(
        Path::new("inline"),
        RawPolicy {
            allow: BTreeMap::from([
                ("selftest-client".to_string(), vec!["abi.statefs.put".to_string()]),
                ("netstackd".to_string(), vec!["net.egress".to_string()]),
                ("keystored".to_string(), vec!["crypto.sign".to_string()]),
            ]),
            abi_profile: BTreeMap::new(),
            quota: BTreeMap::new(),
        },
    )
    .unwrap();

    assert!(
        doc.evaluate(&["abi.statefs.put"], "selftest-client", PolicyMode::Enforce)
            .expect("abi")
            .allow
    );
    assert!(doc.evaluate(&["net.egress"], "netstackd", PolicyMode::Enforce).expect("egress").allow);
    assert!(
        doc.evaluate(&["crypto.sign"], "keystored", PolicyMode::Enforce).expect("signing").allow
    );
}

#[test]
fn learn_log_normalization_is_deterministic() {
    let doc = PolicyDoc::default();
    let decision = doc
        .evaluate(&["net.egress", "crypto.sign"], "demo", PolicyMode::Learn)
        .expect("learn eval");
    let observations = PolicyDoc::learn_observations(&decision);
    let mut reversed = observations.clone();
    reversed.reverse();

    assert_eq!(
        PolicyDoc::normalize_learn_log(observations),
        PolicyDoc::normalize_learn_log(reversed)
    );
}

#[test]
fn policy_manifest_is_deterministic_and_validates_tree_hash() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['base.toml']\n")
        .unwrap();
    fs::write(policies.join("base.toml"), "[allow]\ndemo = ['ipc.core']\n").unwrap();
    let tree = PolicyTree::load_root(&policies).unwrap();

    tree.write_manifest(&policies).unwrap();

    let manifest = fs::read_to_string(policies.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"version\": 1"));
    assert!(manifest.contains("\"generated_at_ns\": 0"));
    assert!(manifest.contains(tree.version().as_str()));
    tree.validate_manifest(&policies).unwrap();
}

#[test]
fn test_reject_policy_manifest_mismatch() {
    let temp = TempDir::new().unwrap();
    let policies = temp.path().join("policies");
    fs::create_dir_all(&policies).unwrap();
    fs::write(policies.join("nexus.policy.toml"), "version = 1\ninclude = ['base.toml']\n")
        .unwrap();
    fs::write(policies.join("base.toml"), "[allow]\ndemo = ['ipc.core']\n").unwrap();
    fs::write(
        policies.join("manifest.json"),
        r#"{"version":1,"tree_sha256":"bad","generated_at_ns":0}"#,
    )
    .unwrap();
    let tree = PolicyTree::load_root(&policies).unwrap();

    let err = tree.validate_manifest(&policies).unwrap_err();

    assert_eq!(err.code(), "policy.manifest_mismatch");
}

#[test]
fn adapter_parity_signing_capability_matches_legacy_check() {
    let mut doc = PolicyDoc::default();
    doc.merge(
        Path::new("inline"),
        RawPolicy {
            allow: BTreeMap::from([("keystored".to_string(), vec!["crypto.sign".to_string()])]),
            abi_profile: BTreeMap::new(),
            quota: BTreeMap::new(),
        },
    )
    .unwrap();

    let legacy_allow = doc.check(&["crypto.sign"], "keystored").is_ok();
    let unified_allow = doc
        .evaluate(&["crypto.sign"], "keystored", PolicyMode::Enforce)
        .expect("unified eval")
        .allow;
    let legacy_deny = doc.check(&["crypto.verify"], "keystored").is_err();
    let unified_deny = !doc
        .evaluate(&["crypto.verify"], "keystored", PolicyMode::Enforce)
        .expect("unified eval")
        .allow;

    assert_eq!(unified_allow, legacy_allow);
    assert_eq!(unified_deny, legacy_deny);
}

#[test]
fn adapter_parity_exec_capability_matches_legacy_check() {
    let mut doc = PolicyDoc::default();
    doc.merge(
        Path::new("inline"),
        RawPolicy {
            allow: BTreeMap::from([("execd".to_string(), vec!["proc.spawn".to_string()])]),
            abi_profile: BTreeMap::new(),
            quota: BTreeMap::new(),
        },
    )
    .unwrap();

    let legacy_allow = doc.check(&["proc.spawn"], "execd").is_ok();
    let unified_allow =
        doc.evaluate(&["proc.spawn"], "execd", PolicyMode::Enforce).expect("unified eval").allow;
    let legacy_deny = doc.check(&["fs.write"], "execd").is_err();
    let unified_deny =
        !doc.evaluate(&["fs.write"], "execd", PolicyMode::Enforce).expect("unified eval").allow;

    assert_eq!(unified_allow, legacy_allow);
    assert_eq!(unified_deny, legacy_deny);
}
