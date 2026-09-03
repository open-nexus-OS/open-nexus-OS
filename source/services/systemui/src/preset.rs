// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Dev-mode **display/profile preset** manifests (TASK-0055D). A
//! preset is a named, deterministic bundle — registered profile + shell,
//! display mode (w × h @ hz), orientation, dpi class, emulated input set —
//! that a developer selects for a QEMU bring-up (`just start-preset <name>`).
//! Presets resolve INTO the existing authorities and never fork one: the
//! profile/shell ids must be registered manifests (`registry::PROFILES` /
//! `SHELLS`) with a valid pairing, and the display mode feeds the fw_cfg
//! `display-mode` key the compositor already follows (RFC-0074 / ADR-0050).
//! Parsing reuses the profile manifest's bounded TOML subset; validation is
//! generic (any registered id passes on its own merits), so forks add a
//! preset by adding a TOML + one registry line.
//!
//! Honest limits (v1): the display mode is bounded by the compositor's fixed
//! layout maximum (the shared atlas/VMO layout is sized to 1280×800 — a
//! larger mode would be clamped by gpud, so it is rejected here instead of
//! silently shrinking), and only refresh rates the windowd pacer can deliver
//! are accepted (`SUPPORTED_DISPLAY_HZ`). Guest-side preset ingestion
//! (fw_cfg key → settingsd default overlay → `systemui: profile …` marker)
//! is a follow-up; this module is the host-side authority + validator.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p systemui preset` (catalog resolve, reject
//!                suite, convertible switch); `tools/nx/tests/ui_preset_cli.rs`
//! SPEC: docs/dev/ui/foundations/layout/profiles.md (§ Dev presets)

use alloc::string::String;

use crate::profile::{
    bool_field, parse_entries, string_field, u32_field, DeviceInput, Result, SystemUiError,
    KNOWN_DPI_CLASSES, KNOWN_ORIENTATIONS,
};

/// The compositor's fixed layout maximum (width, height). windowd's shared
/// atlas/VMO layout and gpud's scanout budget are sized to this; the fw_cfg
/// display mode may be SMALLER (a visible sub-rect) but never larger.
pub const PRESET_LAYOUT_MAX: (u32, u32) = (1280, 800);

/// Smallest edge a preset may declare. Below this the shell chrome (dock,
/// status bar, launcher grid) has no honest layout.
pub const PRESET_MIN_EDGE: u32 = 320;

/// Refresh rates the compositor can actually pace. windowd's pacer is a
/// single 120 Hz constant (`PACER_INTERVAL_NS`); accepting another rate here
/// would promise pacing the guest cannot deliver, so the set stays `[120]`
/// until the pacer becomes mode-driven.
pub const SUPPORTED_DISPLAY_HZ: &[u32] = &[120];

/// One parsed + validated preset manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetManifest {
    pub id: String,
    pub label: String,
    /// Registered profile id (`registry::PROFILES`).
    pub profile: String,
    /// Registered shell id (`registry::SHELLS`); must be allowed by the profile.
    pub shell: String,
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    pub orientation: String,
    pub dpi_class: String,
    /// Emulated QEMU input sources for this preset.
    pub input: DeviceInput,
}

/// Parse a preset manifest from its TOML source and validate its shape.
/// Registry-level checks (profile/shell registered + paired) live in
/// [`crate::registry::resolve_preset`].
pub fn parse_preset_manifest(input: &str) -> Result<PresetManifest> {
    let entries = parse_entries(input)?;
    let manifest = PresetManifest {
        id: string_field(&entries, "", "id")?,
        label: string_field(&entries, "", "label")?,
        profile: string_field(&entries, "", "profile")?,
        shell: string_field(&entries, "", "shell")?,
        width: u32_field(&entries, "display", "width")?,
        height: u32_field(&entries, "display", "height")?,
        hz: u32_field(&entries, "display", "hz")?,
        orientation: string_field(&entries, "display", "orientation")?,
        dpi_class: string_field(&entries, "display", "dpi_class")?,
        input: DeviceInput {
            touch: bool_field(&entries, "input", "touch")?,
            mouse: bool_field(&entries, "input", "mouse")?,
            kbd: bool_field(&entries, "input", "kbd")?,
            remote: bool_field(&entries, "input", "remote")?,
            rotary: bool_field(&entries, "input", "rotary")?,
        },
    };
    validate_preset(&manifest)?;
    Ok(manifest)
}

/// Shape validation: non-empty identity, known orientation/dpi vocabularies,
/// a display mode inside `[PRESET_MIN_EDGE, PRESET_LAYOUT_MAX]` whose aspect
/// matches the declared orientation, and a pace-able refresh rate.
pub fn validate_preset(manifest: &PresetManifest) -> Result<()> {
    if manifest.id.is_empty()
        || manifest.label.is_empty()
        || manifest.profile.is_empty()
        || manifest.shell.is_empty()
    {
        return Err(SystemUiError::InvalidManifest);
    }
    if !KNOWN_ORIENTATIONS.contains(&manifest.orientation.as_str())
        || !KNOWN_DPI_CLASSES.contains(&manifest.dpi_class.as_str())
    {
        return Err(SystemUiError::InvalidManifest);
    }
    validate_display_mode(manifest.width, manifest.height, &manifest.orientation)?;
    if !SUPPORTED_DISPLAY_HZ.contains(&manifest.hz) {
        return Err(SystemUiError::UnsupportedRefreshRate);
    }
    Ok(())
}

