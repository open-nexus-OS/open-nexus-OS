// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

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
//! data-only (no rendering, no app state). Next to it live the control system
//! every control primitive reads ([`control`]: sizes, label style, tint, role),
//! the interaction state ([`state`]) and the glyphs the controls draw themselves
//! ([`glyph`]).

extern crate alloc;

use nexus_layout_types::{
    BoxShadow, CornerRadius, EdgeBorder, Fraction, FxPx, Material, Rgba8, VisualStyle,
};
use nexus_theme_tokens::{
    recipe, ColorToken, RadiusToken, ShadowSpec, ShadowToken, Tokens, HAIRLINE,
};

pub mod control;
pub mod glyph;
pub mod state;
pub use state::{blend, Dim, InteractionState};

/// The width of the focus ring (the artifacts' `outline: 2px`).
pub const FOCUS_RING: FxPx = FxPx::new(2);

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

    /// No border on any edge (a recipe's edge dropped, e.g. the subtle card).
    pub fn no_border(mut self) -> Self {
        self.visual.border = EdgeBorder::none();
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

    /// A material's recipe (RFC-0116): its fill role, the 1 px glass edge, its shadow and lit top
    /// edge, and — for a surface on the wallpaper — its backdrop blur. THE single definition:
    /// the DSL's `.material(...)` and every kit widget route through here, so a page and a
    /// hand-written widget cannot render the same material differently. No shine gradient: a
    /// raised surface takes the one-pixel `edgeHighlight` instead.
    pub fn material(mut self, material: Material, tokens: &dyn Tokens) -> Self {
        let r = recipe(material);
        self.visual.material = Some(material);
        self.visual.background = Some(tokens.color(r.fill));
        if let Some(edge) = r.edge {
            self.visual.border = EdgeBorder::all(HAIRLINE, tokens.color(edge));
        }
        if let Some(shadow) = r.shadow {
            self.visual.shadow = Some(box_shadow(tokens.shadow(shadow)));
        }
        if r.edge_highlight {
            self.visual.inset_highlight = Some(tokens.shadow(ShadowToken::EdgeHighlight).color);
        }
        self.backdrop_blur = r.backdrop.map(|b| {
            let b = tokens.backdrop(b);
            BackdropBlur { radius: u32::from(b.blur), saturation_percent: u32::from(b.saturation) }
        });
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

    /// The 1 px hairline border in a colour role (`Separator`, `GlassEdge`).
    pub fn border_token(self, tokens: &dyn Tokens, color: ColorToken) -> Self {
        self.border(HAIRLINE, tokens.color(color))
    }

    /// Corner radius from a radius token.
    pub fn rounded_token(self, radius: RadiusToken) -> Self {
        self.rounded(radius.px())
    }

    /// An outer elevation (`Shadow1`–`Shadow4`) from the theme, or one of the two shadows drawn
    /// INSIDE the box as the painter's one-pixel top line: the lit edge (`EdgeHighlight`) in its
    /// light colour, the recessed well (`Inset`, a track) in its dark one — the painter has no
    /// blurred inset shadow, and the DSL's `.shadow(shadowInset)` draws the same line.
    pub fn shadow_token(mut self, tokens: &dyn Tokens, shadow: ShadowToken) -> Self {
        let spec = tokens.shadow(shadow);
        match shadow {
            ShadowToken::EdgeHighlight | ShadowToken::Inset => {
                self.visual.inset_highlight = Some(spec.color)
            }
            _ => self.visual.shadow = Some(box_shadow(spec)),
        }
        self
    }

    /// The keyboard focus ring (the artifacts' `outline: 2px solid <role>`): a 2 px ring in the
    /// role's colour just OUTSIDE the box, following its radius, changing neither the layout nor
    /// the border. `VisualStyle` has no outline, so the ring is drawn as a hard shadow spread
    /// 2 px (the painter knocks the box itself out, so it never shows through a translucent
    /// fill); it takes the place of the box's own shadow while focused, and it sits at offset 0
    /// where the artifacts leave a 2 px gap.
    pub fn focus_ring(mut self, tokens: &dyn Tokens, color: ColorToken) -> Self {
        self.visual.shadow = Some(BoxShadow {
            offset_x: FxPx::ZERO,
            offset_y: FxPx::ZERO,
            blur_radius: FxPx::ZERO,
            spread: FOCUS_RING,
            color: tokens.color(color),
        });
        self
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

/// A theme shadow as the painter's outer box shadow.
#[must_use]
pub fn box_shadow(spec: ShadowSpec) -> BoxShadow {
    BoxShadow {
        offset_x: FxPx::new(i32::from(spec.x)),
        offset_y: FxPx::new(i32::from(spec.y)),
        blur_radius: FxPx::new(i32::from(spec.blur)),
        spread: FxPx::new(i32::from(spec.spread)),
        color: spec.color,
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
            .background_token(&t, ColorToken::Fill)
            .rounded_token(RadiusToken::Md)
            .border_token(&t, ColorToken::Separator)
            .shadow_token(&t, ShadowToken::Shadow3);
        let vs = s.visual();
        assert_eq!(vs.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(vs.corner_radius, CornerRadius::uniform(FxPx::new(12)));
        assert!(vs.border.top.is_some());
        assert_eq!(vs.shadow.map(|s| s.blur_radius), Some(FxPx::new(30)));
    }

    #[test]
    fn the_inset_well_and_the_lit_edge_draw_inside_the_box() {
        use nexus_theme_tokens::DarkTokens;
        let t = DarkTokens;
        let well = Style::new().shadow_token(&t, ShadowToken::Inset).visual();
        assert_eq!(well.inset_highlight, Some(t.shadow(ShadowToken::Inset).color));
        assert_eq!(well.shadow, None, "the well casts no outer shadow");
        let lit = Style::new().shadow_token(&t, ShadowToken::EdgeHighlight).visual();
        assert_eq!(lit.inset_highlight, Some(t.shadow(ShadowToken::EdgeHighlight).color));
    }

    #[test]
    fn the_focus_ring_is_a_2px_ring_outside_the_box() {
        use nexus_theme_tokens::BaseTokens;
        let t = BaseTokens;
        let base = Style::new().border_token(&t, ColorToken::GlassEdge);
        let ring = base.clone().focus_ring(&t, ColorToken::Accent).visual();
        let shadow = ring.shadow.expect("the ring");
        assert_eq!(shadow.spread, FOCUS_RING);
        assert_eq!(
            (shadow.offset_x, shadow.offset_y, shadow.blur_radius),
            (FxPx::ZERO, FxPx::ZERO, FxPx::ZERO)
        );
        assert_eq!(shadow.color, t.color(ColorToken::Accent));
        assert_eq!(ring.border, base.visual().border, "the border stays");
    }

    #[test]
    fn a_material_is_its_recipe() {
        use nexus_theme_tokens::DarkTokens;
        let t = DarkTokens;
        let panel = Style::new().material(Material::Panel, &t);
        assert_eq!(panel.visual().background, Some(t.color(ColorToken::GlassPanel)));
        assert_eq!(
            panel.backdrop_blur(),
            Some(BackdropBlur { radius: 40, saturation_percent: 180 })
        );
        assert!(panel.visual().inset_highlight.is_some() && panel.visual().shadow.is_some());
        let card = Style::new().material(Material::Card, &t);
        assert_eq!(card.backdrop_blur(), None, "a card never blurs");
        assert_eq!(card.visual().background, Some(t.color(ColorToken::GlassCard)));
        let pane = Style::new().material(Material::Pane, &t);
        assert_eq!(pane.visual().border.top, None, "a pane has no glass edge");
        assert_eq!(pane.visual().background, Some(t.color(ColorToken::SurfacePane)));
    }
}
