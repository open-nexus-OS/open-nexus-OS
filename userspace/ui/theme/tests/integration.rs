// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use nexus_theme::{Backdrop, ColorValue, Qualifier, ThemeError, ThemeRuntime};

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
// Token resolution (RFC-0116: the design artifacts' roles)
// ---------------------------------------------------------------------------

fn hex(s: &str) -> ColorValue {
    ColorValue::from_hex(s).unwrap()
}

fn at(q: Qualifier) -> ThemeRuntime {
    let mut runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    runtime.set_qualifier(q);
    runtime
}

/// Every role RFC-0116 names.
const ROLES: [&str; 35] = [
    "background",
    "label",
    "labelSecondary",
    "labelTertiary",
    "separator",
    "fill",
    "fillHover",
    "fillActive",
    "glassPanel",
    "glassWindow",
    "glassCard",
    "glassEdge",
    "surfacePane",
    "surfaceOverlay",
    "scrim",
    "primary",
    "onPrimary",
    "accent",
    "accentGlow",
    "onAccent",
    "danger",
    "success",
    "warning",
    "modeAirplane",
    "modeDesktop",
    "modeTablet",
    "island",
    "chart1",
    "chart2",
    "chart3",
    "chart4",
    "chart5",
    "wallpaperTint",
    "labelShadow",
    "transparent",
];

/// The roles whose light and dark values differ: dark and high contrast author each one.
const THEME_VARYING: [&str; 23] = [
    "background",
    "label",
    "labelSecondary",
    "labelTertiary",
    "separator",
    "fill",
    "fillHover",
    "fillActive",
    "glassPanel",
    "glassWindow",
    "glassCard",
    "glassEdge",
    "surfacePane",
    "surfaceOverlay",
    "scrim",
    "primary",
    "onPrimary",
    "accent",
    "danger",
    "success",
    "warning",
    "wallpaperTint",
    "labelShadow",
];

#[test]
fn every_role_resolves_in_every_theme() {
    for q in [Qualifier::Base, Qualifier::Light, Qualifier::Dark, Qualifier::HighContrast] {
        let runtime = at(q);
        for role in ROLES {
            assert!(runtime.resolve(role).is_ok(), "{role} missing under {q:?}");
        }
    }
}

#[test]
fn dark_and_high_contrast_author_every_theme_varying_role() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    for q in [Qualifier::Dark, Qualifier::HighContrast] {
        for role in THEME_VARYING {
            assert!(runtime.defines_token(q, role), "{role} inherited the light value under {q:?}");
        }
    }
}

#[test]
fn the_light_values_are_the_artifacts() {
    let runtime = at(Qualifier::Light);
    assert_eq!(runtime.resolve("label").unwrap(), hex("#000000e0"));
    assert_eq!(runtime.resolve("labelSecondary").unwrap(), hex("#0000008f"));
    assert_eq!(runtime.resolve("fill").unwrap(), hex("#78788029"));
    assert_eq!(runtime.resolve("glassWindow").unwrap(), hex("#f6f7fae6"));
    assert_eq!(runtime.resolve("accent").unwrap(), hex("#2563eb"));
    assert_eq!(runtime.resolve("danger").unwrap(), hex("#d4183d"));
    assert_eq!(runtime.resolve("surfacePane").unwrap(), hex("#fcfdff"));
}

#[test]
fn the_dark_values_are_the_artifacts() {
    let runtime = at(Qualifier::Dark);
    assert_eq!(runtime.resolve("background").unwrap(), hex("#0a0a0a"));
    assert_eq!(runtime.resolve("label").unwrap(), hex("#ffffffeb"));
    assert_eq!(runtime.resolve("glassCard").unwrap(), hex("#ffffff14"));
    assert_eq!(runtime.resolve("accent").unwrap(), hex("#3b82f6"));
    assert_eq!(runtime.resolve("onAccent").unwrap(), hex("#ffffff"));
    assert_eq!(runtime.resolve("danger").unwrap(), hex("#fa5a55"));
    // Not theme-varying: inherited from base.
    assert_eq!(runtime.resolve("modeAirplane").unwrap(), hex("#f97316d9"));
}

#[test]
fn test_resolve_token_not_found() {
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    let err = runtime.resolve("nonexistent_token_xyz").unwrap_err();
    assert!(matches!(err, ThemeError::TokenNotFound { .. }));
}

