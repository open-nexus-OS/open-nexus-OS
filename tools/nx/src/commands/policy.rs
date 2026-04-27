// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx policy` host-first Policy-as-Code command surface.
//! OWNERS: @tools-team @security
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0014-policy-architecture.md

use crate::cli::{
    PolicyAction, PolicyArgs, PolicyCliMode, PolicyDiffArgs, PolicyExplainArgs, PolicyModeArgs,
    PolicyValidateArgs,
};
use crate::error::{ExecResult, ExitClass, NxError};
use crate::runtime::RuntimeConfig;
use nexus_policy::{PolicyMode, PolicyTree};
use serde_json::json;
use std::path::{Path, PathBuf};

pub(crate) fn handle_policy(args: PolicyArgs, cfg: &RuntimeConfig) -> ExecResult {
    match args.action {
        PolicyAction::Validate(a) => handle_policy_validate(a, cfg),
        PolicyAction::Diff(a) => handle_policy_diff(a),
        PolicyAction::Explain(a) => handle_policy_explain(a, cfg),
        PolicyAction::Mode(a) => handle_policy_mode(a, cfg),
    }
}

fn handle_policy_validate(args: PolicyValidateArgs, cfg: &RuntimeConfig) -> ExecResult {
    let root = policy_root(cfg, args.root);
    let tree = load_tree(&root)?;
    tree.validate_manifest(&root).map_err(|err| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("{}: {err}", err.code()),
        )
    })?;
    let data = json!({
        "root": root,
        "version": tree.version().as_str(),
        "manifest": true,
        "subjects": tree.policy().subject_count(),
        "capabilities": tree.policy().capability_count(),
    });
    Ok((
        ExitClass::Success,
        "policy validate passed".to_string(),
        args.json,
        Some(data),
    ))
}

fn handle_policy_diff(args: PolicyDiffArgs) -> ExecResult {
    let from = load_tree(&args.from)?;
    let to = load_tree(&args.to)?;
    let changed = from.version() != to.version();
    let data = json!({
        "changed": changed,
        "from_version": from.version().as_str(),
        "to_version": to.version().as_str(),
    });
    Ok((
        ExitClass::Success,
        "policy diff generated".to_string(),
        args.json,
        Some(data),
    ))
}

fn handle_policy_explain(args: PolicyExplainArgs, cfg: &RuntimeConfig) -> ExecResult {
    if args.caps.is_empty() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "policy explain requires at least one --cap",
        ));
    }
    let root = policy_root(cfg, args.root);
    let tree = load_tree(&root)?;
    let caps = args.caps.iter().map(String::as_str).collect::<Vec<_>>();
    let decision = tree
        .policy()
        .evaluate(&caps, &args.subject, args.mode.into())
        .map_err(|err| NxError::new(ExitClass::ValidationReject, err.to_string()))?;
    let data = json!({
        "root": root,
        "version": tree.version().as_str(),
        "decision": decision,
    });
    Ok((
        ExitClass::Success,
        "policy explain generated".to_string(),
        args.json,
        Some(data),
    ))
}

fn handle_policy_mode(args: PolicyModeArgs, cfg: &RuntimeConfig) -> ExecResult {
    let root = policy_root(cfg, args.root);
    let tree = load_tree(&root)?;
    if args.actor_service_id == 0 || !args.authorized {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "policy mode change rejected: unauthorized",
        ));
    }
    if args.observed_version != tree.version().as_str() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "policy mode change rejected: stale observed version",
        ));
    }
    let data = json!({
        "root": root,
        "version": tree.version().as_str(),
        "mode": args.set,
        "applied": false,
        "preflight_only": true,
    });
    Ok((
        ExitClass::Success,
        "policy mode preflight accepted".to_string(),
        args.json,
        Some(data),
    ))
}

fn policy_root(cfg: &RuntimeConfig, root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| cfg.repo_root.join("policies"))
}

fn load_tree(root: &Path) -> Result<PolicyTree, NxError> {
    PolicyTree::load_root(root).map_err(|err| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("{}: {err}", err.code()),
        )
    })
}

impl From<PolicyCliMode> for PolicyMode {
    fn from(value: PolicyCliMode) -> Self {
        match value {
            PolicyCliMode::Enforce => Self::Enforce,
            PolicyCliMode::DryRun => Self::DryRun,
            PolicyCliMode::Learn => Self::Learn,
        }
    }
}
