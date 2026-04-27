// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx dsl` deterministic delegate wrapper.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::{DslAction, DslArgs};
use crate::error::{ExecResult, ExitClass, NxError};
use crate::output::bounded_tail;
use crate::runtime::RuntimeConfig;
use serde_json::json;
use std::process::Command;

pub(crate) fn handle_dsl(args: DslArgs, cfg: &RuntimeConfig) -> ExecResult {
    let backend = match &cfg.dsl_backend {
        Some(path) => path.clone(),
        None => {
            return Ok((
                ExitClass::Unsupported,
                "dsl backend unsupported; set NX_DSL_BACKEND to enable delegation".to_string(),
                args.json,
                Some(json!({
                    "action": format!("{:?}", args.action).to_lowercase(),
                    "classification": "unsupported",
                })),
            ));
        }
    };

    if !backend.exists() {
        return Ok((
            ExitClass::Unsupported,
            format!(
                "dsl backend unsupported; backend not found: {}",
                backend.display()
            ),
            args.json,
            Some(json!({
                "backend": backend,
                "classification": "unsupported",
            })),
        ));
    }

    let action = match args.action {
        DslAction::Fmt => "fmt",
        DslAction::Lint => "lint",
        DslAction::Build => "build",
    };
    let output = Command::new(&backend)
        .arg(action)
        .args(&args.args)
        .output()
        .map_err(|e| {
            NxError::new(
                ExitClass::DelegateFailure,
                format!("failed executing dsl delegate: {e}"),
            )
        })?;

    let data = json!({
        "backend": backend,
        "action": action,
        "delegate_exit": output.status.code().unwrap_or(-1),
        "tail": bounded_tail(&String::from_utf8_lossy(&output.stderr), 40),
    });

    if output.status.success() {
        Ok((
            ExitClass::Success,
            "dsl delegate succeeded".to_string(),
            args.json,
            Some(data),
        ))
    } else {
        Ok((
            ExitClass::DelegateFailure,
            "dsl delegate failed".to_string(),
            args.json,
            Some(data),
        ))
    }
}
