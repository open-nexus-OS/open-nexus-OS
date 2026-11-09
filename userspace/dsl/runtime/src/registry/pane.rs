// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The re-tint inside an opaque surface (RFC-0116; the design artifacts re-tint `glassCard`
//! for everything inside a pane or a floating surface): there a card, a grouped row or a bubble
//! is `fill` at half its alpha — the translucent white of a card on glass would wash out an
//! opaque pane. A token view, so a DSL node and a kit widget built inside the surface resolve
//! the same role the same way.

use nexus_layout_types::Rgba8;
use nexus_theme_tokens::{
    Backdrop, BackdropToken, ColorToken, MotionDurationToken, ShadowSpec, ShadowToken, Tokens,
};

/// `inner` as seen from inside a pane or an overlay.
pub(crate) struct PaneTokens<'a>(pub(crate) &'a dyn Tokens);

impl Tokens for PaneTokens<'_> {
    fn color(&self, token: ColorToken) -> Rgba8 {
        match token {
            ColorToken::GlassCard => {
                let fill = self.0.color(ColorToken::Fill);
                Rgba8 { a: ((u16::from(fill.a) + 1) / 2) as u8, ..fill }
            }
            other => self.0.color(other),
        }
    }

    fn shadow(&self, token: ShadowToken) -> ShadowSpec {
        self.0.shadow(token)
    }

    fn backdrop(&self, token: BackdropToken) -> Backdrop {
        self.0.backdrop(token)
    }

    fn motion_ms(&self, token: MotionDurationToken) -> u32 {
        self.0.motion_ms(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    #[test]
    fn a_card_inside_a_pane_is_half_the_fill() {
        for inner in [&BaseTokens as &dyn Tokens, &DarkTokens as &dyn Tokens] {
            let pane = PaneTokens(inner);
            let fill = inner.color(ColorToken::Fill);
            let card = pane.color(ColorToken::GlassCard);
            assert_eq!((card.r, card.g, card.b), (fill.r, fill.g, fill.b));
            assert_eq!(card.a, ((u16::from(fill.a) + 1) / 2) as u8);
            // Every other role is the theme's own.
            assert_eq!(pane.color(ColorToken::Label), inner.color(ColorToken::Label));
            assert_eq!(pane.color(ColorToken::SurfacePane), inner.color(ColorToken::SurfacePane));
        }
    }
}
