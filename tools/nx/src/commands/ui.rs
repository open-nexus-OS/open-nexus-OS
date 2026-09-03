// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx ui preset list/env` (TASK-0055D) — resolves a dev-mode
//! display/profile preset through the SystemUI manifest registry (the ONE
//! authority: `systemui::resolve_preset`) and renders it as the launcher
//! environment `just start` already honours: `QEMU_GPU_XRES/YRES` (→ the
//! fw_cfg `display-mode` key the compositor follows, RFC-0074),
//! `QEMU_PROOF_POINTER_SOURCE` (→ the visible input injector), plus the
//! `NEXUS_PROFILE_INPUT_*` flags and a `NEXUS_UI_PRESET_*` description of
//! the resolved profile/shell/environment. No second parser, no env-only
//! knobs: an invalid or unknown preset is a `validation_reject` with the
//! valid ids listed.
//! OWNERS: @tools-team @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/ui_preset_cli.rs (process boundary)
//! ADR: docs/adr/0035-systemui-declarative-shell-configuration.md

use crate::cli_ui::{UiAction, UiArgs, UiPresetAction, UiPresetEnvArgs, UiPresetListArgs};
use crate::error::{ExecResult, ExitClass, NxError};
use serde_json::json;
use systemui::{DeviceInput, ResolvedPreset, SystemUiError, PRESETS};

pub(crate) fn handle_ui(args: UiArgs) -> ExecResult {
    match args.action {
        UiAction::Preset(p) => match p.action {
            UiPresetAction::List(a) => handle_list(a),
            UiPresetAction::Env(a) => handle_env(a),
        },
    }
}

fn preset_ids() -> Vec<&'static str> {
    PRESETS.iter().map(|e| e.id).collect()
}

fn reject(err: SystemUiError, name: &str) -> NxError {
    let ids = preset_ids().join(", ");
    let why = match err {
        SystemUiError::ManifestNotFound => {
            format!("unknown preset or unregistered profile/shell '{name}'; valid presets: {ids}")
        }
        SystemUiError::UnsupportedShell | SystemUiError::IncompatibleShell => {
            format!("preset '{name}' names a shell its profile does not allow/support")
        }
        SystemUiError::InvalidDisplayMode => {
            format!("preset '{name}' display mode is outside the compositor layout bounds or contradicts its orientation")
        }
        SystemUiError::UnsupportedRefreshRate => {
            format!("preset '{name}' names a refresh rate the compositor cannot pace")
        }
        other => format!("preset '{name}' manifest invalid: {other:?}"),
    };
    NxError::new(ExitClass::ValidationReject, why)
}

