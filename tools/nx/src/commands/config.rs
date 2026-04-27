// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx config` deterministic Config v1 command implementation.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::{
    ConfigAction, ConfigArgs, ConfigDiffArgs, ConfigEffectiveArgs, ConfigPushArgs,
    ConfigReloadArgs, ConfigValidateArgs, ConfigWhereArgs,
};
use crate::error::{ExecResult, ExitClass, NxError};
use crate::runtime::RuntimeConfig;
use configd::{Configd, ReloadReport};
use nexus_config::{
    build_effective_snapshot, env_overrides_from_pairs, load_config_path, load_layer_dir,
    LayerInputs, STATE_CONFIG_FILENAME,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn handle_config(args: ConfigArgs, cfg: &RuntimeConfig) -> ExecResult {
    match args.action {
        ConfigAction::Validate(a) => handle_config_validate(a, cfg),
        ConfigAction::Effective(a) => handle_config_effective(a, cfg),
        ConfigAction::Diff(a) => handle_config_diff(a, cfg),
        ConfigAction::Push(a) => handle_config_push(a, cfg),
        ConfigAction::Reload(a) => handle_config_reload(a, cfg),
        ConfigAction::Where(a) => handle_config_where(a, cfg),
    }
}

fn handle_config_validate(args: ConfigValidateArgs, cfg: &RuntimeConfig) -> ExecResult {
    if args.paths.is_empty() {
        let _ = load_layers_from_repo(cfg)?;
        return Ok((
            ExitClass::Success,
            "config validate passed for layered sources".to_string(),
            args.json,
            None,
        ));
    }

    for path in &args.paths {
        let overlay = load_config_path(path)
            .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
        let mut layers = LayerInputs::with_defaults_only();
        layers.state = overlay;
        build_effective_snapshot(layers)
            .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    }

    Ok((
        ExitClass::Success,
        format!("config validate passed for {} file(s)", args.paths.len()),
        args.json,
        None,
    ))
}

fn handle_config_effective(args: ConfigEffectiveArgs, cfg: &RuntimeConfig) -> ExecResult {
    let layers = load_layers_from_repo(cfg)?;
    let snapshot = build_effective_snapshot(layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let data = json!({
        "version": snapshot.version,
        "effective": snapshot.merged_json,
    });
    Ok((
        ExitClass::Success,
        "effective config generated".to_string(),
        args.json,
        Some(data),
    ))
}

fn handle_config_diff(args: ConfigDiffArgs, _cfg: &RuntimeConfig) -> ExecResult {
    let from_overlay = read_overlay_file(&args.from)?;
    let to_overlay = read_overlay_file(&args.to)?;

    let mut from_layers = LayerInputs::with_defaults_only();
    from_layers.state = from_overlay;
    let from_snapshot = build_effective_snapshot(from_layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let mut to_layers = LayerInputs::with_defaults_only();
    to_layers.state = to_overlay;
    let to_snapshot = build_effective_snapshot(to_layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let changed = from_snapshot.version != to_snapshot.version;
    let data = json!({
        "changed": changed,
        "from_version": from_snapshot.version,
        "to_version": to_snapshot.version,
        "from_effective": from_snapshot.merged_json,
        "to_effective": to_snapshot.merged_json,
    });
    Ok((
        ExitClass::Success,
        "config diff generated".to_string(),
        args.json,
        Some(data),
    ))
}

fn handle_config_push(args: ConfigPushArgs, cfg: &RuntimeConfig) -> ExecResult {
    let bytes = fs::read(&args.file).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!("failed reading push source '{}': {e}", args.file.display()),
        )
    })?;
    let overlay = load_config_path(&args.file)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let mut layers = LayerInputs::with_defaults_only();
    layers.state = overlay;
    build_effective_snapshot(layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let state_dir = cfg.repo_root.join("state/config");
    fs::create_dir_all(&state_dir).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!(
                "failed creating state config directory '{}': {e}",
                state_dir.display()
            ),
        )
    })?;
    let state_path = state_dir.join(format!("{STATE_CONFIG_FILENAME}.json"));
    fs::write(&state_path, bytes).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!(
                "failed writing state config '{}': {e}",
                state_path.display()
            ),
        )
    })?;
    Ok((
        ExitClass::Success,
        format!("config pushed to {}", state_path.display()),
        args.json,
        Some(json!({ "path": state_path })),
    ))
}

fn handle_config_reload(args: ConfigReloadArgs, cfg: &RuntimeConfig) -> ExecResult {
    let layers = load_layers_from_repo(cfg)?;
    let mut daemon = Configd::new(layers.clone())
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let report: ReloadReport = daemon
        .reload(layers)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    let class = if report.committed {
        ExitClass::Success
    } else {
        ExitClass::DelegateFailure
    };
    let message = if report.committed {
        "config reload committed".to_string()
    } else {
        "config reload aborted".to_string()
    };
    let data = json!({
        "committed": report.committed,
        "from_version": report.from_version,
        "candidate_version": report.candidate_version,
        "active_version": report.active_version,
        "reason": report.reason,
    });
    Ok((class, message, args.json, Some(data)))
}

fn handle_config_where(args: ConfigWhereArgs, cfg: &RuntimeConfig) -> ExecResult {
    let data = config_paths(cfg);
    Ok((
        ExitClass::Success,
        "config source paths".to_string(),
        args.json,
        Some(json!(data)),
    ))
}

pub(crate) fn config_paths(cfg: &RuntimeConfig) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "system".to_string(),
            cfg.repo_root.join("system/config").display().to_string(),
        ),
        (
            "state".to_string(),
            cfg.repo_root.join("state/config").display().to_string(),
        ),
        ("env_prefix".to_string(), "NEXUS_CFG_".to_string()),
    ])
}

pub(crate) fn load_layers_from_repo(cfg: &RuntimeConfig) -> Result<LayerInputs, NxError> {
    let mut layers = LayerInputs::with_defaults_only();
    let paths = config_paths(cfg);
    let system_path = PathBuf::from(
        paths
            .get("system")
            .ok_or_else(|| NxError::new(ExitClass::Internal, "missing system config path"))?,
    );
    let state_path = PathBuf::from(
        paths
            .get("state")
            .ok_or_else(|| NxError::new(ExitClass::Internal, "missing state config path"))?,
    );

    layers.system = load_layer_dir(&system_path)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;
    layers.state = load_layer_dir(&state_path)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    let env_pairs = std::env::vars()
        .filter(|(k, _)| k.starts_with("NEXUS_CFG_"))
        .collect::<BTreeMap<_, _>>();
    layers.env = env_overrides_from_pairs(&env_pairs)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))?;

    Ok(layers)
}

fn read_overlay_file(path: &Path) -> Result<Value, NxError> {
    load_config_path(path).map_err(|e| NxError::new(ExitClass::ValidationReject, e.to_string()))
}
