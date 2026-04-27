// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx doctor` local dependency checks.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::DoctorArgs;
use crate::error::{ExecResult, ExitClass};
use serde_json::json;
use std::ffi::OsStr;

pub(crate) fn handle_doctor(args: DoctorArgs) -> ExecResult {
    handle_doctor_with_path(args, std::env::var_os("PATH"))
}

pub(crate) fn handle_doctor_with_path(
    args: DoctorArgs,
    path_var: Option<std::ffi::OsString>,
) -> ExecResult {
    let required = ["rustc", "cargo", "just", "qemu-system-riscv64", "capnp"];
    let optional = ["rg", "python3"];

    let mut missing_required = Vec::new();
    let mut found = serde_json::Map::new();

    for tool in required {
        let found_path = which_in_path(tool, path_var.as_deref());
        if found_path.is_none() {
            missing_required.push(tool.to_string());
        }
        found.insert(
            tool.to_string(),
            json!({
                "required": true,
                "found": found_path.is_some(),
                "path": found_path,
            }),
        );
    }
    for tool in optional {
        let found_path = which_in_path(tool, path_var.as_deref());
        found.insert(
            tool.to_string(),
            json!({
                "required": false,
                "found": found_path.is_some(),
                "path": found_path,
            }),
        );
    }

    let data = json!({
        "tools": found,
        "missing_required": missing_required,
        "hint": "Install missing required tools and rerun nx doctor"
    });

    if data["missing_required"]
        .as_array()
        .map(|v| v.is_empty())
        .unwrap_or(false)
    {
        Ok((
            ExitClass::Success,
            "doctor passed".to_string(),
            args.json,
            Some(data),
        ))
    } else {
        Ok((
            ExitClass::MissingDependency,
            "doctor detected missing required tools".to_string(),
            args.json,
            Some(data),
        ))
    }
}

pub(crate) fn which_in_path(bin: &str, path_var: Option<&OsStr>) -> Option<String> {
    let paths = path_var?;
    for path in std::env::split_paths(paths) {
        let full = path.join(bin);
        if full.is_file() {
            return Some(full.display().to_string());
        }
    }
    None
}
