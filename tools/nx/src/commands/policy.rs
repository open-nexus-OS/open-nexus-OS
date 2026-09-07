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
    PolicyAction, PolicyArgs, PolicyCliMode, PolicyDiffArgs, PolicyExplainArgs, PolicyLearnGenArgs,
    PolicyModeArgs, PolicyValidateArgs,
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
        PolicyAction::LearnGen(a) => handle_policy_learn_gen(a),
    }
}

fn handle_policy_validate(args: PolicyValidateArgs, cfg: &RuntimeConfig) -> ExecResult {
    let root = policy_root(cfg, args.root);
    let tree = load_tree(&root)?;
    if args.write_manifest {
        tree.write_manifest(&root).map_err(|err| {
            NxError::new(ExitClass::ValidationReject, format!("{}: {err}", err.code()))
        })?;
    }
    tree.validate_manifest(&root).map_err(|err| {
        NxError::new(ExitClass::ValidationReject, format!("{}: {err}", err.code()))
    })?;
    let data = json!({
        "root": root,
        "version": tree.version().as_str(),
        "manifest": true,
        "subjects": tree.policy().subject_count(),
        "capabilities": tree.policy().capability_count(),
    });
    Ok((ExitClass::Success, "policy validate passed".to_string(), args.json, Some(data)))
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
    Ok((ExitClass::Success, "policy diff generated".to_string(), args.json, Some(data)))
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
    Ok((ExitClass::Success, "policy explain generated".to_string(), args.json, Some(data)))
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
    Ok((ExitClass::Success, "policy mode preflight accepted".to_string(), args.json, Some(data)))
}

/// Largest learn log the generator reads (a boot's logd dump is kilobytes).
const MAX_LEARN_LOG_BYTES: u64 = 4 * 1024 * 1024;

fn handle_policy_learn_gen(args: PolicyLearnGenArgs) -> ExecResult {
    let subject = args.subject.trim();
    if subject.is_empty() || subject.contains('"') || subject.contains('\n') {
        return Err(NxError::new(ExitClass::ValidationReject, "learn-gen: invalid --subject"));
    }
    let meta = std::fs::metadata(&args.learn_log).map_err(|err| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("learn-gen: cannot read learn log: {err}"),
        )
    })?;
    if meta.len() > MAX_LEARN_LOG_BYTES {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!(
                "learn-gen: learn log too large ({} > {MAX_LEARN_LOG_BYTES} bytes)",
                meta.len()
            ),
        ));
    }
    let log = std::fs::read_to_string(&args.learn_log).map_err(|err| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("learn-gen: cannot read learn log: {err}"),
        )
    })?;
    let opts = nexus_policy::learn_gen::GenOptions {
        subject_name: subject.to_string(),
        subject_id: nexus_policy::learn_record::service_id_from_name(subject.as_bytes()),
        allow_any: args.allow_any,
    };
    let generated = nexus_policy::learn_gen::generate(log.lines(), &opts);
    std::fs::write(&args.out, &generated.toml).map_err(|err| {
        NxError::new(
            ExitClass::Internal,
            format!("learn-gen: cannot write {}: {err}", args.out.display()),
        )
    })?;
    let data = json!({
        "learn_log": args.learn_log,
        "out": args.out,
        "subject": subject,
        "subject_id": format!("{:016x}", opts.subject_id),
        "epoch_observed": generated.epoch,
        "records_parsed": generated.records_parsed,
        "lines_ignored": generated.lines_ignored,
        "records_for_subject": generated.records_for_subject,
        "unique_args": generated.unique_args,
        "rules_emitted": generated.rules_emitted,
        "rules_held": generated.rules_held,
        "rules_capped": generated.rules_capped,
        "allow_any": args.allow_any,
    });
    Ok((
        ExitClass::Success,
        format!(
            "policy learn-gen wrote {} ({} rules, {} held, {} capped) — review before enabling",
            args.out.display(),
            generated.rules_emitted,
            generated.rules_held,
            generated.rules_capped
        ),
        args.json,
        Some(data),
    ))
}

fn policy_root(cfg: &RuntimeConfig, root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| cfg.repo_root.join("policies"))
}

fn load_tree(root: &Path) -> Result<PolicyTree, NxError> {
    PolicyTree::load_root(root)
        .map_err(|err| NxError::new(ExitClass::ValidationReject, format!("{}: {err}", err.code())))
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
