// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Toolbar` — the design-system top bar (handoff `Toolbar`): a frosted bar with
//! leading and trailing slots and a leading-or-centered title (+ optional
//! subtitle). A pure builder producing a `LayoutNode::Stack` row. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};
use nexus_widget_text::Text;

/// Surface treatment (handoff `ToolbarProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarVariant {
    /// Frosted bar with a bottom hairline.
    #[default]
    Panel,
    /// Transparent (over content).
    Transparent,
}

/// A top navigation / title bar.
#[derive(Debug, Clone, Default)]
pub struct Toolbar {
    title: Option<String>,
    subtitle: Option<String>,
    leading: Option<LayoutNode>,
    trailing: Option<LayoutNode>,
    center_title: bool,
    variant: ToolbarVariant,
    id: Option<&'static str>,
}

impl Toolbar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(trailing);
        self
    }
    /// Center the title (iOS nav-bar style).
    pub fn center_title(mut self, center: bool) -> Self {
        self.center_title = center;
        self
    }
    pub fn variant(mut self, variant: ToolbarVariant) -> Self {
        self.variant = variant;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn grow() -> LayoutNode {
        LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 1,
            min_size: Some(FxPx::new(8)),
            item: FlexItem::default(),
        })
    }

    fn title_block(&self, tokens: &dyn Tokens) -> Option<LayoutNode> {
        let title = self.title.clone()?;
        let mut col: Vec<LayoutNode> = Vec::new();
        col.push(
            Text::new(title).size(TypographyToken::Lg).weight(FontWeight::Semibold).build(tokens),
        );
        if let Some(subtitle) = self.subtitle.clone() {
            col.push(Text::caption(subtitle).build(tokens));
        }
        Some(LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: if self.center_title { Align::Center } else { Align::Start },
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            col,
        ))
    }

    /// Build the toolbar node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = match self.variant {
            ToolbarVariant::Panel => Style::new()
                .background(tokens.color(ColorToken::SurfaceVariant))
                .border(tokens.length(LengthToken::BorderThin), tokens.color(ColorToken::Border)),
            ToolbarVariant::Transparent => Style::new(),
        };

        let center = self.center_title;
        let title_block = self.title_block(tokens);
        let leading = self.leading;
        let trailing = self.trailing;

        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(leading) = leading {
            row.push(leading);
        }
        if center {
            row.push(Self::grow());
            if let Some(tb) = title_block {
                row.push(tb);
            }
            row.push(Self::grow());
        } else {
            if let Some(tb) = title_block {
                row.push(tb);
            }
            row.push(Self::grow());
        }
        if let Some(trailing) = trailing {
            row.push(trailing);
        }

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
                min_width: Some(FxPx::new(200)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            row,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::Spacer as Sp;
    use nexus_theme_tokens::BaseTokens;

    fn btn() -> LayoutNode {
        LayoutNode::Spacer(Sp {
            id: None,
            flex_grow: 0,
            min_size: Some(FxPx::new(24)),
            item: FlexItem::default(),
        })
    }

    #[test]
    fn panel_has_surface_bg_and_leading_title_layout() {
        let t = BaseTokens;
        match Toolbar::new().title("Einstellungen").trailing(btn()).id("bar").build(&t) {
            LayoutNode::Stack(stack, v, children) => {
                assert_eq!(stack.id, Some("bar"));
                assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)));
                // title-block + grow + trailing.
                assert_eq!(children.len(), 3);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn center_title_adds_symmetric_spacers_transparent_has_no_bg() {
        let t = BaseTokens;
        match Toolbar::new().title("Titel").center_title(true).leading(btn()).build(&t) {
            // leading + grow + title + grow (no trailing).
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 4),
            _ => panic!(),
        }
        match Toolbar::new().variant(ToolbarVariant::Transparent).title("x").build(&t) {
            LayoutNode::Stack(_, v, _) => assert_eq!(v.background, None),
            _ => panic!(),
        }
    }
}
