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
    // Dark bg = handoff .dark background oklch(0.145) = #0a0a0a.
    assert_eq!(bg, ColorValue::from_hex("#0a0a0a").unwrap());
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

// ---------------------------------------------------------------------------
// Reconciled design-system contract (RFC-0070 / token-reconciliation.md)
// Pins the handoff token + 5-glass-level contract on the real resource themes.
// ---------------------------------------------------------------------------

#[test]
fn test_reconciled_new_tokens_resolve_from_base() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    for (name, hex) in [
        ("primary", "#030213"),
        ("info", "#3b82f6"),
        ("destructive", "#d4183d"),
        ("secondary", "#eceef2"),
        ("sidebar", "#fafafa"),
        ("chart1", "#f54900"),
        // RFC-0082: on-glass ink keeps the handoff's blue-black HUE (not pure
        // black) but its ALPHA is the handoff's own `--glass-text-primary`
        // (.80 = cc). The .95 this used to pin was undocumented drift.
        ("glassTextPrimary", "#14141acc"),
        ("glassTextSecondary", "#14141a66"),
        ("glassTextStrong", "#14141aeb"),
        ("glassIcon", "#14141acc"),
        // Handoff `--glass-divider` — translucent, so a hairline reads the same
        // on a pane, a card and the wallpaper.
        ("divider", "#0000001a"),
        ("glassHover", "#00000014"),
        ("glassActive", "#0000001f"),
        ("wallpaperTint", "#ffffff3d"),
        ("textShadowStrong", "#fffffff2"),
        ("transparent", "#00000000"),
        ("toggleOnBg", "#3b82f6d9"),
    ] {
        assert_eq!(
            runtime.resolve(name).unwrap(),
            ColorValue::from_hex(hex).unwrap(),
            "base token '{name}' should resolve to {hex}"
        );
    }
}

#[test]
fn test_reconciled_dark_token_overrides() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    runtime.set_qualifier(Qualifier::Dark);
    for (name, hex) in [
        ("primary", "#fafafa"),
        ("secondary", "#262626"),
        ("destructive", "#fa5a55"),
        ("chart1", "#1447e6"),
        // Handoff dark: --glass-text-primary .90 / -secondary .45 / -strong .95.
        ("glassTextPrimary", "#ffffffe6"),
        ("glassTextSecondary", "#ffffff73"),
        ("glassTextStrong", "#fffffff2"),
        ("divider", "#ffffff1a"),
        ("wallpaperTint", "#0000008c"),
        ("textShadow", "#00000066"),
    ] {
        assert_eq!(
            runtime.resolve(name).unwrap(),
            ColorValue::from_hex(hex).unwrap(),
            "dark token '{name}' should override to {hex}"
        );
    }
    // A token only defined in base still falls back under dark.
    assert_eq!(runtime.resolve("info").unwrap(), ColorValue::from_hex("#3b82f6").unwrap());
}

/// Every glass level `MaterialToken` names must exist in EVERY theme, not just
/// base: `theme-tokens/build.rs` reads them per qualifier, so a level missing
/// from `dark` resolves to the base (light) recipe and paints a white wash on a
/// dark desktop rather than failing.
#[test]
fn test_every_glass_level_is_defined_in_every_theme() {
    const LEVELS: &[&str] = &[
        "glassPanel",
        "glassCard",
        "glassSubtle",
        "glassWindow",
        // The two window levels the file-manager handoff needs: a PANE
        // (content / properties) is far more transmissive than a panel, and
        // the action-bar PILL is far denser.
        "glassWindowPane",
        "glassWindowBar",
        "glassOverlay",
    ];
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    for qualifier in [Qualifier::Base, Qualifier::Dark, Qualifier::HighContrast] {
        let theme = runtime.get_theme(qualifier).unwrap();
        for level in LEVELS {
            assert!(
                matches!(theme.materials.get(*level), Some(nexus_theme::Material::Glass(_))),
                "{qualifier:?} must define glass material '{level}'"
            );
        }
    }
    let base = runtime.get_theme(Qualifier::Base).unwrap();
    // The old ad-hoc glassLow/glassHigh names are gone (replaced by the levels).
    assert!(base.materials.get("glassLow").is_none());
    assert!(base.materials.get("glassHigh").is_none());
}

