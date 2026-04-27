// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx postflight` allowlisted delegate runner.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::PostflightArgs;
use crate::error::{ExecResult, ExitClass, NxError};
use crate::output::bounded_tail;
use crate::runtime::RuntimeConfig;
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Command;
use std::time::Instant;

fn postflight_topics() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("kspawn", "postflight-kspawn.sh"),
        ("loader", "postflight-loader.sh"),
        ("loader-v1_1", "postflight-loader-v1_1.sh"),
        ("min-exec", "postflight-min-exec.sh"),
        ("policy", "postflight-policy.sh"),
        ("proc", "postflight-proc.sh"),
        ("vfs", "postflight-vfs.sh"),
        ("vfs-userspace", "postflight-vfs-userspace.sh"),
    ])
}

pub(crate) fn handle_postflight(args: PostflightArgs, cfg: &RuntimeConfig) -> ExecResult {
    let topics = postflight_topics();
    let Some(script_name) = topics.get(args.topic.as_str()) else {
        let valid_topics = topics.keys().copied().collect::<Vec<_>>();
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!(
                "unknown postflight topic '{}'; valid topics: {}",
                args.topic,
                valid_topics.join(", ")
            ),
        ));
    };
    let script_path = cfg.postflight_dir.join(script_name);
    if !script_path.exists() {
        return Err(NxError::new(
            ExitClass::Unsupported,
            format!("postflight script not available: {}", script_path.display()),
        ));
    }

    let start = Instant::now();
    let output = Command::new(&script_path).output().map_err(|e| {
        NxError::new(
            ExitClass::DelegateFailure,
            format!("failed to execute delegate {}: {e}", script_path.display()),
        )
    })?;
    let elapsed_ms = start.elapsed().as_millis();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail = bounded_tail(&[stdout.as_ref(), stderr.as_ref()].join("\n"), args.tail);

    let data = json!({
        "topic": args.topic,
        "script": script_path,
        "delegate_exit": output.status.code().unwrap_or(-1),
        "elapsed_ms": elapsed_ms,
        "tail": tail,
    });

    if output.status.success() {
        Ok((
            ExitClass::Success,
            "postflight delegate succeeded".to_string(),
            args.json,
            Some(data),
        ))
    } else {
        Ok((
            ExitClass::DelegateFailure,
            "postflight delegate failed".to_string(),
            args.json,
            Some(data),
        ))
    }
}
