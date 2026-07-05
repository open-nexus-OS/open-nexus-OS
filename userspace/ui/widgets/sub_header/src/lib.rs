// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `SubHeader` — the design-system section header (handoff `SubHeader`): a small
//! muted title with an optional caption and a trailing text action; place above
//! grouped lists. A pure builder producing a `LayoutNode::Stack`. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens, TypographyToken};
use nexus_widget_text::Text;

/// A grouped-list section header.
#[derive(Debug, Clone, Default)]
pub struct SubHeader {
    title: String,
    secondary: Option<String>,
    action: Option<String>,
    icon: Option<LayoutNode>,
    id: Option<&'static str>,
}

impl SubHeader {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    /// Caption under the title.
    pub fn secondary(mut self, secondary: impl Into<String>) -> Self {
        self.secondary = Some(secondary.into());
        self
    }
    /// Trailing text-button label (accent).
    pub fn action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn row(children: Vec<LayoutNode>, id: Option<&'static str>) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: EdgeInsets::symmetric(FxPx::new(4), FxPx::new(4)),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            children,
        )
    }

    /// Build the sub-header node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut header: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = self.icon {
            header.push(icon);
        }
        header.push(
            Text::new(self.title)
                .size(TypographyToken::Sm)
                .weight(FontWeight::Semibold)
                .color(ColorToken::OnSurfaceVariant)
                .build(tokens),
        );
        if let Some(action) = self.action {
            header.push(LayoutNode::Spacer(Spacer {
                id: None,
                flex_grow: 1,
                min_size: Some(FxPx::new(8)),
                item: FlexItem::default(),
            }));
            header.push(
                Text::new(action).size(TypographyToken::Sm).color(ColorToken::Accent).build(tokens),
            );
        }

        let header_row = Self::row(header, self.id);
        match self.secondary {
            None => header_row,
            Some(secondary) => LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Column,
                    gap: FxPx::new(2),
                    padding: EdgeInsets::zero(),
                    align: Align::Start,
                    justify: Justify::Start,
                    overflow: Overflow::Visible,
                    flex_wrap: false,
                    min_width: None,
                    max_width: None,
                    min_height: None,
                    max_height: None,
                    item: FlexItem::default(),
                },
                VisualStyle::default(),
                alloc::vec![header_row, Text::caption(secondary).build(tokens)],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn title_only_is_a_single_row() {
        let t = BaseTokens;
        match SubHeader::new("Allgemein").id("sec").build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("sec"));
                assert_eq!(children.len(), 1, "just the title");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn action_adds_spacer_and_accent_label_secondary_wraps_in_column() {
        let t = BaseTokens;
        match SubHeader::new("Geräte").action("Alle").build(&t) {
            LayoutNode::Stack(_, _, children) => assert_eq!(children.len(), 3, "title + spacer + action"),
            _ => panic!(),
        }
        match SubHeader::new("Geräte").secondary("3 verbunden").build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.direction, Direction::Column);
                assert_eq!(children.len(), 2, "header row + caption");
            }
            _ => panic!(),
        }
    }
}
