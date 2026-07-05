// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `GlassToggle` — the design-system on/off switch (handoff `GlassToggle`): an
//! iOS/macOS-style pill track with a sliding knob. A pure builder producing a
//! `LayoutNode::Stack` (track) containing the knob, positioned by `checked`
//! (justify Start/End). Colors come from theme tokens; the knob is the
//! conventional white switch cap. DSL-emittable; the compositor hit-tests `id`.

extern crate alloc;

use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Rgba8, Stack,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, Tokens};

/// Track geometry (logical px).
const TRACK_W: i32 = 44;
const TRACK_H: i32 = 26;
const KNOB: i32 = 22;
/// Inset around the knob = (TRACK_H - KNOB) / 2.
const INSET: i32 = 2;

/// The on/off switch.
#[derive(Debug, Clone, Default)]
pub struct GlassToggle {
    checked: bool,
    state: InteractionState,
    id: Option<&'static str>,
}

impl GlassToggle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The track fill for the current checked state (accent when on, neutral off).
    pub fn track_color(&self, tokens: &dyn Tokens) -> Rgba8 {
        tokens.color(if self.checked { ColorToken::Accent } else { ColorToken::SurfaceVariant })
    }

    fn knob_node() -> LayoutNode {
        // The knob is the conventional white switch cap (a physical-metaphor
        // constant, theme-independent — like iOS/macOS).
        let visual = Style::new()
            .background(Rgba8::new(255, 255, 255, 255))
            .rounded(FxPx::new(KNOB / 2))
            .visual();
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(KNOB)),
                max_width: Some(FxPx::new(KNOB)),
                min_height: Some(FxPx::new(KNOB)),
                max_height: Some(FxPx::new(KNOB)),
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }

    /// Build the switch node (track + knob).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut style = Style::new().background(self.track_color(tokens)).rounded(FxPx::new(TRACK_H / 2));
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity());
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::all(FxPx::new(INSET)),
                align: Align::Center,
                // Knob slides to the trailing edge when on.
                justify: if self.checked { Justify::End } else { Justify::Start },
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(TRACK_W)),
                max_width: Some(FxPx::new(TRACK_W)),
                min_height: Some(FxPx::new(TRACK_H)),
                max_height: Some(FxPx::new(TRACK_H)),
                item: FlexItem::default(),
            },
            style.visual(),
            alloc::vec![Self::knob_node()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn track_color_reflects_checked() {
        let t = BaseTokens;
        assert_eq!(GlassToggle::new().checked(true).track_color(&t), t.color(ColorToken::Accent));
        assert_eq!(
            GlassToggle::new().checked(false).track_color(&t),
            t.color(ColorToken::SurfaceVariant)
        );
    }

    #[test]
    fn knob_slides_with_checked_state() {
        let t = BaseTokens;
        let on = GlassToggle::new().checked(true).build(&t);
        let off = GlassToggle::new().checked(false).build(&t);
        let justify = |n: &LayoutNode| match n {
            LayoutNode::Stack(s, _, _) => s.justify,
            _ => panic!("toggle must be a Stack"),
        };
        assert_eq!(justify(&on), Justify::End);
        assert_eq!(justify(&off), Justify::Start);
    }

    #[test]
    fn has_a_knob_child_and_id() {
        let t = BaseTokens;
        match GlassToggle::new().id("wifi").build(&t) {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("wifi"));
                assert!(visual.background.is_some());
                assert_eq!(children.len(), 1, "the knob");
            }
            _ => panic!("toggle must be a Stack"),
        }
    }

    #[test]
    fn disabled_dims() {
        let t = BaseTokens;
        match GlassToggle::new().state(InteractionState::Disabled).build(&t) {
            LayoutNode::Stack(_, visual, _) => {
                assert_eq!(visual.opacity, Some(InteractionState::Disabled.opacity()))
            }
            _ => panic!(),
        }
    }
}
