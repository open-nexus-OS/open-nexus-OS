// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Interaction state for design-system components — the shared vocabulary that
//! maps a control's live input state to deterministic visual modulation, so
//! every component (and its goldens) expresses default/hover/pressed/focus/
//! disabled identically. Pure data (no rendering, no app state): the compositor
//! decides the state by hit-testing the component's id; the component builds the
//! state-adjusted `Style` from tokens. This is the canonical state model the
//! windowd live path maps onto during convergence (RFC-0070 W6).

use nexus_layout_types::{Fraction, Rgba8};

/// A control's visual interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InteractionState {
    /// Resting.
    #[default]
    Default,
    /// Pointer over the control.
    Hover,
    /// Actively pressed.
    Pressed,
    /// Keyboard/focus target.
    Focused,
    /// Non-interactive, dimmed.
    Disabled,
}

impl InteractionState {
    /// Whether the control is non-interactive.
    pub fn is_disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }

    /// Whether to draw the focus ring.
    pub fn shows_focus_ring(self) -> bool {
        matches!(self, Self::Focused)
    }

    /// Element opacity for the state (disabled dims to ~55%).
    pub fn opacity(self) -> Fraction {
        match self {
            Self::Disabled => Fraction::new(140),
            _ => Fraction::OPAQUE,
        }
    }

    /// Alpha (0..255) of the feedback wash blended over a surface toward its
    /// foreground, giving tactile hover/press feedback (0 = none). Blending
    /// toward the *foreground* darkens on light themes and lightens on dark
    /// themes automatically.
    pub fn wash_alpha(self) -> u8 {
        match self {
            Self::Hover => 20,
            Self::Pressed => 40,
            _ => 0,
        }
    }
}

/// Alpha-composite `over` onto `base` at `over_alpha`/255 (straight alpha);
/// keeps `base`'s alpha channel. Deterministic integer math for stable goldens.
pub fn blend(base: Rgba8, over: Rgba8, over_alpha: u8) -> Rgba8 {
    let a = over_alpha as u32;
    let inv = 255 - a;
    let mix = |b: u8, o: u8| (((b as u32 * inv) + (o as u32 * a)) / 255) as u8;
    Rgba8::new(mix(base.r, over.r), mix(base.g, over.g), mix(base.b, over.b), base.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_dims_only() {
        assert!(InteractionState::Disabled.is_disabled());
        assert_eq!(InteractionState::Disabled.opacity(), Fraction::new(140));
        assert_eq!(InteractionState::Default.opacity(), Fraction::OPAQUE);
        assert_eq!(InteractionState::Hover.wash_alpha(), 20);
        assert_eq!(InteractionState::Pressed.wash_alpha(), 40);
        assert!(InteractionState::Focused.shows_focus_ring());
    }

    #[test]
    fn blend_endpoints() {
        let base = Rgba8::new(100, 100, 100, 255);
        let white = Rgba8::new(255, 255, 255, 255);
        assert_eq!(blend(base, white, 0), base);
        assert_eq!(blend(base, white, 255), Rgba8::new(255, 255, 255, 255));
        // 50% toward white.
        let mid = blend(base, white, 128);
        assert!(mid.r > base.r && mid.r < 255);
    }
}
