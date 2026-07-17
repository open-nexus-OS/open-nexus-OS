// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Breadcrumbs` — the design-system path trail (handoff `Breadcrumbs`):
//! chevron-separated crumbs with the current (last) page bold and
//! non-interactive, earlier crumbs in accent. A pure builder producing a
//! `LayoutNode::Stack` row. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow, Stack,
    VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens};
use nexus_widget_text::Text;

/// A breadcrumb trail.
#[derive(Debug, Clone, Default)]
pub struct Breadcrumbs {
    items: Vec<String>,
    id: Option<&'static str>,
}

impl Breadcrumbs {
    pub fn new(items: Vec<String>) -> Self {
        Self { items, id: None }
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Build the breadcrumb row.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let last = self.items.len().saturating_sub(1);
        let mut row: Vec<LayoutNode> = Vec::new();
        for (i, crumb) in self.items.into_iter().enumerate() {
            if i > 0 {
                row.push(Text::new("›").color(ColorToken::OnSurfaceVariant).build(tokens));
            }
            let text = if i == last {
                Text::new(crumb).weight(FontWeight::Semibold).color(ColorToken::OnSurface)
            } else {
                Text::new(crumb).color(ColorToken::Accent)
            };
            row.push(text.build(tokens));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(6),
                padding: EdgeInsets::symmetric(FxPx::new(2), FxPx::new(2)),
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
            row,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn interleaves_separators_and_bolds_the_last() {
        let t = BaseTokens;
        let items = alloc::vec![
            String::from("Home"),
            String::from("Dokumente"),
            String::from("Bericht.pdf")
        ];
        match Breadcrumbs::new(items).id("path").build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("path"));
                // 3 crumbs + 2 separators.
                assert_eq!(children.len(), 5);
                // last crumb is semibold + OnSurface.
                match children.last().unwrap() {
                    LayoutNode::Text(n, _) => {
                        assert_eq!(n.style.font_weight, FontWeight::Semibold);
                        assert_eq!(n.style.color, t.color(ColorToken::OnSurface));
                    }
                    _ => panic!(),
                }
                // first crumb is accent (a link).
                match &children[0] {
                    LayoutNode::Text(n, _) => {
                        assert_eq!(n.style.color, t.color(ColorToken::Accent))
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }
}
