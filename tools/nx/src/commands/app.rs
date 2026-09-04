// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx app compile` (TASK-0321 P5): the app's canonical
//! ui-program payload (`nexus_dsl_core::compile_project_bundle` — the same
//! byte-deterministic compile bundlemgrd's build.rs used to bake into the
//! boot image) plus the `meta/app.properties` sidecar (`label=`, `icon=`,
//! `bundle_type=` from `manifest.toml`) that the volume registry reads.
//! OWNERS: @tools-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/app_cli.rs (real app project → payload + sidecar)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use serde_json::json;

use crate::cli_app::{AppAction, AppArgs, AppCompileArgs};
use crate::error::{ExecResult, ExitClass, NxError};

pub(crate) fn handle_app(args: AppArgs) -> ExecResult {
    match args.action {
        AppAction::Compile(args) => compile(args),
    }
}

/// Minimal `key = "value"` reader for the fields the sidecar needs (the
/// manifest is validated in full by `nxb-pack`; this only extracts).
fn manifest_field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix(key)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim();
        let val = rest.trim_matches('"');
        (!val.is_empty()).then(|| val.to_string())
    })
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// The sidecar text bundlemgrd parses (`volume::parse_app_properties`).
pub(crate) fn app_properties(manifest_toml: &str) -> Result<String, NxError> {
    let name = manifest_field(manifest_toml, "name").ok_or_else(|| {
        NxError::new(ExitClass::ValidationReject, "app: manifest.toml has no name")
    })?;
    let label = manifest_field(manifest_toml, "label").unwrap_or_else(|| capitalize(&name));
    let bundle_type = manifest_field(manifest_toml, "bundle_type").unwrap_or_else(|| "app".into());
    let icon = manifest_field(manifest_toml, "icon").unwrap_or_default();
    Ok(format!("label={label}\nicon={icon}\nbundle_type={bundle_type}\npayload_kind=ui-program\n"))
}

fn compile(args: AppCompileArgs) -> ExecResult {
    let manifest_path = args.app.join("manifest.toml");
    let manifest = std::fs::read_to_string(&manifest_path).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("app: read {}: {err}", manifest_path.display()),
        )
    })?;
    if manifest_field(&manifest, "payload_kind").as_deref() != Some("ui-program") {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("app: {} is not a ui-program bundle", manifest_path.display()),
        ));
    }
    let payload = nexus_dsl_core::compile_project_bundle(&args.app).map_err(|err| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("app: compile {}: {err}", args.app.display()),
        )
    })?;
    if let Some(parent) = args.out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&args.out, &payload).map_err(|err| {
        NxError::new(ExitClass::Internal, format!("app: write {}: {err}", args.out.display()))
    })?;
    let properties = app_properties(&manifest)?;
    if let Some(meta) = &args.meta {
        if let Some(parent) = meta.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(meta, &properties).map_err(|err| {
            NxError::new(ExitClass::Internal, format!("app: write {}: {err}", meta.display()))
        })?;
    }
    Ok((
        ExitClass::Success,
        format!("app: compiled {} ({} bytes)", args.out.display(), payload.len()),
        args.json,
        Some(json!({
            "out": args.out.display().to_string(),
            "bytes": payload.len(),
            "properties": properties,
        })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_from_manifest_fields_with_defaults() {
        let text = "name = \"calculator\"\nbundle_type = \"app\"\nicon = \"calculator|#3d4757|#1a202c\"\npayload_kind = \"ui-program\"\n";
        assert_eq!(
            app_properties(text).unwrap(),
            "label=Calculator\nicon=calculator|#3d4757|#1a202c\nbundle_type=app\npayload_kind=ui-program\n"
        );
        assert!(app_properties("bundle_type = \"app\"\n").is_err());
    }
}
