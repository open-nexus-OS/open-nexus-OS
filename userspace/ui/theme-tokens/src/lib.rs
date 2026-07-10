// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! Semantic **design tokens** — the interface through which a theme supplies
//! *variables* (colors, lengths) to modifiers and widgets.
//!
//! Tokens are **enum keys**, not strings: deterministic, `no_std`, and a clean
//! 1:1 target for the DSL (a DSL `color.surface` maps to [`ColorToken::Surface`]).
//! A theme is any [`Tokens`] implementation; widgets/shells resolve
//! `tokens.color(ColorToken::Surface)` etc. and feed concrete values into the
//! `Style` modifier builder. Swapping the theme (light/dark, an enterprise brand)
//! = swapping the `Tokens` impl — no widget code changes. This is the
//! semantic-color / resource-token model.
//!
//! The authoring side (`.nxtheme.toml`, the `nexus-theme` std crate) resolves to
//! one of these `Tokens` snapshots (build-time generated or runtime-selected);
//! this crate is the `no_std` runtime contract consumed by the UI.

use nexus_layout_types::{FxPx, Rgba8};

/// Semantic color roles. Add roles here (and to every theme) rather than using
/// raw colors in widgets/shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorToken {
    /// Base surface (panels, cards, sheets).
    Surface,
    /// A raised/variant surface (hover, selected rows).
    SurfaceVariant,
    /// Primary content (text/icons) on `Surface`.
    OnSurface,
    /// Secondary/dimmed content on `Surface`.
    OnSurfaceVariant,
    /// Accent / primary action.
    Accent,
    /// Content on `Accent`.
    OnAccent,
    /// Hairline borders / separators.
    Border,
    /// Shadow tint.
    Shadow,
    /// Window/desktop background.
    Background,
    /// Brand ink (headings, high-emphasis).
    Primary,
    /// Content on `Primary`.
    OnPrimary,
    /// Destructive / danger action.
    Danger,
    /// Content on `Danger`.
    OnDanger,
    /// Warning status.
    Warning,
    /// Success status.
    Success,
    /// Informational status.
    Info,
    /// Content on `Warning`.
    OnWarning,
    /// Content on `Success`.
    OnSuccess,
    /// Content on `Info`.
    OnInfo,
    /// Focus ring / keyboard focus indicator.
    FocusRing,
}

/// Semantic length roles (radii, spacing, hairline widths).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LengthToken {
    RadiusSmall,
    RadiusMedium,
    RadiusLarge,
    SpacingSmall,
    SpacingMedium,
    SpacingLarge,
    BorderThin,
}

/// Font-size step (handoff type scale). Theme-invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypographyToken {
    /// 11px — status bar, captions.
    Xs,
    /// 12px — metadata, timestamps, helper text.
    Sm,
    /// 14px — body, list items.
    Base,
    /// 16px — labels, UI text.
    Md,
    /// 18px — subheadings.
    Lg,
    /// 20px — headings.
    Xl,
    /// 24px — titles.
    Xxl,
    /// 30px — large headings.
    Xxxl,
    /// 36px — display.
    Display,
}

/// Motion duration step (handoff motion scale, ms). Themable so a
/// reduced-motion theme can zero every step; authored in `[motion]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionDurationToken {
    /// 100ms — press-down feedback.
    Instant,
    /// 160ms — hover, color/background shifts.
    Swift,
    /// 280ms — toggles, small controls.
    Quick,
    /// 400ms — collapse, dismiss.
    Base,
    /// 500ms — spring expand, window entry.
    Slow,
}

/// Motion easing curve (handoff motion vocabulary). The control points are the
/// theme-invariant cubic-beziers from `reference/tokens/motion.css` — one
/// physics vocabulary for the whole OS: enter/expand springs, exits are smooth
/// and quicker, micro-feedback is swift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MotionCurveToken {
    /// Panel/expand entry, elastic overshoot.
    Spring,
    /// Subtle overshoot: switch thumbs, chips.
    SpringSoft,
    /// Icon pop, strongest overshoot.
    SpringIcon,
    /// Collapse, exit, fades — never bouncy on the way out.
    Smooth,
    /// Page swipes, large moves.
    Glide,
}

impl MotionCurveToken {
    /// The `cubic-bezier(x1, y1, x2, y2)` control points.
    pub const fn control_points(self) -> [f32; 4] {
        match self {
            MotionCurveToken::Spring => [0.34, 1.4, 0.5, 1.0],
            MotionCurveToken::SpringSoft => [0.34, 1.2, 0.5, 1.0],
            MotionCurveToken::SpringIcon => [0.34, 1.56, 0.64, 1.0],
            MotionCurveToken::Smooth => [0.4, 0.0, 0.2, 1.0],
            MotionCurveToken::Glide => [0.22, 1.0, 0.36, 1.0],
        }
    }
}

/// Liquid-glass material level (handoff panel/card/subtle/window/overlay).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialToken {
    /// Main surfaces (dock, control center, launcher).
    Panel,
    /// Nested cards inside panels.
    Card,
    /// Settings rows / list items.
    Subtle,
    /// Dense app-window chrome.
    Window,
    /// Modal/alert/sheet/popover reading surface.
    Overlay,
}

