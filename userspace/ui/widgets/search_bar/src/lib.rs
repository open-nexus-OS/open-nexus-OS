// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `SearchBar` — the design-system search pill (handoff `SearchBar`): a
//! rounded-full glass pill with a leading magnifier, the input, and a trailing
//! clear affordance (caller-provided icon nodes). A pure builder producing a
//! `LayoutNode::Stack` row; the inner input is the low-level [`TextField`]
//! primitive (app owns the value). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, LineHeight,
    Overflow, Stack, TextAlign, TextStyle, WhiteSpace,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, Tokens, TypographyToken};
use nexus_widget_text_field::TextField;

/// Fully-rounded pill radius (clamped to height/2 by the renderer).
const PILL_RADIUS: i32 = 999;

/// An iOS-style search pill.
#[derive(Debug, Clone, Default)]
pub struct SearchBar {
    value: String,
    placeholder: Option<String>,
    leading: Option<LayoutNode>,
    trailing: Option<LayoutNode>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl SearchBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
    /// Leading node (typically a magnifier icon).
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    /// Trailing node (typically a clear button, shown when non-empty).
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(trailing);
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

    /// Build the search pill node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let border = if self.state.shows_focus_ring() {
            ColorToken::FocusRing
        } else {
            ColorToken::Border
        };
        let mut style = Style::new()
            .background(tokens.color(ColorToken::SurfaceVariant))
            .rounded(FxPx::new(PILL_RADIUS))
            .border_token(tokens, nexus_theme_tokens::LengthToken::BorderThin, border);
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity());
        }

        let input = {
            let mut tf = TextField::new()
                .style(Style::new())
                .text_style(TextStyle {
                    font_size: tokens.type_size(TypographyToken::Base),
                    font_weight: FontWeight::Regular,
                    line_height: LineHeight::Relative(FxPx::new(150)),
                    text_align: TextAlign::Left,
                    color: tokens.color(ColorToken::OnSurface),
                    white_space: WhiteSpace::NoWrap,
                })
                .value(self.value.clone());
            if let Some(p) = &self.placeholder {
                tf = tf.placeholder(p.clone());
            }
            if let Some(id) = self.id {
                tf = tf.id(id);
            }
            tf.build()
        };

        let mut children: Vec<LayoutNode> = Vec::new();
        if let Some(leading) = self.leading {
            children.push(leading);
        }
        children.push(input);
        if let Some(trailing) = self.trailing {
            children.push(trailing);
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: EdgeInsets::symmetric(FxPx::new(7), FxPx::new(12)),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(180)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FlexItem, Spacer};
    use nexus_theme_tokens::BaseTokens;

    fn icon() -> LayoutNode {
        LayoutNode::Spacer(Spacer { id: None, flex_grow: 0, min_size: Some(FxPx::new(16)), item: FlexItem::default() })
    }

    #[test]
    fn builds_a_pill_with_leading_input_and_id() {
        let t = BaseTokens;
        match SearchBar::new().id("q").leading(icon()).placeholder("Apps suchen").build(&t) {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("q"));
                assert_eq!(visual.background, Some(t.color(ColorToken::SurfaceVariant)));
                assert!(visual.corner_radius.top_left.0 >= 999);
                assert_eq!(children.len(), 2, "leading + input");
            }
            _ => panic!("SearchBar must build a Stack"),
        }
    }

    #[test]
    fn focus_ring_and_trailing_clear() {
        let t = BaseTokens;
        match SearchBar::new().state(InteractionState::Focused).value("x").trailing(icon()).build(&t) {
            LayoutNode::Stack(_, visual, children) => {
                assert!(visual.border.top.is_some());
                assert_eq!(children.len(), 2, "input + clear");
            }
            _ => panic!(),
        }
    }
}
