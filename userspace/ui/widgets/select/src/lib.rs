// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Select` — the design-system dropdown trigger (handoff `Select`): a bordered
//! glass pill showing the selected value (or placeholder) with a trailing
//! chevron. A pure builder producing the CLOSED trigger `LayoutNode::Stack`; the
//! open option panel is an overlay (Popover/Menu, W4), so the app opens it on the
//! trigger's `id`. Placeholder shows in muted ink when no value. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    LineHeight, Overflow, ShapeKind, Spacer, Stack, TextAlign, TextContent, TextNode, TextStyle,
    VisualStyle, WhiteSpace,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};

/// A dropdown select trigger (closed state).
#[derive(Debug, Clone, Default)]
pub struct Select {
    value: Option<String>,
    placeholder: Option<String>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Select {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
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

    fn label_node(&self, tokens: &dyn Tokens) -> LayoutNode {
        // Value in primary ink; placeholder in muted ink.
        let (text, color) = match (&self.value, &self.placeholder) {
            (Some(v), _) => (v.clone(), ColorToken::OnSurface),
            (None, Some(p)) => (p.clone(), ColorToken::OnSurfaceVariant),
            (None, None) => (String::new(), ColorToken::OnSurfaceVariant),
        };
        LayoutNode::Text(
            TextNode {
                id: None,
                content: TextContent::new(text),
                style: TextStyle {
                    font_size: tokens.type_size(TypographyToken::Base),
                    font_weight: FontWeight::Regular,
                    line_height: LineHeight::Relative(FxPx::new(150)),
                    text_align: TextAlign::Left,
                    color: tokens.color(color),
                    white_space: WhiteSpace::NoWrap,
                },
                item: FlexItem::default(),
                max_lines: Some(1),
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        )
    }

    fn chevron(tokens: &dyn Tokens) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(tokens.color(ColorToken::OnSurfaceVariant)),
            shape: ShapeKind::TriangleDown,
            corner_radius: CornerRadius::uniform(FxPx::ZERO),
            ..VisualStyle::default()
        };
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
                min_width: Some(FxPx::new(10)),
                max_width: Some(FxPx::new(10)),
                min_height: Some(FxPx::new(6)),
                max_height: Some(FxPx::new(6)),
                item: FlexItem::default(),
            },
            visual,
            alloc::vec![],
        )
    }

    /// Build the select trigger node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let border =
            if self.state.shows_focus_ring() { ColorToken::FocusRing } else { ColorToken::Border };
        let mut style = Style::new()
            .background(tokens.color(ColorToken::Surface))
            .rounded(tokens.length(LengthToken::RadiusMedium))
            .border_token(tokens, LengthToken::BorderThin, border);
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity());
        }

        let spacer = LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 1,
            min_size: Some(FxPx::new(8)),
            item: FlexItem::default(),
        });

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12)),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(140)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            alloc::vec![self.label_node(tokens), spacer, Self::chevron(tokens)],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn builds_trigger_with_value_spacer_and_chevron() {
        let t = BaseTokens;
        match Select::new().id("lang").value("Deutsch").build(&t) {
            LayoutNode::Stack(stack, visual, children) => {
                assert_eq!(stack.id, Some("lang"));
                assert_eq!(visual.background, Some(t.color(ColorToken::Surface)));
                assert_eq!(children.len(), 3, "label + spacer + chevron");
                // The chevron is a down-triangle.
                match &children[2] {
                    LayoutNode::Stack(_, v, _) => assert_eq!(v.shape, ShapeKind::TriangleDown),
                    _ => panic!("chevron must be a shaped Stack"),
                }
            }
            _ => panic!("Select must build a Stack"),
        }
    }

    #[test]
    fn placeholder_uses_muted_ink() {
        let t = BaseTokens;
        let sel = Select::new().placeholder("Sprache");
        match sel.label_node(&t) {
            LayoutNode::Text(node, _) => {
                assert_eq!(node.style.color, t.color(ColorToken::OnSurfaceVariant))
            }
            _ => panic!(),
        }
    }
}
