// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The typed tokens against the design artifacts' values (RFC-0116) and the theme files.

use super::*;
use nexus_layout_types::Material;

fn rgba(hex: u32) -> Rgba8 {
    Rgba8::new((hex >> 24) as u8, (hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[test]
fn every_role_has_a_canonical_name_that_names_it_back() {
    for token in ColorToken::ALL {
        assert_eq!(ColorToken::from_name(token.name()), Some(token));
    }
    assert_eq!(ColorToken::from_name("onSurface"), None, "an old name names nothing");
    for style in TextStyleToken::ALL {
        assert_eq!(TextStyleToken::from_name(style.name()), Some(style));
    }
}

#[test]
fn the_light_and_dark_roles_are_the_artifacts() {
    assert_eq!(LightTokens.color(ColorToken::Label), rgba(0x0000_00e0));
    assert_eq!(LightTokens.color(ColorToken::Fill), rgba(0x7878_8029));
    assert_eq!(LightTokens.color(ColorToken::Accent), rgba(0x2563_ebff));
    assert_eq!(BaseTokens.color(ColorToken::Accent), LightTokens.color(ColorToken::Accent));
    assert_eq!(DarkTokens.color(ColorToken::Background), rgba(0x0a0a_0aff));
    assert_eq!(DarkTokens.color(ColorToken::GlassCard), rgba(0xffff_ff14));
    assert_eq!(DarkTokens.color(ColorToken::OnAccent), rgba(0xffff_ffff));
    assert_eq!(DarkTokens.color(ColorToken::Danger), rgba(0xfa5a_55ff));
    // Not theme-varying: dark inherits it.
    assert_eq!(DarkTokens.color(ColorToken::ModeTablet), LightTokens.color(ColorToken::ModeTablet));
    assert_eq!(HighContrastTokens.color(ColorToken::Background), rgba(0x0000_00ff));
    assert_eq!(HighContrastTokens.color(ColorToken::Label), rgba(0xffff_ffff));
}

#[test]
fn every_theme_distinguishes_the_label_steps() {
    for t in [&BaseTokens as &dyn Tokens, &DarkTokens, &LightTokens, &HighContrastTokens] {
        let (l, s, x) = (
            t.color(ColorToken::Label),
            t.color(ColorToken::LabelSecondary),
            t.color(ColorToken::LabelTertiary),
        );
        assert!(l != s && s != x, "three label steps");
    }
}

#[test]
fn dark_shadows_are_stronger_and_high_contrast_has_no_blur() {
    for s in
        [ShadowToken::Shadow1, ShadowToken::Shadow2, ShadowToken::Shadow3, ShadowToken::Shadow4]
    {
        assert!(DarkTokens.shadow(s).color.a > LightTokens.shadow(s).color.a, "{s:?}");
        assert!(!LightTokens.shadow(s).inset);
    }
    assert!(LightTokens.shadow(ShadowToken::Inset).inset);
    assert_eq!(LightTokens.shadow(ShadowToken::Shadow4).blur, 50);
    assert_eq!(
        LightTokens.backdrop(BackdropToken::Regular),
        Backdrop { blur: 40, saturation: 180, downsample: 2 }
    );
    assert_eq!(DarkTokens.backdrop(BackdropToken::Thick).blur, 64);
    for b in [
        BackdropToken::UltraThin,
        BackdropToken::Thin,
        BackdropToken::Regular,
        BackdropToken::Thick,
    ] {
        assert_eq!(HighContrastTokens.backdrop(b).blur, 0, "{b:?}");
    }
}

#[test]
fn the_scales_are_the_artifacts() {
    let r: Vec<i32> = [RadiusToken::Sm, RadiusToken::Md, RadiusToken::Lg, RadiusToken::Xl]
        .into_iter()
        .map(|t| t.px().0)
        .collect();
    assert_eq!(r, [8, 12, 16, 24]);
    assert!(RadiusToken::Full.px().0 >= 9999);
    let h: Vec<i32> =
        [ControlSize::Xs, ControlSize::Sm, ControlSize::Md, ControlSize::Lg, ControlSize::Xl]
            .into_iter()
            .map(|c| c.height().0)
            .collect();
    assert_eq!(h, [24, 30, 36, 44, 52]);
    assert_eq!(SpaceToken::GapInline.px(), FxPx::new(8));
    assert_eq!(SpaceToken::MarginLayout.px(), FxPx::new(16));
    assert!(
        on_spacing_grid(6) && on_spacing_grid(2) && !on_spacing_grid(28) && !on_spacing_grid(10)
    );
    assert_eq!(HAIRLINE, FxPx::new(1));
}

#[test]
fn a_nested_radius_is_concentric_and_never_below_sm() {
    assert_eq!(concentric(FxPx::new(24), FxPx::new(12)), FxPx::new(12));
    assert_eq!(concentric(FxPx::new(24), FxPx::new(16)), FxPx::new(8));
    assert_eq!(concentric(FxPx::new(16), FxPx::new(16)), FxPx::new(8), "the floor is sm");
}

#[test]
fn the_text_styles_and_the_control_labels() {
    let body = TextStyleToken::Body.spec();
    assert_eq!((body.size, body.line, body.weight), (14, 20, FontWeight::Normal));
    let overline = TextStyleToken::Overline.spec();
    assert!(overline.upper && overline.tracking_milli_em == 60);
    let display = TextStyleToken::Display.spec();
    assert_eq!(
        (display.size, display.weight, display.tracking_milli_em),
        (64, FontWeight::Light, -20)
    );
    assert_eq!(ControlSize::Md.label(), (TextStyleToken::Body, Some(FontWeight::Medium)));
    assert_eq!(ControlSize::Xl.field_radius(), RadiusToken::Lg);
    assert_eq!(style_weight(TextStyleToken::Title1), FontWeight::Bold);
}

#[test]
fn only_the_surfaces_on_the_wallpaper_blur() {
    for m in
        [Material::Panel, Material::Window, Material::Dock, Material::Thin, Material::UltraThin]
    {
        assert!(recipe(m).backdrop.is_some() && m.blurs(), "{m:?}");
    }
    for m in [Material::Card, Material::Pane, Material::Overlay] {
        assert!(recipe(m).backdrop.is_none() && !m.blurs(), "{m:?}");
    }
    assert_eq!(recipe(Material::Overlay).fill, ColorToken::SurfaceOverlay);
    assert_eq!(recipe(Material::Pane).edge, None);
    assert_eq!(recipe(Material::Card).edge, Some(ColorToken::GlassEdge));
    assert_eq!(recipe(Material::Dock).backdrop, Some(BackdropToken::Thick));
}

#[test]
fn a_user_accent_moves_the_tint_and_its_glow() {
    static DARK: DarkTokens = DarkTokens;
    let orange = Rgba8::new(249, 115, 22, 255);
    let t = AccentTokens { base: &DARK, accent: orange };
    assert_eq!(t.color(ColorToken::Accent), orange);
    let glow = t.color(ColorToken::AccentGlow);
    assert_eq!((glow.r, glow.g, glow.b), (249, 115, 22));
    assert_eq!(glow.a, DARK.color(ColorToken::AccentGlow).a);
    assert_eq!(t.color(ColorToken::Label), DARK.color(ColorToken::Label));
}

#[test]
fn a_solid_theme_is_its_glass_opaque_and_unblurred() {
    static DARK: DarkTokens = DarkTokens;
    static LIGHT: LightTokens = LightTokens;
    let luma =
        |c: Rgba8| (u32::from(c.r) * 299 + u32::from(c.g) * 587 + u32::from(c.b) * 114) / 1000;
    for (base, dark) in [(&DARK as &'static (dyn Tokens + Sync), true), (&LIGHT, false)] {
        let solid = SolidTokens { base };
        let (panel, window, card) = (
            solid.color(ColorToken::GlassPanel),
            solid.color(ColorToken::GlassWindow),
            solid.color(ColorToken::GlassCard),
        );
        for c in [panel, window, card] {
            assert_eq!(c.a, 255, "solid glass is opaque");
        }
        assert_eq!(panel, window, "panels and windows share the solid surface");
        // A card is the lighter surface: it stays visible on its panel.
        assert!(luma(card) > luma(panel), "card {card:?} must be lighter than panel {panel:?}");
        if dark {
            // Dark `glassPanel` is a white veil over the blur — its colour at full alpha turned every
            // dark panel and card white.
            assert!(luma(panel) < 0x50 && luma(card) < 0x60, "dark solid stays dark grey");
        } else {
            // Light panels are a grey tone, not white (the operator's call, 2026-11-09).
            assert!((0xC8..0xF4).contains(&luma(panel)), "light solid panel {panel:?} is grey");
        }
        assert_eq!(solid.backdrop(BackdropToken::Regular).blur, 0);
        assert_eq!(solid.color(ColorToken::Scrim), base.color(ColorToken::Scrim));
    }
}

#[test]
fn generated_motion_tokens_match_the_design() {
    assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Instant), 100);
    assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Swift), 160);
    assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Quick), 280);
    assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Base), 400);
    assert_eq!(DarkTokens.motion_ms(MotionDurationToken::Slow), 500);
    assert_eq!(MotionCurveToken::Spring.control_points(), [0.34, 1.4, 0.5, 1.0]);
    assert!(MotionCurveToken::SpringIcon.control_points()[1] > 1.0);
    assert!(MotionCurveToken::Smooth.control_points()[1] <= 1.0);
    assert_eq!(MotionCurveToken::Glide.control_points(), [0.22, 1.0, 0.36, 1.0]);
    assert_eq!(MotionCurveToken::Enter.control_points(), [0.16, 1.0, 0.3, 1.0]);
}

#[test]
fn a_theme_is_swappable_behind_the_trait() {
    struct Brand;
    impl Tokens for Brand {
        fn color(&self, _t: ColorToken) -> Rgba8 {
            Rgba8::new(255, 0, 0, 255)
        }
        fn shadow(&self, t: ShadowToken) -> ShadowSpec {
            BaseTokens.shadow(t)
        }
        fn backdrop(&self, t: BackdropToken) -> Backdrop {
            BaseTokens.backdrop(t)
        }
    }
    fn label(t: &dyn Tokens) -> Rgba8 {
        t.color(ColorToken::Label)
    }
    assert_ne!(label(&BaseTokens), label(&Brand));
}
