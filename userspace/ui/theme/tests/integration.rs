// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use nexus_theme::{ColorValue, Qualifier, ThemeError, ThemeRuntime};

fn themes_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("resources")
        .join("themes")
}

// ---------------------------------------------------------------------------
// ColorValue parsing
// ---------------------------------------------------------------------------

#[test]
fn test_color_value_from_hex_rrggbb() {
    let c = ColorValue::from_hex("#3b82f6").unwrap();
    assert_eq!(c.r, 0x3b);
    assert_eq!(c.g, 0x82);
    assert_eq!(c.b, 0xf6);
    assert_eq!(c.a, 255);
}

#[test]
fn test_color_value_from_hex_rrggbbaa() {
    let c = ColorValue::from_hex("#3b82f6cc").unwrap();
    assert_eq!(c.r, 0x3b);
    assert_eq!(c.g, 0x82);
    assert_eq!(c.b, 0xf6);
    assert_eq!(c.a, 0xcc);
}

#[test]
fn test_color_value_from_hex_rgb() {
    let c = ColorValue::from_hex("#f80").unwrap();
    assert_eq!(c.r, 0xff);
    assert_eq!(c.g, 0x88);
    assert_eq!(c.b, 0x00);
    assert_eq!(c.a, 255);
}

#[test]
fn test_color_value_invalid_no_prefix() {
    let err = ColorValue::from_hex("3b82f6").unwrap_err();
    assert!(matches!(err, ThemeError::InvalidColor { .. }));
}

#[test]
fn test_color_value_invalid_length() {
    let err = ColorValue::from_hex("#3b82f").unwrap_err();
    assert!(matches!(err, ThemeError::InvalidColor { .. }));
}

#[test]
fn test_color_value_invalid_hex_digit() {
    let err = ColorValue::from_hex("#3b8zf6").unwrap_err();
    assert!(matches!(err, ThemeError::InvalidColor { .. }));
}

#[test]
fn test_color_value_display_rrggbb() {
    let c = ColorValue { r: 0x3b, g: 0x82, b: 0xf6, a: 255 };
    assert_eq!(c.to_string(), "#3b82f6");
}

#[test]
fn test_color_value_display_rrggbbaa() {
    let c = ColorValue { r: 0x3b, g: 0x82, b: 0xf6, a: 0xcc };
    assert_eq!(c.to_string(), "#3b82f6cc");
}

// ---------------------------------------------------------------------------
// Theme loading
// ---------------------------------------------------------------------------

#[test]
fn test_load_base_theme() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    assert_eq!(runtime.active_qualifier(), Qualifier::Base);
    assert!(runtime.get_theme(Qualifier::Base).is_some());
}

#[test]
fn test_load_all_themes() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    assert!(runtime.get_theme(Qualifier::Base).is_some());
    assert!(runtime.get_theme(Qualifier::Dark).is_some());
    assert!(runtime.get_theme(Qualifier::Light).is_some());
    assert!(runtime.get_theme(Qualifier::HighContrast).is_some());
}

#[test]
fn test_missing_base_theme() {
    let tmp = std::env::temp_dir().join("nexus-theme-test-missing-base");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Create a dark theme but no base theme
    std::fs::write(
        tmp.join("dark.nxtheme.toml"),
        "[theme]\nname = \"dark\"\nversion = 1\n[tokens]\naccent = \"#ffffff\"\n",
    )
    .unwrap();

    let err = ThemeRuntime::load(&tmp).unwrap_err();
    assert!(matches!(err, ThemeError::MissingBaseTheme { .. }));

    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Token resolution
// ---------------------------------------------------------------------------

#[test]
fn test_resolve_token_from_base() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    let accent = runtime.resolve("accent").unwrap();
    assert_eq!(accent, ColorValue::from_hex("#3b82f6").unwrap());
}

#[test]
fn test_resolve_token_dark_override() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    runtime.set_qualifier(Qualifier::Dark);
    let bg = runtime.resolve("bg").unwrap();
    // Dark bg should be near-black, not white
    assert_eq!(bg, ColorValue::from_hex("#0f172a").unwrap());
}

