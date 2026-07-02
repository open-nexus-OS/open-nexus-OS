// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI greeter (login window) appearance config — manifest-driven
//! (TASK-0065B), consumed by windowd's greeter renderer. SystemUI owns what the
//! login window looks like; sessiond owns WHO can log in; windowd renders.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cargo test -p systemui`

use crate::profile::{parse_entries, string_field, u32_field, Result, SystemUiError};

/// The shipped greeter manifest.
pub const DEFAULT_GREETER_TOML: &str =
    include_str!("../manifests/greeter/default/greeter.toml");

/// Greeter appearance (display pixels at the canonical mode). Obtained via
/// [`greeter_config`]; forks tune the manifest, not windowd code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreeterConfig {
    /// Manifest id.
    pub id: alloc::string::String,
    /// Wallpaper box-blur radius (separable one-time bake).
    pub blur_radius: u32,
    /// Dark tint multiplier over the blurred wallpaper, 0..=255 (255 = none).
    pub dim: u8,
    /// Avatar circle diameter.
    pub avatar_diameter: u32,
    /// Ring stroke width around the circle.
    pub ring_stroke: u32,
    /// Gap between the circle and the user-name label.
    pub label_gap: u32,
}

impl GreeterConfig {
    /// Hardcoded fallback so windowd always has a valid greeter to render
    /// (same discipline as `ShellConfig::desktop_fallback`).
    pub fn fallback() -> Self {
        Self {
            id: alloc::string::String::from("fallback"),
            blur_radius: 18,
            dim: 176,
            avatar_diameter: 96,
            ring_stroke: 3,
            label_gap: 18,
        }
    }
}

/// Parses a greeter manifest.
pub fn parse_greeter_manifest(input: &str) -> Result<GreeterConfig> {
    let entries = parse_entries(input)?;
    let cfg = GreeterConfig {
        id: string_field(&entries, "", "id")?,
        blur_radius: u32_field(&entries, "backdrop", "blur_radius")?,
        dim: u8_field(&entries, "backdrop", "dim")?,
        avatar_diameter: u32_field(&entries, "avatar", "diameter")?,
        ring_stroke: u32_field(&entries, "avatar", "ring_stroke")?,
        label_gap: u32_field(&entries, "avatar", "label_gap")?,
    };
    validate_greeter(&cfg)?;
    Ok(cfg)
}

/// Schema validation: sane, bounded values (the blur ring buffer and the
/// avatar bake in windowd rely on these bounds).
pub fn validate_greeter(cfg: &GreeterConfig) -> Result<()> {
    if cfg.id.is_empty() {
        return Err(SystemUiError::InvalidManifest);
    }
    if cfg.blur_radius == 0 || cfg.blur_radius > 64 {
        return Err(SystemUiError::InvalidManifest);
    }
    if cfg.avatar_diameter < 32 || cfg.avatar_diameter > 256 {
        return Err(SystemUiError::InvalidManifest);
    }
    if cfg.ring_stroke > 16 || cfg.label_gap > 96 {
        return Err(SystemUiError::InvalidManifest);
    }
    Ok(())
}

/// The shipped greeter config; infallible (fallback on any manifest error) so
/// windowd's greeter path never depends on manifest health at runtime — the
/// shipped manifest itself is host-tested to parse.
pub fn greeter_config() -> GreeterConfig {
    parse_greeter_manifest(DEFAULT_GREETER_TOML).unwrap_or_else(|_| GreeterConfig::fallback())
}

fn u8_field(entries: &[crate::profile::TomlEntry], section: &str, key: &str) -> Result<u8> {
    u8::try_from(u32_field(entries, section, key)?).map_err(|_| SystemUiError::InvalidManifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_greeter_manifest_parses() {
        let cfg = parse_greeter_manifest(DEFAULT_GREETER_TOML).expect("shipped manifest parses");
        assert_eq!(cfg.id, "default");
        assert_eq!(cfg.blur_radius, 18);
        assert_eq!(cfg.dim, 176);
        assert_eq!(cfg.avatar_diameter, 96);
        assert_eq!(cfg.ring_stroke, 3);
        assert_eq!(cfg.label_gap, 18);
        assert_eq!(greeter_config(), cfg);
    }

    #[test]
    fn fallback_config_sane() {
        validate_greeter(&GreeterConfig::fallback()).expect("fallback validates");
    }

    #[test]
    fn out_of_bounds_rejected() {
        let manifest = DEFAULT_GREETER_TOML.replace("blur_radius = 18", "blur_radius = 999");
        assert!(parse_greeter_manifest(&manifest).is_err());
        let manifest = DEFAULT_GREETER_TOML.replace("diameter = 96", "diameter = 4");
        assert!(parse_greeter_manifest(&manifest).is_err());
    }
}