/// A resolved glass material: the pre-composited values a widget feeds into a
/// `Style` to render one liquid-glass surface (fill tint, top-shine edge, 1px
/// border, backdrop blur). Colors carry their alpha already (color × material
/// alpha); `saturation` is the backdrop saturation percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlassSurface {
    pub tint: Rgba8,
    pub edge: Rgba8,
    pub border: Option<Rgba8>,
    pub blur_radius: u32,
    pub saturation: u32,
    pub downsample: u32,
}

/// A resolved theme: maps semantic tokens to concrete values. The variable
/// interface for the UI — `no_std`, deterministic.
pub trait Tokens {
    fn color(&self, token: ColorToken) -> Rgba8;
    fn length(&self, token: LengthToken) -> FxPx;

    /// Font size (px) for a type-scale step. Theme-invariant, so the default
    /// impl (generated from `[typography]`) is authoritative for every theme.
    fn type_size(&self, token: TypographyToken) -> FxPx {
        type_size(token)
    }

    /// Motion duration in ms for a motion step (generated from `[motion]`).
    /// A reduced-motion theme overrides this to 0 — animation drivers must
    /// treat 0 as "jump to the final frame".
    fn motion_ms(&self, token: MotionDurationToken) -> u32 {
        motion_duration_ms(token)
    }

    /// The resolved glass material for a level. The default is a non-blurred
    /// opaque-ish fallback derived from the color tokens; generated themes
    /// override it with the authored `[material.glass*]` values.
    fn glass(&self, _token: MaterialToken) -> GlassSurface {
        GlassSurface {
            tint: self.color(ColorToken::Surface),
            edge: Rgba8::TRANSPARENT,
            border: Some(self.color(ColorToken::Border)),
            blur_radius: 0,
            saturation: 100,
            downsample: 1,
        }
    }
}

// Color snapshots + the length scale, generated from `resources/themes/*.nxtheme.toml`
// (RFC-0070 D3). Provides `base_color`/`dark_color`/`light_color`/`highcontrast_color`
// and `scale_length` (radius/spacing from `[radius]`/`[spacing]`; BorderThin = 1px).
include!(concat!(env!("OUT_DIR"), "/generated_tokens.rs"));

/// The built-in **base** theme, resolved from `base.nxtheme.toml` (the
/// light-leaning default layer). Values are generated from the theme SSOT, not
/// hand-authored — swapping the theme = using [`DarkTokens`]/[`LightTokens`]/
/// [`HighContrastTokens`] (or any [`Tokens`] impl); no widget code changes.
#[derive(Debug, Clone, Copy, Default)]
pub struct BaseTokens;

impl Tokens for BaseTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        base_color(token)
    }
    fn length(&self, token: LengthToken) -> FxPx {
        scale_length(token)
    }
    fn glass(&self, token: MaterialToken) -> GlassSurface {
        base_glass(token)
    }
}

/// Dark theme, resolved from `dark.nxtheme.toml` (falling back to base).
#[derive(Debug, Clone, Copy, Default)]
pub struct DarkTokens;

impl Tokens for DarkTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        dark_color(token)
    }
    fn length(&self, token: LengthToken) -> FxPx {
        scale_length(token)
    }
    fn glass(&self, token: MaterialToken) -> GlassSurface {
        dark_glass(token)
    }
}

/// Light theme, resolved from `light.nxtheme.toml` (falling back to base).
#[derive(Debug, Clone, Copy, Default)]
pub struct LightTokens;

impl Tokens for LightTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        light_color(token)
    }
    fn length(&self, token: LengthToken) -> FxPx {
        scale_length(token)
    }
    fn glass(&self, token: MaterialToken) -> GlassSurface {
        light_glass(token)
    }
}

/// High-contrast a11y theme, resolved from `highcontrast.nxtheme.toml`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HighContrastTokens;