#[test]
fn test_resolve_token_dark_falls_back_to_base() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    runtime.set_qualifier(Qualifier::Dark);
    // 'danger' is not defined in dark theme, should fall back to base
    let danger = runtime.resolve("danger").unwrap();
    assert_eq!(danger, ColorValue::from_hex("#ef4444").unwrap());
}

#[test]
fn test_resolve_token_not_found() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    let err = runtime.resolve("nonexistent_token_xyz").unwrap_err();
    assert!(matches!(err, ThemeError::TokenNotFound { .. }));
}

#[test]
fn test_resolve_highcontrast_override() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    runtime.set_qualifier(Qualifier::HighContrast);
    let bg = runtime.resolve("bg").unwrap();
    assert_eq!(bg, ColorValue::from_hex("#000000").unwrap());
    let fg = runtime.resolve("fg").unwrap();
    assert_eq!(fg, ColorValue::from_hex("#ffffff").unwrap());
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn test_deterministic_resolution() {
    let r1 = ThemeRuntime::load(&themes_dir()).unwrap();
    let r2 = ThemeRuntime::load(&themes_dir()).unwrap();

    let tokens = ["accent", "bg", "fg", "surface", "border", "muted", "danger"];
    for token in &tokens {
        assert_eq!(r1.resolve(token).unwrap(), r2.resolve(token).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Schema validation (reject tests)
// ---------------------------------------------------------------------------

#[test]
fn test_reject_unknown_section() {
    let err = nexus_theme::parse_theme_file(
        "[theme]\nname = \"x\"\nversion = 1\n[foobar]\nkey = \"val\"\n",
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::UnknownSection { .. }));
}

#[test]
fn test_reject_unknown_theme_key() {
    let err = nexus_theme::parse_theme_file(
        "[theme]\nname = \"x\"\nversion = 1\nauthor = \"me\"\n",
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::UnknownKey { .. }));
}

#[test]
fn test_reject_invalid_version() {
    let err = nexus_theme::parse_theme_file(
        "[theme]\nname = \"x\"\nversion = 0\n",
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}

#[test]
fn test_reject_invalid_material_type() {
    let err = nexus_theme::parse_theme_file(
        concat!("[theme]\nname = \"x\"\nversion = 1\n", "[material.test]\ntype = \"metallic\"\n",),
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}

#[test]
fn test_reject_invalid_theme_toml() {
    // Missing required [theme] section
    let err = nexus_theme::parse_theme_file(
        "[tokens]\naccent = \"#ff0000\"\n",
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::MissingSection { .. }));
}

#[test]
fn test_reject_non_string_token() {
    let err = nexus_theme::parse_theme_file(
        "[theme]\nname = \"x\"\nversion = 1\n[tokens]\naccent = 123\n",
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}

#[test]
fn test_reject_missing_glass_fields() {
    let err = nexus_theme::parse_theme_file(
        concat!("[theme]\nname = \"x\"\nversion = 1\n", "[material.test]\ntype = \"glass\"\n",),
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}

// ---------------------------------------------------------------------------
// Material parsing
// ---------------------------------------------------------------------------

#[test]
fn test_material_opaque() {
    let theme = nexus_theme::parse_theme_file(
        concat!("[theme]\nname = \"x\"\nversion = 1\n", "[material.surface]\ntype = \"opaque\"\n",),
        std::path::Path::new("test.toml"),
    )
    .unwrap();
    let mat = theme.materials.get("surface").unwrap();
    assert!(matches!(mat, nexus_theme::Material::Opaque));
}

#[test]
fn test_material_glass() {
    let theme = nexus_theme::parse_theme_file(
        concat!(
            "[theme]\nname = \"x\"\nversion = 1\n",
            "[material.glassLow]\n",
            "type = \"glass\"\n",
            "blurRadiusDp = 8\n",
            "downsampleFactor = 4\n",
            "tintColor = \"#ffffff\"\n",
            "tintAlpha = 0.3\n",
            "edgeHighlightColor = \"#ffffff\"\n",
            "edgeHighlightAlpha = 0.15\n",
        ),
        std::path::Path::new("test.toml"),
    )
    .unwrap();
    let mat = theme.materials.get("glassLow").unwrap();
    assert!(matches!(mat, nexus_theme::Material::Glass(_)));
}
