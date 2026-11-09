// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Interaction state for design-system components — the shared vocabulary that maps a
//! control's live input state to deterministic visuals, so every component (and its goldens)
//! expresses default/hover/pressed/focus/disabled identically. Pure data (no rendering, no app
//! state): the compositor decides the state by hit-testing the component's id; the component
//! builds the state-adjusted `Style` from tokens.
//!
//! Hover and press NEVER repaint a control (the design artifacts' control system): the host
//! shows a `fillHover` / `fillActive` layer over the control by opacity, so a widget builds the
//! same style for `Hover`, `Pressed` and `Default`. What a widget does draw from the state is
//! the disabled dim ([`InteractionState::opacity_as`]) and the focus ring
//! ([`InteractionState::shows_focus_ring`]).

use nexus_layout_types::{Fraction, Rgba8};

/// A control's visual interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InteractionState {
    /// Resting.
    #[default]
    Default,
    /// Pointer over the control (the host's hover layer; the control does not repaint).
    Hover,
    /// Actively pressed (the host's press layer; the control does not repaint).
    Pressed,
    /// Keyboard/focus target.
    Focused,
    /// Non-interactive, dimmed.
    Disabled,
}

/// How far a control dims when disabled — the artifacts set it per control family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dim {
    /// Buttons and text entry (button, text field, search, text area, progress): 0.50.
    #[default]
    Action,
    /// Value and selection controls (switch, checkbox, radio, segment, select, stepper, slider,
    /// PIN field, chip, list row): 0.45.
    Control,
    /// One option inside a control (a segment item, a select or menu row): 0.40.
    Option,
}

impl Dim {
    /// The disabled opacity of the family (0..255).
    #[must_use]
    pub const fn opacity(self) -> Fraction {
        match self {
            Dim::Action => Fraction::new(128),
            Dim::Control => Fraction::new(115),
            Dim::Option => Fraction::new(102),
        }
    }
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

    /// Element opacity for the state with the action family's dim (disabled: 0.50).
    pub fn opacity(self) -> Fraction {
        self.opacity_as(Dim::Action)
    }

    /// Element opacity for the state with `dim`'s disabled opacity; opaque otherwise.
    pub fn opacity_as(self, dim: Dim) -> Fraction {
        match self {
            Self::Disabled => dim.opacity(),
            _ => Fraction::OPAQUE,
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
    fn disabled_dims_per_control_family() {
        assert!(InteractionState::Disabled.is_disabled());
        assert_eq!(InteractionState::Disabled.opacity_as(Dim::Action), Fraction::new(128));
        assert_eq!(InteractionState::Disabled.opacity_as(Dim::Control), Fraction::new(115));
        assert_eq!(InteractionState::Disabled.opacity_as(Dim::Option), Fraction::new(102));
        assert_eq!(InteractionState::Disabled.opacity(), Dim::Action.opacity());
        for live in [
            InteractionState::Default,
            InteractionState::Hover,
            InteractionState::Pressed,
            InteractionState::Focused,
        ] {
            assert_eq!(live.opacity_as(Dim::Control), Fraction::OPAQUE, "{live:?}");
        }
        assert!(InteractionState::Focused.shows_focus_ring());
        assert!(!InteractionState::Hover.shows_focus_ring());
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