/// The display-mode bound shared by every preset: both edges within
/// `[PRESET_MIN_EDGE, PRESET_LAYOUT_MAX]`, and `portrait` ⇒ `h ≥ w`,
/// `landscape` ⇒ `w ≥ h` (a "portrait" preset wider than tall would lie to
/// `device.orientation`).
pub fn validate_display_mode(width: u32, height: u32, orientation: &str) -> Result<()> {
    let (max_w, max_h) = PRESET_LAYOUT_MAX;
    if width < PRESET_MIN_EDGE || height < PRESET_MIN_EDGE || width > max_w || height > max_h {
        return Err(SystemUiError::InvalidDisplayMode);
    }
    let aspect_ok = match orientation {
        "portrait" => height >= width,
        "landscape" => width >= height,
        _ => false,
    };
    if !aspect_ok {
        return Err(SystemUiError::InvalidDisplayMode);
    }
    Ok(())
}

/// `device.sizeClass` for a visible width — the SAME mobile-first tiers the
/// app runtime derives from the real surface width (compact < 640 ≤ regular
/// < 1024 ≤ wide), so a preset's environment predicts what the DSL sees.
pub fn size_class_for_width(width: u32) -> &'static str {
    if width < 640 {
        "compact"
    } else if width < 1024 {
        "regular"
    } else {
        "wide"
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_preset_manifest, size_class_for_width, validate_display_mode};
    use crate::profile::SystemUiError;

    fn manifest(display: &str) -> alloc::string::String {
        alloc::format!(
            r#"
id = "probe"
label = "Probe"
profile = "tablet"
shell = "tablet"

[display]
{display}

[input]
touch = true
mouse = false
kbd = false
remote = false
rotary = false
"#
        )
    }

    #[test]
    fn well_formed_preset_parses() {
        let m = parse_preset_manifest(&manifest(
            "width = 600\nheight = 800\nhz = 120\norientation = \"portrait\"\ndpi_class = \"high\"",
        ))
        .expect("parse");
        assert_eq!((m.width, m.height, m.hz), (600, 800, 120));
        assert_eq!(m.orientation, "portrait");
        assert!(m.input.touch && !m.input.mouse);
    }

    #[test]
    fn test_reject_mode_exceeds_layout() {
        let err = parse_preset_manifest(&manifest(
            "width = 1920\nheight = 1080\nhz = 120\norientation = \"landscape\"\ndpi_class = \"normal\"",
        ))
        .expect_err("must reject");
        assert_eq!(err, SystemUiError::InvalidDisplayMode);
    }

    #[test]
    fn test_reject_mode_below_min_edge() {
        assert_eq!(
            validate_display_mode(200, 800, "portrait"),
            Err(SystemUiError::InvalidDisplayMode)
        );
    }

    #[test]
    fn test_reject_orientation_aspect_mismatch() {
        // A "portrait" preset that is wider than tall lies to device.orientation.
        assert_eq!(
            validate_display_mode(800, 480, "portrait"),
            Err(SystemUiError::InvalidDisplayMode)
        );
        assert_eq!(
            validate_display_mode(480, 800, "landscape"),
            Err(SystemUiError::InvalidDisplayMode)
        );
        assert_eq!(validate_display_mode(800, 800, "portrait"), Ok(()));
    }

    #[test]
    fn test_reject_unsupported_hz() {
        let err = parse_preset_manifest(&manifest(
            "width = 1280\nheight = 800\nhz = 60\norientation = \"landscape\"\ndpi_class = \"normal\"",
        ))
        .expect_err("60 Hz is not pace-able yet");
        assert_eq!(err, SystemUiError::UnsupportedRefreshRate);
    }

    #[test]
    fn test_reject_unknown_orientation_and_missing_field() {
        let err = parse_preset_manifest(&manifest(
            "width = 1280\nheight = 800\nhz = 120\norientation = \"sideways\"\ndpi_class = \"normal\"",
        ))
        .expect_err("unknown orientation");
        assert_eq!(err, SystemUiError::InvalidManifest);
        let err = parse_preset_manifest(&manifest(
            "width = 1280\nheight = 800\norientation = \"landscape\"\ndpi_class = \"normal\"",
        ))
        .expect_err("hz missing");
        assert_eq!(err, SystemUiError::MissingField);
    }

    #[test]
    fn size_class_tiers_match_runtime() {
        assert_eq!(size_class_for_width(480), "compact");
        assert_eq!(size_class_for_width(600), "compact");
        assert_eq!(size_class_for_width(800), "regular");
        assert_eq!(size_class_for_width(1280), "wide");
    }
}
