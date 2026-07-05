// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `GlassCheckbox` — the design-system checkbox (handoff `GlassCheckbox`): a
//! rounded-square box, glass outline when off, accent fill with a white mark
//! when on. A pure builder producing a `LayoutNode::Stack`. The compositor
//! hit-tests `id`; the app maps id → toggle. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack, VisualStyle,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

const BOX: i32 = 20;
const MARK: i32 = 10;

/// A checkbox.
#[derive(Debug, Clone, Default)]
pub struct GlassCheckbox {
    checked: bool,
    state: InteractionState,
    id: Option<&'static str>,
}

impl GlassCheckbox {
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

    /// The resolved box [`Style`] (accent fill when checked, glass outline off).
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let mut s = Style::new().rounded(tokens.length(LengthToken::RadiusSmall));
        if self.checked {
            s = s.background(tokens.color(ColorToken::Accent));
        }
        let border = if self.state.shows_focus_ring() {
            ColorToken::FocusRing
        } else if self.checked {
            ColorToken::Accent
        } else {
            ColorToken::Border
        };
        s = s.border_token(tokens, LengthToken::BorderThin, border);
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity());
        }
        s
    }

    /// The check mark node (a white inner square) — the on-state indicator.
    fn mark(tokens: &dyn Tokens) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(tokens.color(ColorToken::OnAccent)),
            corner_radius: nexus_layout_types::CornerRadius::uniform(FxPx::new(2)),
            ..VisualStyle::default()
        };
        let d = Some(FxPx::new(MARK));
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
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }

    /// Build the checkbox node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let visual = self.style(tokens).visual();
        let children = if self.checked { alloc::vec![Self::mark(tokens)] } else { alloc::vec![] };
        let d = Some(FxPx::new(BOX));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem::default(),
            },
            visual,
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn checked_fills_accent_and_shows_mark() {
        let t = BaseTokens;
        let on = GlassCheckbox::new().checked(true);
        assert_eq!(on.style(&t).visual().background, Some(t.color(ColorToken::Accent)));
        match on.build(&t) {
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 1, "the mark"),
            _ => panic!(),
        }
    }

    #[test]
    fn unchecked_is_a_glass_outline() {
        let t = BaseTokens;
        let off = GlassCheckbox::new().checked(false);
        assert_eq!(off.style(&t).visual().background, None);
        assert!(off.style(&t).visual().border.top.is_some());
        match off.build(&t) {
            LayoutNode::Stack(_, _, children) => assert!(children.is_empty()),
            _ => panic!(),
        }
    }

    #[test]
    fn focus_and_disabled() {
        let t = BaseTokens;
        assert_eq!(
            GlassCheckbox::new().state(InteractionState::Disabled).style(&t).visual().opacity,
            Some(InteractionState::Disabled.opacity())
        );
        // Focus overrides the border color with the ring.
        let _ = GlassCheckbox::new().state(InteractionState::Focused).id("agree").build(&t);
    }
}
