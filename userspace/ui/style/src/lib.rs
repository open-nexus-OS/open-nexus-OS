// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! Composable visual **modifiers** for the UI framework (a declarative,
//! chainable-modifier style): a chainable [`Style`] builder that produces a
//! `nexus_layout_types::VisualStyle`, plus the one modifier that is not a
//! `VisualStyle` field — backdrop blur — carried alongside so a single chain
//! expresses every visual property.
//!
//! ```ignore
//! let s = Style::new()
//!     .background(surface)        // color (later: theme token)
//!     .rounded(FxPx::new(16))     // corner radius
//!     .border(FxPx::new(1), edge) // border
//!     .shadow(BoxShadow::default())
//!     .blur(20, 140);             // backdrop blur + saturation
//! let visual = s.visual();        // -> VisualStyle for a LayoutNode
//! let blur = s.backdrop_blur();   // -> Option<BackdropBlur> the widget emits
//! ```
//!
//! Widgets build their `LayoutNode` trees through `Style`; the layout engine
//! lays them out; the compositor paints the resulting `LayoutBox`es generically.
//! This is the modifier vocabulary a future DSL emits — so `Style` is pure and
//! data-only (no rendering, no app state).

use nexus_layout_types::{
    BoxShadow, CornerRadius, EdgeBorder, Fraction, FxPx, Rgba8, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

pub mod state;
pub use state::{blend, InteractionState};

/// Backdrop-blur modifier. The only visual modifier not representable on
/// `VisualStyle` (the renderer emits it as a backdrop filter over the region
/// behind the element). Carried on [`Style`] so one chain covers every modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackdropBlur {
    /// Blur radius in pixels.
    pub radius: u32,
    /// Saturation boost percent (100 = unchanged).
    pub saturation_percent: u32,
}

/// Chainable visual-modifier builder. Each method returns `self`, so modifiers
/// compose left-to-right. Pure: produces only data (`VisualStyle` + `BackdropBlur`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    visual: VisualStyle,
    backdrop_blur: Option<BackdropBlur>,
}

impl Style {
    /// An empty style (transparent, no border/rounding/shadow).
    pub fn new() -> Self {
        Self::default()
    }

    /// Background fill color.
    pub fn background(mut self, color: Rgba8) -> Self {
        self.visual.background = Some(color);
        self
    }

    /// Uniform border on all edges.
    pub fn border(mut self, width: FxPx, color: Rgba8) -> Self {
        self.visual.border = EdgeBorder::all(width, color);
        self
    }

    /// Uniform corner radius (rounded rectangle).
    pub fn rounded(mut self, radius: FxPx) -> Self {
        self.visual.corner_radius = CornerRadius::uniform(radius);
        self
    }

    /// Element opacity (0..255).
    pub fn opacity(mut self, opacity: Fraction) -> Self {
        self.visual.opacity = Some(opacity);
        self
    }

    /// Outer box shadow.
    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.visual.shadow = Some(shadow);
        self
    }

    /// Backdrop blur (glass) over the region behind the element.
    pub fn blur(mut self, radius: u32, saturation_percent: u32) -> Self {
        self.backdrop_blur = Some(BackdropBlur { radius, saturation_percent });
        self
    }

    // ── Theme-token modifiers ──────────────────────────────────────────────
    // Resolve a semantic token against a theme and apply it. Widgets/shells use
    // these so visual values come from the theme (variables), never hardcoded —
    // swap the `Tokens` impl to rebrand without touching widget code.

    /// Background from a color token.
    pub fn background_token(self, tokens: &dyn Tokens, color: ColorToken) -> Self {
        self.background(tokens.color(color))
    }

    /// Uniform border from length + color tokens.
    pub fn border_token(self, tokens: &dyn Tokens, width: LengthToken, color: ColorToken) -> Self {
        self.border(tokens.length(width), tokens.color(color))
    }

    /// Corner radius from a length token.
    pub fn rounded_token(self, tokens: &dyn Tokens, radius: LengthToken) -> Self {
        self.rounded(tokens.length(radius))
    }

    /// The accumulated `VisualStyle` (for a `LayoutNode`).
    pub fn visual(&self) -> VisualStyle {
        self.visual.clone()
    }

    /// The backdrop-blur modifier, if any (the widget emits it as a filter).
    pub fn backdrop_blur(&self) -> Option<BackdropBlur> {
        self.backdrop_blur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_chains_into_visual_and_blur() {
        let bg = Rgba8::new(10, 20, 30, 255);
        let edge = Rgba8::new(0, 0, 0, 255);
        let s = Style::new()
            .background(bg)
            .border(FxPx::new(2), edge)
            .rounded(FxPx::new(8))
            .opacity(Fraction::new(200))
            .blur(20, 140);

        let vs = s.visual();
        assert_eq!(vs.background, Some(bg));
        assert_eq!(vs.corner_radius, CornerRadius::uniform(FxPx::new(8)));
        assert!(vs.border.top.is_some());
        assert_eq!(vs.opacity, Some(Fraction::new(200)));
        assert_eq!(s.backdrop_blur(), Some(BackdropBlur { radius: 20, saturation_percent: 140 }));
    }

    #[test]
    fn empty_style_is_transparent() {
        let s = Style::new();
        assert_eq!(s.visual(), VisualStyle::default());
        assert_eq!(s.backdrop_blur(), None);
    }

    #[test]
    fn token_modifiers_pull_values_from_the_theme() {
        use nexus_theme_tokens::BaseTokens;
        let t = BaseTokens;
        let s = Style::new()
            .background_token(&t, ColorToken::Surface)
            .rounded_token(&t, LengthToken::RadiusMedium)
            .border_token(&t, LengthToken::BorderThin, ColorToken::Border);
        let vs = s.visual();
        assert_eq!(vs.background, Some(t.color(ColorToken::Surface)));
        assert_eq!(vs.corner_radius, CornerRadius::uniform(t.length(LengthToken::RadiusMedium)));
        assert!(vs.border.top.is_some());
    }
}