#[test]
fn test_deterministic_resolution() {
    let r1 = ThemeRuntime::load(&themes_dir()).unwrap();
    let r2 = ThemeRuntime::load(&themes_dir()).unwrap();
    for role in ROLES {
        assert_eq!(r1.resolve(role).unwrap(), r2.resolve(role).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Text styles, shadows, backdrops, scales
// ---------------------------------------------------------------------------

#[test]
fn the_twelve_text_styles_carry_size_line_weight_tracking_and_case() {
    let runtime = at(Qualifier::Dark);
    let table = [
        ("display", 64, 64, 300, -20, false),
        ("largeTitle", 30, 36, 700, -20, false),
        ("title1", 24, 30, 700, -15, false),
        ("title2", 20, 26, 600, -10, false),
        ("title3", 17, 22, 600, -5, false),
        ("headline", 15, 20, 600, 0, false),
        ("body", 14, 20, 400, 0, false),
        ("subheadline", 13, 18, 400, 0, false),
        ("footnote", 12, 16, 400, 0, false),
        ("caption", 11, 14, 500, 10, false),
        ("caption2", 10, 13, 500, 20, false),
        ("overline", 11, 14, 600, 60, true),
    ];
    for (name, size, line, weight, tracking, upper) in table {
        let s = runtime.resolve_text_style(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(
            (s.size, s.line, s.weight, s.tracking_milli_em, s.upper),
            (size, line, weight, tracking, upper),
            "{name}"
        );
    }
}

#[test]
fn dark_shadows_are_stronger_than_light() {
    let light = at(Qualifier::Light);
    let dark = at(Qualifier::Dark);
    for name in ["shadow1", "shadow2", "shadow3", "shadow4", "shadowInset"] {
        let (l, d) = (light.resolve_shadow(name).unwrap(), dark.resolve_shadow(name).unwrap());
        assert_eq!(
            (l.x, l.y, l.blur, l.inset),
            (d.x, d.y, d.blur, d.inset),
            "{name}: one geometry"
        );
        assert!(d.color.a > l.color.a, "{name}: dark alpha above light");
    }
    let edge = light.resolve_shadow("edgeHighlight").unwrap();
    assert!(edge.inset && edge.y == 1 && edge.blur == 0);
    let runtime = ThemeRuntime::load(&themes_dir()).unwrap();
    for name in ["shadow1", "shadow2", "shadow3", "shadow4", "shadowInset", "edgeHighlight"] {
        assert!(runtime.defines_shadow(Qualifier::Dark, name), "{name}");
        assert!(runtime.defines_shadow(Qualifier::HighContrast, name), "{name}");
    }
}

#[test]
fn the_four_backdrops_and_high_contrast_flat() {
    let runtime = at(Qualifier::Dark);
    let b = |n: &str| runtime.resolve_material(n).unwrap();
    assert_eq!(
        b("ultraThin"),
        Backdrop { blur_radius_dp: 8, saturation_pct: 100, downsample_factor: 2 }
    );
    assert_eq!(b("thin").blur_radius_dp, 20);
    assert_eq!(
        b("regular"),
        Backdrop { blur_radius_dp: 40, saturation_pct: 180, downsample_factor: 2 }
    );
    assert_eq!(b("thick").blur_radius_dp, 64);
    let hc = at(Qualifier::HighContrast);
    for n in ["ultraThin", "thin", "regular", "thick"] {
        assert_eq!(hc.resolve_material(n).unwrap().blur_radius_dp, 0, "{n}");
    }
}

#[test]
fn the_scales_are_the_artifacts() {
    let runtime = at(Qualifier::Light);
    let r = |n: &str| runtime.resolve_radius(n).unwrap();
    assert_eq!((r("sm"), r("md"), r("lg"), r("xl"), r("full")), (8, 12, 16, 24, 9999));
    let c = |n: &str| runtime.resolve_scale("control", n).unwrap();
    assert_eq!((c("xs"), c("sm"), c("md"), c("lg"), c("xl")), (24, 30, 36, 44, 52));
    let g = |n: &str| runtime.resolve_spacing(n).unwrap();
    assert_eq!(
        (g("gapInline"), g("gapGroup"), g("gapSection"), g("marginLayout")),
        (8, 12, 24, 16)
    );
    assert_eq!(runtime.resolve_scale("zindex", "modal"), Some(30));
    assert_eq!(runtime.resolve_scale("motion", "quick"), Some(280));
}

// ---------------------------------------------------------------------------
// Schema validation (reject tests)
// ---------------------------------------------------------------------------

fn parse(src: &str) -> Result<nexus_theme::Theme, ThemeError> {
    nexus_theme::parse_theme_file(src, std::path::Path::new("test.toml"))
}

const HEAD: &str = "[theme]\nname = \"x\"\nversion = 2\n";

#[test]
fn test_reject_unknown_section() {
    let err = parse(&format!("{HEAD}[foobar]\nkey = \"val\"\n")).unwrap_err();
    assert!(matches!(err, ThemeError::UnknownSection { .. }));
}

#[test]
fn test_reject_the_retired_type_ramp_and_leading_sections() {
    for section in ["typography", "leading"] {
        let err = parse(&format!("{HEAD}[{section}]\nbase = 14\n")).unwrap_err();
        assert!(matches!(err, ThemeError::UnknownSection { .. }), "{section}");
    }
}

#[test]
fn test_reject_unknown_theme_key() {
    let err = parse("[theme]\nname = \"x\"\nversion = 1\nauthor = \"me\"\n").unwrap_err();
    assert!(matches!(err, ThemeError::UnknownKey { .. }));
}

#[test]
fn test_reject_invalid_version() {
    let err = parse("[theme]\nname = \"x\"\nversion = 0\n").unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}

#[test]
fn test_reject_invalid_theme_toml() {
    let err = parse("[tokens]\naccent = \"#ff0000\"\n").unwrap_err();
    assert!(matches!(err, ThemeError::MissingSection { .. }));
}

#[test]
fn test_reject_non_string_token() {
    let err = parse(&format!("{HEAD}[tokens]\naccent = 123\n")).unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}

#[test]
fn test_reject_a_glass_recipe_material() {
    let src = format!("{HEAD}[material.glassPanel]\ntype = \"glass\"\nblurRadiusDp = 40\nsaturationPct = 180\ndownsampleFactor = 2\n");
    assert!(matches!(parse(&src).unwrap_err(), ThemeError::SchemaValidation { .. }));
}

#[test]
fn test_reject_a_backdrop_missing_a_key_or_with_an_extra_one() {
    let missing = format!(
        "{HEAD}[material.thin]\ntype = \"backdrop\"\nblurRadiusDp = 20\ndownsampleFactor = 2\n"
    );
    assert!(matches!(parse(&missing).unwrap_err(), ThemeError::SchemaValidation { .. }));
    let extra = format!("{HEAD}[material.thin]\ntype = \"backdrop\"\nblurRadiusDp = 20\nsaturationPct = 100\ndownsampleFactor = 2\ntintColor = \"#ffffff\"\n");
    assert!(matches!(parse(&extra).unwrap_err(), ThemeError::UnknownKey { .. }));
}

#[test]
fn test_reject_a_text_style_off_the_weight_axis_or_shorter_than_its_size() {
    let style = |size: i64, line: i64, weight: i64, case: &str| {
        format!("{HEAD}[textStyle.x]\nsize = {size}\nline = {line}\nweight = {weight}\ntrackingMilliEm = 0\ncase = \"{case}\"\nfigures = \"proportional\"\n")
    };
    assert!(parse(&style(14, 20, 400, "none")).is_ok());
    assert!(parse(&style(14, 20, 450, "none")).is_err(), "weight off the 100 grid");
    assert!(parse(&style(14, 20, 900, "none")).is_err(), "weight outside 300-700");
    assert!(parse(&style(20, 14, 400, "none")).is_err(), "line shorter than size");
    assert!(parse(&style(14, 20, 400, "title")).is_err(), "unknown case");
}

/// `figures` is required and closed: `proportional` or `tabular` (RFC-0116 M-3).
#[test]
fn test_reject_a_text_style_without_or_with_unknown_figures() {
    let style = |figures: Option<&str>| {
        let line = figures.map(|f| format!("figures = \"{f}\"\n")).unwrap_or_default();
        format!("{HEAD}[textStyle.x]\nsize = 14\nline = 20\nweight = 400\ntrackingMilliEm = 0\ncase = \"none\"\n{line}")
    };
    assert!(parse(&style(Some("tabular"))).is_ok());
    assert!(parse(&style(Some("proportional"))).is_ok());
    assert!(parse(&style(Some("oldstyle"))).is_err(), "unknown figures");
    assert!(parse(&style(None)).is_err(), "figures is required");
}

#[test]
fn test_reject_a_shadow_without_a_colour_or_inset() {
    let no_inset =
        format!("{HEAD}[shadow.s]\nx = 0\ny = 1\nblur = 3\nspread = 0\ncolor = \"#000000\"\n");
    assert!(parse(&no_inset).is_err());
    let bad_colour = format!(
        "{HEAD}[shadow.s]\nx = 0\ny = 1\nblur = 3\nspread = 0\ncolor = \"black\"\ninset = false\n"
    );
    assert!(parse(&bad_colour).is_err());
}

#[test]
fn test_reject_non_integer_scale_value() {
    let err = parse(&format!("{HEAD}[radius]\nsm = \"8px\"\n")).unwrap_err();
    assert!(matches!(err, ThemeError::SchemaValidation { .. }));
}