impl Tokens for HighContrastTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        highcontrast_color(token)
    }
    fn length(&self, token: LengthToken) -> FxPx {
        scale_length(token)
    }
    fn glass(&self, token: MaterialToken) -> GlassSurface {
        highcontrast_glass(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_tokens_resolve_every_role() {
        let t = BaseTokens;
        // Distinct, opaque-enough surface vs. its variant.
        assert_ne!(t.color(ColorToken::Surface), t.color(ColorToken::SurfaceVariant));
        assert_ne!(t.color(ColorToken::OnSurface), t.color(ColorToken::OnSurfaceVariant));
        // Lengths form a sane ascending scale.
        assert!(t.length(LengthToken::RadiusSmall) < t.length(LengthToken::RadiusLarge));
        assert!(t.length(LengthToken::SpacingSmall) < t.length(LengthToken::SpacingLarge));
        assert_eq!(t.length(LengthToken::BorderThin), FxPx::new(1));
    }

    #[test]
    fn generated_tokens_match_theme_toml() {
        // Locks the build.rs generation to the `.nxtheme.toml` SSOT (RFC-0070 D3):
        // no more hand-authored drift. base = light-leaning default, dark overrides.
        assert_eq!(BaseTokens.color(ColorToken::Surface), Rgba8::new(255, 255, 255, 255)); // #ffffff
        assert_eq!(BaseTokens.color(ColorToken::Accent), Rgba8::new(59, 130, 246, 255)); // #3b82f6
        assert_eq!(DarkTokens.color(ColorToken::Surface), Rgba8::new(23, 23, 23, 255)); // #171717
        assert_eq!(DarkTokens.color(ColorToken::Background), Rgba8::new(10, 10, 10, 255)); // #0a0a0a
        assert_eq!(DarkTokens.color(ColorToken::Accent), Rgba8::new(96, 165, 250, 255)); // #60a5fa
        assert_eq!(LightTokens.color(ColorToken::Surface), Rgba8::new(255, 255, 255, 255)); // #ffffff
        // High contrast: pure black background, white foreground.
        assert_eq!(HighContrastTokens.color(ColorToken::Background), Rgba8::new(0, 0, 0, 255));
        assert_eq!(HighContrastTokens.color(ColorToken::OnSurface), Rgba8::new(255, 255, 255, 255));
        // Base falls back for a role a theme doesn't override (Shadow only in base).
        assert_eq!(DarkTokens.color(ColorToken::Shadow), Rgba8::new(0, 0, 0, 96)); // #00000060
    }

    #[test]
    fn generated_glass_materials_from_toml() {
        // base glassPanel: tint #ffffff@.50, blur 40, border #ffffff@.75, sat 140.
        let p = BaseTokens.glass(MaterialToken::Panel);
        assert_eq!(p.tint, Rgba8::new(255, 255, 255, 128));
        assert_eq!(p.blur_radius, 40);
        assert_eq!(p.border, Some(Rgba8::new(255, 255, 255, 191)));
        assert_eq!(p.saturation, 140);
        // dark panel is more translucent (#ffffff@.10).
        assert_eq!(DarkTokens.glass(MaterialToken::Panel).tint, Rgba8::new(255, 255, 255, 26));
        // light inherits base materials via the qualifier chain.
        assert_eq!(LightTokens.glass(MaterialToken::Panel).blur_radius, 40);
        // high contrast zeroes blur (a11y).
        assert_eq!(HighContrastTokens.glass(MaterialToken::Overlay).blur_radius, 0);
    }

    #[test]
    fn generated_type_sizes_match_toml() {
        let t = BaseTokens;
        assert_eq!(t.type_size(TypographyToken::Base), FxPx::new(14)); // body
        assert_eq!(t.type_size(TypographyToken::Sm), FxPx::new(12)); // helper
        assert_eq!(t.type_size(TypographyToken::Display), FxPx::new(36));
        // Invariant across themes (default trait impl).
        assert_eq!(DarkTokens.type_size(TypographyToken::Md), FxPx::new(16));
    }

    #[test]
    fn generated_scale_length_matches_toml() {
        // scale_length is generated from base's [radius]/[spacing]; BorderThin = 1px.
        assert_eq!(BaseTokens.length(LengthToken::RadiusSmall), FxPx::new(6));
        assert_eq!(BaseTokens.length(LengthToken::RadiusLarge), FxPx::new(16));
        assert_eq!(BaseTokens.length(LengthToken::SpacingMedium), FxPx::new(16));
        assert_eq!(BaseTokens.length(LengthToken::BorderThin), FxPx::new(1));
        // Every theme shares the invariant scale.
        assert_eq!(DarkTokens.length(LengthToken::RadiusMedium), FxPx::new(10));
    }

    #[test]
    fn generated_motion_tokens_match_handoff() {
        // Durations from [motion] (reference/tokens/motion.css: 0.10..0.50s).
        assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Instant), 100);
        assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Swift), 160);
        assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Quick), 280);
        assert_eq!(BaseTokens.motion_ms(MotionDurationToken::Base), 400);
        assert_eq!(DarkTokens.motion_ms(MotionDurationToken::Slow), 500);
        // Curves are the handoff cubic-beziers; every spring overshoots (y1 > 1),
        // exits never do.
        assert_eq!(MotionCurveToken::Spring.control_points(), [0.34, 1.4, 0.5, 1.0]);
        assert!(MotionCurveToken::SpringIcon.control_points()[1] > 1.0);
        assert!(MotionCurveToken::Smooth.control_points()[1] <= 1.0);
        assert_eq!(MotionCurveToken::Glide.control_points(), [0.22, 1.0, 0.36, 1.0]);
    }

    #[test]
    fn theme_is_swappable_behind_the_trait() {
        // A second theme (e.g. an enterprise brand) — same widgets, new values.
        struct Brand;
        impl Tokens for Brand {
            fn color(&self, _t: ColorToken) -> Rgba8 {
                Rgba8::new(255, 0, 0, 255)
            }
            fn length(&self, _t: LengthToken) -> FxPx {
                FxPx::new(4)
            }
        }
        fn surface(t: &dyn Tokens) -> Rgba8 {
            t.color(ColorToken::Surface)
        }
        assert_ne!(surface(&BaseTokens), surface(&Brand));
    }
}
