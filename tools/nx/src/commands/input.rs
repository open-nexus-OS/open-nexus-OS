// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx input` host diagnostics and preflight helpers for TASK-0253.
//! OWNERS: @tools-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 unit tests plus CLI contract coverage in `tests/cli_contract.rs`.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::{
    InputAction, InputArgs, InputCursorArgs, InputKeymapAction, InputKeymapSetArgs,
    InputTestAction, InputTypeArgs,
};
use crate::error::{ExecResult, ExitClass, NxError};
use serde_json::json;

const SUPPORTED_LAYOUTS: &[&str] = &["us", "de", "jp", "kr", "zh"];
const DEFAULT_LAYOUT: &str = "de";
const HOST_PROOFS: &[&str] = &[
    "cargo test -p input_v1_0_host -- --nocapture",
    "cargo test -p hidrawd -- --nocapture",
    "cargo test -p touchd -- --nocapture",
    "cargo test -p inputd -- --nocapture",
];
const OS_PROOFS: &[&str] = &["RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap"];

pub(crate) fn handle_input(args: InputArgs) -> ExecResult {
    match args.action {
        InputAction::Layouts(flags) => Ok((
            ExitClass::Success,
            "input layouts listed".to_string(),
            flags.json,
            Some(layouts_payload()),
        )),
        InputAction::Status(flags) => Ok((
            ExitClass::Success,
            "input authority chain reported".to_string(),
            flags.json,
            Some(status_payload()),
        )),
        InputAction::Proof(flags) => Ok((
            ExitClass::Success,
            "input proof commands listed".to_string(),
            flags.json,
            Some(proof_payload()),
        )),
        InputAction::Devices(flags) => Ok((
            ExitClass::Success,
            "input proof devices listed".to_string(),
            flags.json,
            Some(devices_payload()),
        )),
        InputAction::Keymap(args) => match args.action {
            InputKeymapAction::Get(flags) => Ok((
                ExitClass::Success,
                "input keymap reported".to_string(),
                flags.json,
                Some(json!({
                    "layout": DEFAULT_LAYOUT,
                    "source": "task-0253 diagnostic default"
                })),
            )),
            InputKeymapAction::Set(args) => handle_keymap_set(args),
        },
        InputAction::Test(args) => match args.action {
            InputTestAction::Type(args) => handle_type(args),
        },
        InputAction::Cursor(args) => handle_cursor(args),
    }
}

fn layouts_payload() -> serde_json::Value {
    json!({
        "default_layout": DEFAULT_LAYOUT,
        "supported_layouts": SUPPORTED_LAYOUTS,
    })
}

fn status_payload() -> serde_json::Value {
    json!({
        "authority_chain": ["hidrawd", "touchd", "inputd", "windowd"],
        "windowd_authority": "hit-test/hover/focus/click",
        "ime_scope": "show/hide hooks only",
        "settings_keys": {
            "layout": "keyboard.layout",
            "repeat_delay_ms": "keyboard.repeat.delay_ms",
            "repeat_rate_hz": "keyboard.repeat.rate_hz",
            "pointer_threshold": "pointer.accel.threshold",
            "pointer_ratio": "pointer.accel.ratio",
            "pointer_max_output": "pointer.accel.max_output"
        },
        "proof_profile": "visible-bootstrap"
    })
}

fn proof_payload() -> serde_json::Value {
    json!({
        "host": HOST_PROOFS,
        "os": OS_PROOFS,
        "postflight": "nx postflight input",
    })
}

fn devices_payload() -> serde_json::Value {
    json!({
        "devices": [
            {"id": "kbd-7", "kind": "keyboard", "source": "proof-fixture", "service": "hidrawd"},
            {"id": "mouse-8", "kind": "mouse", "source": "proof-fixture", "service": "hidrawd"},
            {"id": "touch-9", "kind": "touch", "source": "proof-fixture", "service": "touchd"}
        ],
        "proof_profile": "visible-bootstrap"
    })
}

fn handle_keymap_set(args: InputKeymapSetArgs) -> ExecResult {
    let layout = args.layout.to_ascii_lowercase();
    if !SUPPORTED_LAYOUTS.contains(&layout.as_str()) {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("input.keymap.unsupported: {layout}"),
        ));
    }
    Ok((
        ExitClass::Success,
        format!("input keymap preflight accepted: {layout}"),
        args.json,
        Some(json!({
            "layout": layout,
            "marker": format!("nx: input keymap={}", args.layout.to_ascii_lowercase()),
            "preflight_only": true,
            "applied": false
        })),
    ))
}

fn handle_type(args: InputTypeArgs) -> ExecResult {
    let scalar_count = args.text.chars().count();
    if scalar_count == 0 {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "input.type.empty",
        ));
    }
    if scalar_count > 64 {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "input.type.too_long",
        ));
    }
    Ok((
        ExitClass::Success,
        "input type preflight accepted".to_string(),
        args.json,
        Some(json!({
            "text": args.text,
            "scalar_count": scalar_count,
            "utf8_bytes": args.text.len(),
            "preflight_only": true,
            "ime_scope": "show/hide hooks only"
        })),
    ))
}

fn handle_cursor(args: InputCursorArgs) -> ExecResult {
    if args.x < 0 || args.y < 0 {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("input.cursor.out_of_bounds: ({}, {})", args.x, args.y),
        ));
    }
    Ok((
        ExitClass::Success,
        format!("input cursor preflight accepted: ({}, {})", args.x, args.y),
        args.json,
        Some(json!({
            "x": args.x,
            "y": args.y,
            "marker": format!("nx: input cursor set ({},{})", args.x, args.y),
            "preflight_only": true,
            "proof_profile": "visible-bootstrap"
        })),
    ))
}
