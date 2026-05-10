// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx idl` schema inventory commands.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::{IdlAction, IdlArgs};
use crate::error::{ExecResult, ExitClass, NxError};
use crate::runtime::RuntimeConfig;
use serde_json::json;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

fn idl_root(cfg: &RuntimeConfig, root: Option<PathBuf>) -> PathBuf {
    match root {
        Some(p) => cfg.repo_root.join(p),
        None => cfg.repo_root.join("tools/nexus-idl/schemas"),
    }
}

pub(crate) fn handle_idl(args: IdlArgs, cfg: &RuntimeConfig) -> ExecResult {
    match args.action {
        IdlAction::List(list) => {
            let root = idl_root(cfg, list.root);
            let schemas = list_schemas(&root)?;
            let data = json!({
                "root": root,
                "schemas": schemas
            });
            Ok((
                ExitClass::Success,
                format!(
                    "listed {} schema file(s)",
                    data["schemas"].as_array().map(|v| v.len()).unwrap_or(0)
                ),
                list.json,
                Some(data),
            ))
        }
        IdlAction::Check(check) => {
            let root = idl_root(cfg, check.root);
            let schemas = list_schemas(&root)?;
            let capnp_ok = which("capnp").is_some();
            if !capnp_ok {
                return Err(NxError::new(
                    ExitClass::MissingDependency,
                    "required tool missing: capnp",
                ));
            }
            let data = json!({
                "root": root,
                "schema_count": schemas.len(),
                "capnp": capnp_ok,
            });
            Ok((ExitClass::Success, "idl check passed".to_string(), check.json, Some(data)))
        }
    }
}

fn list_schemas(root: &Path) -> Result<Vec<String>, NxError> {
    if !root.exists() || !root.is_dir() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("idl root does not exist: {}", root.display()),
        ));
    }
    let mut schemas = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed reading idl root: {e}")))?
    {
        let entry = entry.map_err(|e| {
            NxError::new(ExitClass::Internal, format!("failed iterating idl root: {e}"))
        })?;
        let path = entry.path();
        if path.extension() == Some(OsStr::new("capnp")) {
            schemas.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    schemas.sort();
    if schemas.is_empty() {
        return Err(NxError::new(ExitClass::ValidationReject, "no schema files found in idl root"));
    }
    Ok(schemas)
}

fn which(bin: &str) -> Option<String> {
    let paths = std::env::var_os("PATH")?;
    super::doctor::which_in_path(bin, Some(&paths))
}
