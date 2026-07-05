// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Radio` — the design-system radio indicator (handoff `GlassRadioGroup` row):
//! a circle, glass outline when unselected, accent ring with a filled accent dot
//! when selected. A pure builder producing a `LayoutNode::Stack`. Compose several
//! (with labels) into a radio group. DSL-emittable.

extern crate alloc;

use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Stack, VisualStyle,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};

const SIZE: i32 = 20;
const DOT: i32 = 10;

/// A radio indicator.
#[derive(Debug, Clone, Default)]
pub struct Radio {
    selected: bool,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Radio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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

    /// The resolved ring [`Style`] (accent when selected/focused, else neutral).
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let border = if self.state.shows_focus_ring() {
            ColorToken::FocusRing
        } else if self.selected {
            ColorToken::Accent
        } else {
            ColorToken::Border
        };
        let mut s = Style::new()
            .rounded(FxPx::new(SIZE / 2))
            .border_token(tokens, LengthToken::BorderThin, border);
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity());
        }
        s
    }

    fn dot(tokens: &dyn Tokens) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(tokens.color(ColorToken::Accent)),
            corner_radius: CornerRadius::uniform(FxPx::new(DOT / 2)),
            ..VisualStyle::default()
        };
        let d = Some(FxPx::new(DOT));
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

    /// Build the radio node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let visual = self.style(tokens).visual();
        let children =
            if self.selected { alloc::vec![Self::dot(tokens)] } else { alloc::vec![] };
        let d = Some(FxPx::new(SIZE));
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
    fn selected_has_accent_ring_and_dot() {
        let t = BaseTokens;
        let on = Radio::new().selected(true);
        assert!(on.style(&t).visual().border.top.is_some());
        match on.build(&t) {
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 1, "the dot"),
            _ => panic!(),
        }
    }

    #[test]
    fn unselected_has_no_dot() {
        let t = BaseTokens;
        match Radio::new().selected(false).build(&t) {
            LayoutNode::Stack(_, _, children) => assert!(children.is_empty()),
            _ => panic!(),
        }
    }
}
