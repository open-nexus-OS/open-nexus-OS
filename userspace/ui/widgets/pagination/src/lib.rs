// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Pagination` — the design-system page navigation (handoff `Pagination`):
//! prev/next affordances plus numbered page pills with ellipsis truncation for
//! large ranges (the current page tints accent). A pure builder producing a
//! `LayoutNode::Stack` row. `visible_pages` is the deterministic truncation.
//! DSL-emittable.

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens};
use nexus_widget_text::Text;

/// A page navigator (1-based).
#[derive(Debug, Clone)]
pub struct Pagination {
    count: u32,
    page: u32,
    sibling: u32,
    id: Option<&'static str>,
}

impl Default for Pagination {
    fn default() -> Self {
        Self { count: 1, page: 1, sibling: 1, id: None }
    }
}

impl Pagination {
    pub fn new(count: u32) -> Self {
        Self { count: count.max(1), ..Self::default() }
    }

    pub fn page(mut self, page: u32) -> Self {
        self.page = page.clamp(1, self.count);
        self
    }
    /// Pages shown on each side of the current page before truncating.
    pub fn sibling(mut self, sibling: u32) -> Self {
        self.sibling = sibling;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The visible page slots: `Some(n)` = a page, `None` = an ellipsis gap.
    /// Deterministic: full list up to 7 pages, else first/last + a window.
    pub fn visible_pages(&self) -> Vec<Option<u32>> {
        let (count, page, sib) = (self.count, self.page, self.sibling);
        let mut out = Vec::new();
        if count <= 7 {
            for p in 1..=count {
                out.push(Some(p));
            }
            return out;
        }
        let lo = page.saturating_sub(sib).max(2);
        let hi = (page + sib).min(count - 1);
        out.push(Some(1));
        if lo > 2 {
            out.push(None);
        }
        for p in lo..=hi {
            out.push(Some(p));
        }
        if hi < count - 1 {
            out.push(None);
        }
        out.push(Some(count));
        out
    }

    fn pill(tokens: &dyn Tokens, node: LayoutNode, current: bool) -> LayoutNode {
        let mut style = Style::new().rounded(tokens.length(LengthToken::RadiusSmall));
        if current {
            style = style.background(tokens.color(ColorToken::Accent));
        }
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::symmetric(FxPx::new(4), FxPx::new(8)),
                align: Align::Center,
                justify: Justify::Center,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(24)),
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            alloc::vec![node],
        )
    }

    /// Build the pagination row.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let page = self.page;
        let mut row: Vec<LayoutNode> = Vec::new();
        // Prev.
        row.push(Self::pill(
            tokens,
            Text::new("‹").color(ColorToken::OnSurface).build(tokens),
            false,
        ));
        // Page pills.
        for slot in self.visible_pages() {
            match slot {
                Some(n) => {
                    let current = n == page;
                    let color = if current { ColorToken::OnAccent } else { ColorToken::OnSurface };
                    let label = Text::new(format!("{n}")).color(color).build(tokens);
                    row.push(Self::pill(tokens, label, current));
                }
                None => {
                    row.push(Text::new("…").color(ColorToken::OnSurfaceVariant).build(tokens));
                }
            }
        }
        // Next.
        row.push(Self::pill(
            tokens,
            Text::new("›").color(ColorToken::OnSurface).build(tokens),
            false,
        ));

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(4),
                padding: EdgeInsets::zero(),
                align: Align::Center,
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
            row,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn small_ranges_show_every_page() {
        let p = Pagination::new(5).page(3);
        assert_eq!(p.visible_pages(), alloc::vec![Some(1), Some(2), Some(3), Some(4), Some(5)]);
    }

    #[test]
    fn large_ranges_truncate_with_ellipsis() {
        let p = Pagination::new(20).page(10).sibling(1);
        // 1 … 9 10 11 … 20
        assert_eq!(
            p.visible_pages(),
            alloc::vec![Some(1), None, Some(9), Some(10), Some(11), None, Some(20)]
        );
    }

    #[test]
    fn builds_prev_pages_next_with_current_highlighted() {
        let t = BaseTokens;
        match Pagination::new(5).page(2).id("pg").build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("pg"));
                // prev + 5 pages + next.
                assert_eq!(children.len(), 7);
                // page 2 pill (index 2) is accent-filled.
                match &children[2] {
                    LayoutNode::Stack(_, v, _) => {
                        assert_eq!(v.background, Some(t.color(ColorToken::Accent)))
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }
}
