// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal TOML-backed SystemUI shell seed for TASK-0055C.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::string::String;
use alloc::vec::Vec;

use crate::profile::{
    bool_field, contains_str, desktop_profile, parse_entries, string_array_field, string_field,
    u32_field, ProfileManifest, Result, SystemUiError,
};

pub const DESKTOP_SHELL_TOML: &str = include_str!("../manifests/shells/desktop/shell.toml");

/// Shell `kind`s the runtime understands (the shell's *posture*; the shell `id`
/// is open-ended and validated by registry presence, but the kind drives generic
/// behaviour like launcher/multiwindow defaults and the windowd posture later).
pub const KNOWN_SHELL_KINDS: &[&str] = &["desktop", "tablet", "phone", "tv", "kiosk", "auto"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstFrameSpec {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellFeatures {
    pub launcher: bool,
    pub multiwindow: bool,
    pub quick_settings: bool,
    pub settings_entry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellManifest {
    pub id: String,
    pub kind: String,
    pub dsl_root: String,
    pub supported_profiles: Vec<String>,
    pub first_frame: FirstFrameSpec,
    pub features: ShellFeatures,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedShell {
    pub profile: ProfileManifest,
    pub shell: ShellManifest,
}

pub fn desktop_shell() -> Result<ShellManifest> {
    parse_shell_manifest(DESKTOP_SHELL_TOML)
}

pub fn resolve_desktop_shell() -> Result<ResolvedShell> {
    let profile = desktop_profile()?;
    let shell = desktop_shell()?;
    validate_profile_shell(&profile, &shell)?;
    Ok(ResolvedShell { profile, shell })
}

pub fn parse_shell_manifest(input: &str) -> Result<ShellManifest> {
    let entries = parse_entries(input)?;
    let manifest = ShellManifest {
        id: string_field(&entries, "", "id")?,
        kind: string_field(&entries, "", "kind")?,
        dsl_root: string_field(&entries, "", "dsl_root")?,
        supported_profiles: string_array_field(&entries, "", "supported_profiles")?,
        first_frame: FirstFrameSpec {
            width: u32_field(&entries, "first_frame", "width")?,
            height: u32_field(&entries, "first_frame", "height")?,
        },
        features: ShellFeatures {
            launcher: bool_field(&entries, "features", "launcher")?,
            multiwindow: bool_field(&entries, "features", "multiwindow")?,
            quick_settings: bool_field(&entries, "features", "quick_settings")?,
            settings_entry: bool_field(&entries, "features", "settings_entry")?,
        },
    };
    validate_shell(&manifest)?;
    Ok(manifest)
}

/// Generic shell schema validation (NOT hardcoded to one shell id): non-empty
/// identity, a `kind` from [`KNOWN_SHELL_KINDS`], a non-empty `supported_profiles`,
/// and a positive first-frame size. A custom fork shell id passes as long as its
/// kind is known and its fields are well-formed.
pub fn validate_shell(manifest: &ShellManifest) -> Result<()> {
    if manifest.id.is_empty() || manifest.dsl_root.is_empty() {
        return Err(SystemUiError::InvalidManifest);
    }
    if !KNOWN_SHELL_KINDS.iter().any(|k| *k == manifest.kind) {
        return Err(SystemUiError::UnsupportedShell);
    }
    if manifest.supported_profiles.is_empty() {
        return Err(SystemUiError::UnsupportedProfile);
    }
    if manifest.first_frame.width == 0 || manifest.first_frame.height == 0 {
        return Err(SystemUiError::InvalidFrameDimensions);
    }
    Ok(())
}

/// A profile and shell are compatible iff the profile *allows* that shell
/// (`allowed_shells` ∋ shell.id) AND the shell *supports* that profile
/// (`supported_profiles` ∋ profile.id). NOTE: it need NOT be the profile's
/// `default_shell` — that lets a profile resolve to any of its allowed shells,
/// which is exactly what a runtime shell switch (e.g. convertible desktop↔tablet)
/// relies on.
pub fn validate_profile_shell(profile: &ProfileManifest, shell: &ShellManifest) -> Result<()> {
    if !contains_str(&profile.allowed_shells, &shell.id) {
        return Err(SystemUiError::UnsupportedShell);
    }
    if !contains_str(&shell.supported_profiles, &profile.id) {
        return Err(SystemUiError::IncompatibleShell);
    }
    Ok(())
}