/// The pane is NOT a re-skinned panel. Two things separate them, and both are
/// what a "simplification" that collapses the levels would destroy:
///
/// - **Luminance.** The panel's dark tint is `#121214` — near-black, correct
///   for a dock tile floating on a photograph. A content pane painted with it
///   reads as a black slab inside a grey window; the handoff's pane is
///   `#484a54`, a mid grey.
/// - **Shape.** The pane is a two-stop gradient (handoff `160deg`, .48 → .32);
///   the panel is one flat tint.
///
/// Note what is deliberately NOT asserted: the pane is not uniformly more
/// transmissive. Its top stop (.48) is denser than the panel's flat .40 and its
/// bottom stop (.32) is lighter — the gradient is the point, not the average.
#[test]
fn test_window_pane_is_a_distinct_level_from_panel() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    runtime.set_qualifier(Qualifier::Dark);
    let glass = |name: &str| match runtime.resolve_material(name) {
        Some(nexus_theme::Material::Glass(g)) => g,
        other => panic!("dark {name} must be glass, got {other:?}"),
    };
    let pane = glass("glassWindowPane");
    let panel = glass("glassPanel");
    // The two levels are distinct by COVERAGE, not by luma. Since
    // design_handoff_panels the dark panel is a white 10 % wash — depth comes
    // from blur(72) + saturate, not from covering the wallpaper — while the
    // pane is a dense mid-grey window surface that content sits ON.
    assert!(
        pane.tint_alpha > panel.tint_alpha * 2.0,
        "the pane ({:?}@{}) must be far denser than the floating panel ({:?}@{})",
        pane.tint_color,
        pane.tint_alpha,
        panel.tint_color,
        panel.tint_alpha
    );
    assert_eq!(pane.tint_color, ColorValue::from_hex("#484a54").unwrap());
    let bottom = pane.tint_bottom_color.expect("the pane is a TWO-STOP gradient (handoff 160deg)");
    assert_eq!(bottom, ColorValue::from_hex("#34363e").unwrap());
    assert!(pane.tint_bottom_alpha.unwrap() < pane.tint_alpha, "the gradient fades downward");
    // The bar is the other extreme: a control strip that must stay readable
    // over whatever scrolls under it, so it is denser than either.
    assert!(glass("glassWindowBar").tint_alpha > panel.tint_alpha);
}

#[test]
fn test_glass_material_resolves_through_qualifier_chain() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();

    // Light does not redefine glassPanel → inherits base (blur 72, tint .50).
    // 72 is design_handoff_panels' `blur(72px)`: the drop-down panels are the
    // deepest glass in the system, and dock/taskbar share the level on purpose.
    runtime.set_qualifier(Qualifier::Light);
    match runtime.resolve_material("glassPanel") {
        Some(nexus_theme::Material::Glass(g)) => {
            assert_eq!(g.blur_radius_dp, 72);
            assert!((g.tint_alpha - 0.50).abs() < 1e-6);
        }
        other => panic!("light glassPanel should inherit base glass, got {other:?}"),
    }

    // Dark overrides glassPanel with a LIGHTER wash than light mode uses
    // (design_handoff_panels: rgba(255,255,255,.10) dark vs .50 light). The
    // RFC-0082 dark tint moved to the card/window levels, which sit on a
    // surface rather than floating over the wallpaper.
    runtime.set_qualifier(Qualifier::Dark);
    match runtime.resolve_material("glassPanel") {
        Some(nexus_theme::Material::Glass(g)) => {
            assert!((g.tint_alpha - 0.10).abs() < 1e-6, "dark panel wash is 10%");
            assert_eq!(g.tint_color, ColorValue::from_hex("#ffffff").unwrap());
        }
        other => panic!("dark glassPanel should override, got {other:?}"),
    }

    // High contrast zeroes blur on every level (a11y).
    runtime.set_qualifier(Qualifier::HighContrast);
    match runtime.resolve_material("glassOverlay") {
        Some(nexus_theme::Material::Glass(g)) => assert_eq!(g.blur_radius_dp, 0),
        other => panic!("highcontrast glassOverlay should be blur 0, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Length scales ([spacing] / [radius]) — invariant, authored in base
// ---------------------------------------------------------------------------

#[test]
fn test_resolve_length_scales_from_base() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    assert_eq!(runtime.resolve_radius("small"), Some(6));
    assert_eq!(runtime.resolve_radius("medium"), Some(10));
    assert_eq!(runtime.resolve_radius("large"), Some(16));
    assert_eq!(runtime.resolve_spacing("small"), Some(8));
    assert_eq!(runtime.resolve_spacing("large"), Some(24));
    assert_eq!(runtime.resolve_radius("nonexistent"), None);
}

#[test]
fn test_length_scale_inherits_through_chain() {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    // dark/light/highcontrast don't redefine [radius]/[spacing] → inherit base.
    for q in [Qualifier::Dark, Qualifier::Light, Qualifier::HighContrast] {
        runtime.set_qualifier(q);
        assert_eq!(runtime.resolve_radius("medium"), Some(10), "{q:?} radius.medium");
        assert_eq!(runtime.resolve_spacing("small"), Some(8), "{q:?} spacing.small");
    }
}

#[test]
fn test_resolve_typography_leading_zindex() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    // Font size scale (px).
    assert_eq!(runtime.resolve_scale("typography", "base"), Some(14));
    assert_eq!(runtime.resolve_scale("typography", "display"), Some(36));
    // Line-height ×100.
    assert_eq!(runtime.resolve_scale("leading", "normal"), Some(150));
    // Stacking order.
    assert_eq!(runtime.resolve_scale("zindex", "modal"), Some(30));
    // Motion durations (ms, handoff motion.css).
    assert_eq!(runtime.resolve_scale("motion", "swift"), Some(160));
    assert_eq!(runtime.resolve_scale("motion", "slow"), Some(500));
    // Unknown section / key → None.
    assert_eq!(runtime.resolve_scale("typography", "nope"), None);
    assert_eq!(runtime.resolve_scale("nosuch", "base"), None);
}

#[test]
fn test_reject_non_integer_scale_value() {
    let err = nexus_theme::parse_theme_file(
        concat!("[theme]\nname = \"x\"\nversion = 1\n", "[spacing]\nsmall = \"8\"\n"),
        std::path::Path::new("test.toml"),
    )
    .unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}
