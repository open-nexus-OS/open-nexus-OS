// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx new` scaffold command implementation.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::{NewArgs, NewKind};
use crate::error::{ExecResult, ExitClass, NxError};
use crate::runtime::RuntimeConfig;
use serde_json::json;
use std::fs;
use std::path::{Component, Path};

const CARGO_TOML_TEMPLATE: &str = include_str!("../../templates/Cargo.toml.tpl");
const MAIN_RS_TEMPLATE: &str = include_str!("../../templates/main.rs.tpl");
const STUB_README_TEMPLATE: &str = include_str!("../../templates/stub-readme.md.tpl");

fn validate_name(name: &str) -> Result<(), NxError> {
    if name.is_empty() {
        return Err(NxError::new(ExitClass::ValidationReject, "name must not be empty"));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "name rejects traversal or path separators",
        ));
    }
    Ok(())
}

fn validate_relative_root(root: &Path) -> Result<(), NxError> {
    if root.is_absolute() {
        return Err(NxError::new(ExitClass::ValidationReject, "absolute root path is rejected"));
    }
    if root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(NxError::new(ExitClass::ValidationReject, "root path traversal is rejected"));
    }
    Ok(())
}

pub(crate) fn handle_new(args: NewArgs, cfg: &RuntimeConfig) -> ExecResult {
    let (kind, item_args, base_path, template_title) = match args.kind {
        NewKind::Service(a) => ("service", a, Path::new("source/services"), "service"),
        NewKind::App(a) => ("app", a, Path::new("userspace/apps"), "app"),
        NewKind::Test(a) => ("test", a, Path::new("tests"), "test"),
    };

    validate_name(&item_args.name)?;
    if let Some(root) = &item_args.root {
        validate_relative_root(root)?;
    }
    let root = cfg.repo_root.join(item_args.root.as_deref().unwrap_or(Path::new(".")));
    let target_name =
        if kind == "test" { format!("{}_host", item_args.name) } else { item_args.name.clone() };
    let target_dir = root.join(base_path).join(&target_name);

    if target_dir.exists() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("target already exists: {}", target_dir.display()),
        ));
    }

    fs::create_dir_all(target_dir.join("src"))
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed creating tree: {e}")))?;
    fs::create_dir_all(target_dir.join("docs/stubs")).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("failed creating docs tree: {e}"))
    })?;

    let cargo_toml = CARGO_TOML_TEMPLATE.replace("{{CRATE_NAME}}", &target_name.replace('-', "_"));
    let main_rs = MAIN_RS_TEMPLATE.to_string();
    let stub_doc = STUB_README_TEMPLATE.replace("{{KIND}}", template_title);

    fs::write(target_dir.join("Cargo.toml"), cargo_toml).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("failed writing Cargo.toml: {e}"))
    })?;
    fs::write(target_dir.join("src/main.rs"), main_rs)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed writing main.rs: {e}")))?;
    fs::write(target_dir.join("docs/stubs/README.md"), stub_doc).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("failed writing stub README: {e}"))
    })?;

    let message = format!(
        "created {kind} scaffold at {}; workspace manifest not edited; add member manually",
        target_dir.display()
    );
    let data = json!({
        "kind": kind,
        "target": target_dir,
        "next_step": "manually register workspace member"
    });
    Ok((ExitClass::Success, message, item_args.json, Some(data)))
}
