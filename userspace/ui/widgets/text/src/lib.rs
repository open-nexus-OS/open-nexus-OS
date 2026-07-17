// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Text` — the design-system text primitive: a themed `LayoutNode::Text`
//! builder (size from the typography scale, weight, semantic color, alignment,
//! line clamp). The single place components turn a string into a text node, so
//! type sizing/coloring stays consistent and token-driven. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    FlexItem, FontWeight, FxPx, LayoutNode, LineHeight, TextAlign, TextContent, TextNode,
    TextStyle, VisualStyle, WhiteSpace,
};
use nexus_theme_tokens::{ColorToken, Tokens, TypographyToken};

// Re-export FontWeight so callers style weight without a second import path.
pub use nexus_layout_types::FontWeight as Weight;

/// A themed text run.
#[derive(Debug, Clone)]
pub struct Text {
    content: String,
    size: TypographyToken,
    weight: FontWeight,
    color: ColorToken,
    align: TextAlign,
    max_lines: Option<u32>,
    wrap: bool,
}

impl Text {
    /// Body text (14px, regular, `OnSurface`, single line).
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            size: TypographyToken::Base,
            weight: FontWeight::Regular,
            color: ColorToken::OnSurface,
            align: TextAlign::Left,
            max_lines: Some(1),
            wrap: false,
        }
    }

    /// A title (24px, semibold, `OnSurface`).
    pub fn title(content: impl Into<String>) -> Self {
        Self::new(content).size(TypographyToken::Xxl).weight(FontWeight::Semibold)
    }

    /// A caption (12px, regular, muted).
    pub fn caption(content: impl Into<String>) -> Self {
        Self::new(content).size(TypographyToken::Sm).color(ColorToken::OnSurfaceVariant)
    }

    pub fn size(mut self, size: TypographyToken) -> Self {
        self.size = size;
        self
    }
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
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

    /// The resolved `TextStyle` for the current theme.
    pub fn style(&self, tokens: &dyn Tokens) -> TextStyle {
        TextStyle {
            font_size: tokens.type_size(self.size),
            font_weight: self.weight,
            line_height: LineHeight::Relative(FxPx::new(150)),
            text_align: self.align,
            color: tokens.color(self.color),
            white_space: if self.wrap { WhiteSpace::Normal } else { WhiteSpace::NoWrap },
        }
    }

    /// Build the text node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.style(tokens);
        LayoutNode::Text(
            TextNode {
                id: None,
                content: TextContent::new(self.content),
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
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn presets_resolve_size_and_color_from_tokens() {
        let t = BaseTokens;
        assert_eq!(Text::new("x").style(&t).font_size, t.type_size(TypographyToken::Base));
        assert_eq!(Text::title("H").style(&t).font_weight, FontWeight::Semibold);
        assert_eq!(Text::caption("c").style(&t).color, t.color(ColorToken::OnSurfaceVariant));
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
