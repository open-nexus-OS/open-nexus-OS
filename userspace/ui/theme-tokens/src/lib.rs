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
//! = swapping the `Tokens` impl — no widget code changes. This mirrors Apple's
//! semantic colors and ArkUI's resource tokens.
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

/// A resolved theme: maps semantic tokens to concrete values. The variable
/// interface for the UI — `no_std`, deterministic.
pub trait Tokens {
    fn color(&self, token: ColorToken) -> Rgba8;
    fn length(&self, token: LengthToken) -> FxPx;
}

/// The built-in baseline theme (a calm dark "glass" palette). Forks/products
/// provide their own `Tokens` impl (or a generated one) to rebrand without
/// touching widgets or shells.
#[derive(Debug, Clone, Copy, Default)]
pub struct BaseTokens;

impl Tokens for BaseTokens {
    fn color(&self, token: ColorToken) -> Rgba8 {
        match token {
            ColorToken::Surface => Rgba8::new(20, 24, 32, 235),
            ColorToken::SurfaceVariant => Rgba8::new(32, 38, 48, 235),
            ColorToken::OnSurface => Rgba8::new(236, 240, 245, 255),
            ColorToken::OnSurfaceVariant => Rgba8::new(160, 168, 180, 255),
            ColorToken::Accent => Rgba8::new(90, 150, 245, 255),
            ColorToken::OnAccent => Rgba8::new(8, 12, 20, 255),
            ColorToken::Border => Rgba8::new(255, 255, 255, 28),
            ColorToken::Shadow => Rgba8::new(0, 0, 0, 96),
            ColorToken::Background => Rgba8::new(10, 12, 16, 255),
        }
    }

    fn length(&self, token: LengthToken) -> FxPx {
        match token {
            LengthToken::RadiusSmall => FxPx::new(8),
            LengthToken::RadiusMedium => FxPx::new(16),
            LengthToken::RadiusLarge => FxPx::new(24),
            LengthToken::SpacingSmall => FxPx::new(8),
            LengthToken::SpacingMedium => FxPx::new(16),
            LengthToken::SpacingLarge => FxPx::new(24),
            LengthToken::BorderThin => FxPx::new(1),
        }
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
