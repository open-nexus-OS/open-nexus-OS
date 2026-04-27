// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Human and JSON output envelope helpers for `nx`.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::error::ExitClass;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct OutputEnvelope {
    ok: bool,
    class: &'static str,
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

pub(crate) fn print_result(
    class: ExitClass,
    message: String,
    json_mode: bool,
    data: Option<Value>,
) {
    if json_mode {
        let payload = OutputEnvelope {
            ok: class == ExitClass::Success,
            class: class.label(),
            code: class.code(),
            message,
            data,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{\"ok\":false}".to_string())
        );
        return;
    }

    println!("{message}");
    if let Some(data) = data {
        println!("{}", serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_string()));
    }
}

pub(crate) fn bounded_tail(input: &str, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let lines = input.lines().map(ToString::to_string).collect::<Vec<_>>();
    let len = lines.len();
    if len <= max_lines {
        return lines;
    }
    lines[len - max_lines..].to_vec()
}
