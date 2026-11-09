// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Text` — the design-system text primitive: a themed `LayoutNode::Text` builder. Text is set
//! only through the 12 text styles (RFC-0116: size, line, weight, tracking and case together;
//! a weight override is the one deviation), a colour role, alignment and a line clamp. The single
//! place components turn a string into a text node, so type stays consistent and token-driven.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    FlexItem, FontWeight, LayoutNode, TextAlign, TextContent, TextNode, TextStyle, VisualStyle,
    WhiteSpace,
};
use nexus_theme_tokens::{ColorToken, TextStyleToken, Tokens};

// Re-export FontWeight so callers style weight without a second import path.
pub use nexus_layout_types::FontWeight as Weight;

/// A themed text run.
#[derive(Debug, Clone)]
pub struct Text {
    content: String,
    style: TextStyleToken,
    weight: Option<FontWeight>,
    color: ColorToken,
    align: TextAlign,
    max_lines: Option<u32>,
    wrap: bool,
}

impl Text {
    /// Body text (`body`, `label`, one line).
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: TextStyleToken::Body,
            weight: None,
            color: ColorToken::Label,
            align: TextAlign::Left,
            max_lines: Some(1),
            wrap: false,
        }
    }

    /// A title (`title1`).
    pub fn title(content: impl Into<String>) -> Self {
        Self::new(content).text_style(TextStyleToken::Title1)
    }

    /// A caption (`footnote`, `labelSecondary`).
    pub fn caption(content: impl Into<String>) -> Self {
        Self::new(content).text_style(TextStyleToken::Footnote).color(ColorToken::LabelSecondary)
    }

    /// A section header (`overline`, `labelSecondary`): 11/14 semibold, tracked +0.06 em and
    /// set in capitals — the header of a list section, a sidebar group or a menu.
    pub fn overline(content: impl Into<String>) -> Self {
        Self::new(content).text_style(TextStyleToken::Overline).color(ColorToken::LabelSecondary)
    }

    /// The text style (size, line, weight, tracking and case).
    pub fn text_style(mut self, style: TextStyleToken) -> Self {
        self.style = style;
        self
    }
    /// A weight override (the one deviation from the style the contract allows).
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
    /// Allow wrapping and clamp to `lines` (None = unclamped).
    pub fn lines(mut self, lines: Option<u32>) -> Self {
        self.max_lines = lines;
        self.wrap = true;
        self
    }

    /// The resolved layout `TextStyle` for the current theme.
    pub fn layout_style(&self, tokens: &dyn Tokens) -> TextStyle {
        TextStyle {
            text_align: self.align,
            white_space: if self.wrap { WhiteSpace::Normal } else { WhiteSpace::NoWrap },
            ..self.style.text_style(tokens.color(self.color), self.weight)
        }
    }

    /// Build the text node (an upper-case style sets its content in capitals).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.layout_style(tokens);
        let content =
            if self.style.spec().upper { self.content.to_uppercase() } else { self.content };
        LayoutNode::Text(
            TextNode {
                id: None,
                content: TextContent::new(content),
                style,
                item: FlexItem::default(),
                max_lines: self.max_lines,
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FxPx, LineHeight};
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn presets_resolve_the_text_styles_and_roles() {
        let t = BaseTokens;
        let body = Text::new("x").layout_style(&t);
        assert_eq!(
            (body.font_size, body.line_height),
            (FxPx::new(14), LineHeight::Absolute(FxPx::new(20)))
        );
        assert_eq!(Text::title("H").layout_style(&t).font_weight, FontWeight::Bold);
        assert_eq!(Text::caption("c").layout_style(&t).color, t.color(ColorToken::LabelSecondary));
        assert_eq!(
            Text::new("x").weight(FontWeight::Medium).layout_style(&t).font_weight,
            FontWeight::Medium
        );
    }

    #[test]
    fn an_overline_sets_capitals_and_tracking() {
        match Text::new("Netzwerk").text_style(TextStyleToken::Overline).build(&BaseTokens) {
            LayoutNode::Text(node, _) => {
                assert_eq!(node.content.as_str(), "NETZWERK");
                assert_eq!(node.style.tracking_milli_em, 60);
            }
            _ => panic!("Text must build a Text node"),
        }
        // The preset: the secondary label, and capitals beyond ASCII.
        match Text::overline("Geräte").build(&BaseTokens) {
            LayoutNode::Text(node, _) => {
                assert_eq!(node.content.as_str(), "GERÄTE");
                assert_eq!(node.style.color, BaseTokens.color(ColorToken::LabelSecondary));
                assert_eq!(node.style.font_weight, FontWeight::Semibold);
            }
            _ => panic!("Text must build a Text node"),
        }
    }

    #[test]
    fn builds_a_text_node() {
        match Text::new("hi").color(ColorToken::Accent).build(&BaseTokens) {
            LayoutNode::Text(node, _) => {
                assert_eq!(node.content.as_str(), "hi");
                assert_eq!(node.max_lines, Some(1));
            }
            _ => panic!("Text must build a Text node"),
        }
    }
}