fn handle_list(args: UiPresetListArgs) -> ExecResult {
    let mut rows = Vec::new();
    for id in preset_ids() {
        let r = systemui::resolve_preset(id).map_err(|e| reject(e, id))?;
        rows.push(json!({
            "id": r.preset.id,
            "label": r.preset.label,
            "profile": r.env.profile,
            "shell": r.env.shell_mode,
            "width": r.preset.width,
            "height": r.preset.height,
            "hz": r.preset.hz,
            "orientation": r.env.orientation,
            "size_class": r.env.size_class,
            "dpi_class": r.env.dpi_class,
            "pointer_source": pointer_source(&r.preset.input),
        }));
    }
    let message = if args.json {
        format!("{} presets", rows.len())
    } else {
        rows.iter()
            .map(|r| {
                format!(
                    "{:<17} {:<8} {:<8} {}x{}@{} {}",
                    r["id"].as_str().unwrap_or(""),
                    r["profile"].as_str().unwrap_or(""),
                    r["shell"].as_str().unwrap_or(""),
                    r["width"],
                    r["height"],
                    r["hz"],
                    r["orientation"].as_str().unwrap_or(""),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let data = if args.json { Some(json!({ "presets": rows })) } else { None };
    Ok((ExitClass::Success, message, args.json, data))
}

/// The launcher's `QEMU_PROOF_POINTER_SOURCE` vocabulary
/// (`scripts/qemu-launcher.sh`): touch+mouse → `mixed`, touch → `tablet`,
/// mouse → `mouse`, neither → `keyboard`.
fn pointer_source(input: &DeviceInput) -> &'static str {
    match (input.touch, input.mouse) {
        (true, true) => "mixed",
        (true, false) => "tablet",
        (false, true) => "mouse",
        (false, false) => "keyboard",
    }
}

/// The `KEY=VALUE` lines a POSIX shell can `eval`/source (values are
/// manifest-validated identifiers and integers — never quoted, never
/// user-controlled beyond the registered catalog).
pub(crate) fn launch_env(r: &ResolvedPreset) -> Vec<(&'static str, String)> {
    let i = &r.preset.input;
    vec![
        ("NEXUS_UI_PRESET", r.preset.id.clone()),
        ("QEMU_GPU_XRES", r.preset.width.to_string()),
        ("QEMU_GPU_YRES", r.preset.height.to_string()),
        ("QEMU_PROOF_POINTER_SOURCE", pointer_source(i).to_string()),
        ("NEXUS_PROFILE_INPUT_TOUCH", u8::from(i.touch).to_string()),
        ("NEXUS_PROFILE_INPUT_MOUSE", u8::from(i.mouse).to_string()),
        ("NEXUS_PROFILE_INPUT_KBD", u8::from(i.kbd).to_string()),
        ("NEXUS_PROFILE_INPUT_REMOTE", u8::from(i.remote).to_string()),
        ("NEXUS_PROFILE_INPUT_ROTARY", u8::from(i.rotary).to_string()),
        ("NEXUS_UI_PRESET_PROFILE", r.env.profile.clone()),
        ("NEXUS_UI_PRESET_SHELL", r.env.shell_mode.clone()),
        ("NEXUS_UI_PRESET_ORIENTATION", r.env.orientation.clone()),
        ("NEXUS_UI_PRESET_SIZE_CLASS", r.env.size_class.clone()),
        ("NEXUS_UI_PRESET_DPI_CLASS", r.env.dpi_class.clone()),
        ("NEXUS_UI_PRESET_HZ", r.preset.hz.to_string()),
    ]
}

fn handle_env(args: UiPresetEnvArgs) -> ExecResult {
    let resolved = systemui::resolve_preset(&args.name).map_err(|e| reject(e, &args.name))?;
    let env = launch_env(&resolved);
    let message = env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
    let data = if args.json {
        let map: serde_json::Map<String, serde_json::Value> =
            env.iter().map(|(k, v)| ((*k).to_string(), json!(v))).collect();
        Some(json!({ "preset": resolved.preset.id, "env": map }))
    } else {
        None
    };
    Ok((ExitClass::Success, message, args.json, data))
}

#[cfg(test)]
mod tests {
    use super::{launch_env, pointer_source};
    use systemui::DeviceInput;

    fn input(touch: bool, mouse: bool) -> DeviceInput {
        DeviceInput { touch, mouse, kbd: true, remote: false, rotary: false }
    }

    #[test]
    fn pointer_source_matches_launcher_vocabulary() {
        assert_eq!(pointer_source(&input(true, true)), "mixed");
        assert_eq!(pointer_source(&input(true, false)), "tablet");
        assert_eq!(pointer_source(&input(false, true)), "mouse");
        assert_eq!(pointer_source(&input(false, false)), "keyboard");
    }

    #[test]
    fn baseline_preset_env_is_the_default_launch() {
        let r = systemui::resolve_preset(systemui::BASELINE_PRESET_ID).expect("baseline");
        let env = launch_env(&r);
        let get = |k: &str| env.iter().find(|(key, _)| *key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("QEMU_GPU_XRES"), Some("1280"));
        assert_eq!(get("QEMU_GPU_YRES"), Some("800"));
        assert_eq!(get("QEMU_PROOF_POINTER_SOURCE"), Some("tablet"));
        assert_eq!(get("NEXUS_UI_PRESET_SHELL"), Some("tablet"));
        assert_eq!(get("NEXUS_UI_PRESET_SIZE_CLASS"), Some("wide"));
    }

    #[test]
    fn env_values_are_shell_safe_tokens() {
        for id in systemui::PRESETS.iter().map(|e| e.id) {
            let r = systemui::resolve_preset(id).expect("registered preset resolves");
            for (_, v) in launch_env(&r) {
                assert!(
                    v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                    "{id}: value {v:?} would need quoting"
                );
            }
        }
    }
}
