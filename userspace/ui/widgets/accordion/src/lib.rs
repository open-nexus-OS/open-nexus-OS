// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Accordion` — the design-system disclosure group (handoff `Accordion`): a
//! glass container of collapsible sections; each header shows a chevron
//! (▲ open · ▼ closed via the built-in triangle shapes) and reveals its content
//! when open. A pure builder producing a `LayoutNode::Stack` column (the
//! open/close animation is a runtime concern). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Overflow, ShapeKind, Spacer, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_widget_text::Text;

/// One collapsible section.
#[derive(Debug, Clone)]
pub struct AccordionItem {
    title: String,
    content: LayoutNode,
    open: bool,
}

impl AccordionItem {
    pub fn new(title: impl Into<String>, content: LayoutNode) -> Self {
        Self { title: title.into(), content, open: false }
    }
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }
}

/// A disclosure group.
#[derive(Debug, Clone, Default)]
pub struct Accordion {
    items: Vec<AccordionItem>,
    id: Option<&'static str>,
}

impl Accordion {
    pub fn new(items: Vec<AccordionItem>) -> Self {
        Self { items, id: None }
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn chevron(tokens: &dyn Tokens, open: bool) -> LayoutNode {
        let visual = VisualStyle {
            background: Some(tokens.color(ColorToken::OnSurfaceVariant)),
            shape: if open { ShapeKind::TriangleUp } else { ShapeKind::TriangleDown },
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

    fn section(&self, tokens: &dyn Tokens, item: AccordionItem) -> LayoutNode {
        let header = LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: EdgeInsets::symmetric(FxPx::new(10), FxPx::new(12)),
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
            alloc::vec![
                Text::new(item.title).weight(FontWeight::Medium).build(tokens),
                LayoutNode::Spacer(Spacer {
                    id: None,
                    flex_grow: 1,
                    min_size: Some(FxPx::new(8)),
                    item: FlexItem::default(),
                }),
                Self::chevron(tokens, item.open),
            ],
        );

        let mut children = alloc::vec![header];
        if item.open {
            children.push(LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Column,
                    gap: FxPx::ZERO,
                    padding: EdgeInsets::symmetric(FxPx::new(4), FxPx::new(12)),
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
                alloc::vec![item.content],
            ));
        }
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::ZERO,
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
            children,
        )
    }

    /// Build the accordion node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = Style::new()
            .background(tokens.color(ColorToken::Surface))
            .rounded(tokens.length(LengthToken::RadiusMedium))
            .border(tokens.length(LengthToken::BorderThin), tokens.color(ColorToken::Border));

        let mut sections: Vec<LayoutNode> = Vec::with_capacity(self.items.len());
        for item in self.items.clone() {
            sections.push(self.section(tokens, item));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: Some(FxPx::new(220)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            sections,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::Spacer as Sp;
    use nexus_theme_tokens::BaseTokens;

    fn body() -> LayoutNode {
        LayoutNode::Spacer(Sp {
            id: None,
            flex_grow: 0,
            min_size: Some(FxPx::new(20)),
            item: FlexItem::default(),
        })
    }

    #[test]
    fn open_section_reveals_content_closed_hides_it() {
        let t = BaseTokens;
        let acc = Accordion::new(alloc::vec![
            AccordionItem::new("Allgemein", body()).open(true),
            AccordionItem::new("Datenschutz", body()),
        ])
        .id("acc")
        .build(&t);
        match acc {
            LayoutNode::Stack(stack, v, sections) => {
                assert_eq!(stack.id, Some("acc"));
                assert_eq!(v.background, Some(t.color(ColorToken::Surface)));
                assert_eq!(sections.len(), 2);
                // open section = header + content (2); closed = header only (1).
                let kids = |n: &LayoutNode| match n {
                    LayoutNode::Stack(_, _, c) => c.len(),
                    _ => 0,
                };
                assert_eq!(kids(&sections[0]), 2, "open reveals content");
                assert_eq!(kids(&sections[1]), 1, "closed is header only");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn chevron_flips_with_open_state() {
        let t = BaseTokens;
        match Accordion::chevron(&t, true) {
            LayoutNode::Stack(_, v, _) => assert_eq!(v.shape, ShapeKind::TriangleUp),
            _ => panic!(),
        }
        match Accordion::chevron(&t, false) {
            LayoutNode::Stack(_, v, _) => assert_eq!(v.shape, ShapeKind::TriangleDown),
            _ => panic!(),
        }
    }
}
