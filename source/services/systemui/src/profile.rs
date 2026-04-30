// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal TOML-backed SystemUI profile seed for TASK-0055C.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const DESKTOP_PROFILE_TOML: &str = include_str!("../manifests/profiles/desktop/profile.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemUiError {
    InvalidManifest,
    MissingField,
    UnsupportedProfile,
    UnsupportedShell,
    IncompatibleShell,
    InvalidFrameDimensions,
    ArithmeticOverflow,
}

pub type Result<T> = core::result::Result<T, SystemUiError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInput {
    pub touch: bool,
    pub mouse: bool,
    pub kbd: bool,
    pub remote: bool,
    pub rotary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayDefaults {
    pub orientation: String,
    pub dpi_class: String,
    pub size_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileManifest {
    pub id: String,
    pub label: String,
    pub default_shell: String,
    pub allowed_shells: Vec<String>,
    pub input: DeviceInput,
    pub display_defaults: DisplayDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TomlEntry {
    section: String,
    key: String,
    value: String,
}

pub fn desktop_profile() -> Result<ProfileManifest> {
    parse_profile_manifest(DESKTOP_PROFILE_TOML)
}

pub fn parse_profile_manifest(input: &str) -> Result<ProfileManifest> {
    let entries = parse_entries(input)?;
    let manifest = ProfileManifest {
        id: string_field(&entries, "", "id")?,
        label: string_field(&entries, "", "label")?,
        default_shell: string_field(&entries, "", "default_shell")?,
        allowed_shells: string_array_field(&entries, "", "allowed_shells")?,
        input: DeviceInput {
            touch: bool_field(&entries, "input", "touch")?,
            mouse: bool_field(&entries, "input", "mouse")?,
            kbd: bool_field(&entries, "input", "kbd")?,
            remote: bool_field(&entries, "input", "remote")?,
            rotary: bool_field(&entries, "input", "rotary")?,
        },
        display_defaults: DisplayDefaults {
            orientation: string_field(&entries, "display_defaults", "orientation")?,
            dpi_class: string_field(&entries, "display_defaults", "dpi_class")?,
            size_class: string_field(&entries, "display_defaults", "size_class")?,
        },
    };
    validate_profile(&manifest)?;
    Ok(manifest)
}

pub fn validate_profile(manifest: &ProfileManifest) -> Result<()> {
    if manifest.id != "desktop" {
        return Err(SystemUiError::UnsupportedProfile);
    }
    if manifest.default_shell != "desktop" || !contains_str(&manifest.allowed_shells, "desktop") {
        return Err(SystemUiError::UnsupportedShell);
    }
    if manifest.display_defaults.orientation != "landscape"
        || manifest.display_defaults.dpi_class != "normal"
        || manifest.display_defaults.size_class != "wide"
    {
        return Err(SystemUiError::InvalidManifest);
    }
    Ok(())
}

pub(crate) fn parse_entries(input: &str) -> Result<Vec<TomlEntry>> {
    let mut section = String::new();
    let mut entries = Vec::new();
    for raw in input.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.len() <= 2 {
                return Err(SystemUiError::InvalidManifest);
            }
            section = line[1..line.len() - 1].trim().to_string();
            if section.is_empty() {
                return Err(SystemUiError::InvalidManifest);
            }
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(SystemUiError::InvalidManifest)?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err(SystemUiError::InvalidManifest);
        }
        entries.push(TomlEntry {
            section: section.clone(),
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(entries)
}

pub(crate) fn string_field(entries: &[TomlEntry], section: &str, key: &str) -> Result<String> {
    let value = raw_field(entries, section, key)?;
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(SystemUiError::InvalidManifest);
    }
    let inner = &value[1..value.len() - 1];
    if inner.is_empty() || inner.contains('"') {
        return Err(SystemUiError::InvalidManifest);
    }
    Ok(inner.to_string())
}

pub(crate) fn string_array_field(
    entries: &[TomlEntry],
    section: &str,
    key: &str,
) -> Result<Vec<String>> {
    let value = raw_field(entries, section, key)?;
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(SystemUiError::InvalidManifest);
    }
    let inner = value[1..value.len() - 1].trim();
    if inner.is_empty() {
        return Err(SystemUiError::InvalidManifest);
    }
    let mut out = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if !item.starts_with('"') || !item.ends_with('"') || item.len() < 2 {
            return Err(SystemUiError::InvalidManifest);
        }
        out.push(item[1..item.len() - 1].to_string());
    }
    Ok(out)
}

pub(crate) fn bool_field(entries: &[TomlEntry], section: &str, key: &str) -> Result<bool> {
    match raw_field(entries, section, key)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(SystemUiError::InvalidManifest),
    }
}

pub(crate) fn u32_field(entries: &[TomlEntry], section: &str, key: &str) -> Result<u32> {
    raw_field(entries, section, key)?.parse::<u32>().map_err(|_| SystemUiError::InvalidManifest)
}

pub(crate) fn contains_str(values: &[String], expected: &str) -> bool {
    values.iter().any(|value| value == expected)
}

fn raw_field<'a>(entries: &'a [TomlEntry], section: &str, key: &str) -> Result<&'a str> {
    entries
        .iter()
        .find(|entry| entry.section == section && entry.key == key)
        .map(|entry| entry.value.as_str())
        .ok_or(SystemUiError::MissingField)
}
