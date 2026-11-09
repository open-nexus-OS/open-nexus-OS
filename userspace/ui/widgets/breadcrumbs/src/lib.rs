// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Breadcrumbs` — the design artifacts' path trail (design `Breadcrumbs`, RFC-0116): a
//! wrapping row (4 px apart) of crumbs and 14 px `chevron-right` separators (stroke 2,
//! `labelTertiary`). A crumb is a 26 px capsule (4 8 padding) in subheadline: the earlier crumbs
//! medium `labelSecondary` with their own tap targets, the last — the current page — semibold
//! `label` and inert. Pure builder producing a `LayoutNode::Stack` row.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, LayoutNode, Stack,
    VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens};
use nexus_widget_icon::{Icon, LucideSymbol};
use nexus_widget_text::Text;

/// A breadcrumb trail.
#[derive(Debug, Clone, Default)]
pub struct Breadcrumbs {
    items: Vec<String>,
    ids: Vec<&'static str>,
    id: Option<&'static str>,
}

impl Breadcrumbs {
    pub fn new(items: Vec<String>) -> Self {
        Self { items, ids: Vec::new(), id: None }
    }

    /// The crumbs' node ids (their tap targets), by position; the last crumb is the current page
    /// and stays inert.
    pub fn ids(mut self, ids: Vec<&'static str>) -> Self {
        self.ids = ids;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn crumb(
        tokens: &dyn Tokens,
        label: String,
        current: bool,
        id: Option<&'static str>,
    ) -> LayoutNode {
        let (weight, ink) = if current {
            (FontWeight::Semibold, ColorToken::Label)
        } else {
            (FontWeight::Medium, ColorToken::LabelSecondary)
        };
        let text = Text::new(label)
            .text_style(TextStyleToken::Subheadline)
            .weight(weight)
            .color(ink)
            .build(tokens);
        LayoutNode::Stack(
            Stack {
                id: if current { None } else { id },
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::new(4), FxPx::new(8)),
                align: Align::Center,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![text],
        )
    }

    /// Build the breadcrumb row.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let last = self.items.len().saturating_sub(1);
        let mut row: Vec<LayoutNode> = Vec::new();
        for (i, crumb) in self.items.into_iter().enumerate() {
            if i > 0 {
                row.push(
                    Icon::lucide(LucideSymbol::ChevronRight)
                        .size(14)
                        .color(ColorToken::LabelTertiary)
                        .build(tokens),
                );
            }
            row.push(Self::crumb(tokens, crumb, i == last, self.ids.get(i).copied()));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(4),
                align: Align::Center,
                flex_wrap: true,
                ..Stack::default()
            },
            VisualStyle::default(),
            row,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::ShapeKind;
    use nexus_text_baked::measure_text::BakedTextMeasure;
    use nexus_theme_tokens::BaseTokens;

    fn trail() -> Breadcrumbs {
        Breadcrumbs::new(alloc::vec![
            String::from("Home"),
            String::from("Dokumente"),
            String::from("Bericht.pdf")
        ])
        .ids(alloc::vec!["home", "docs", "file"])
        .id("path")
    }

    fn text_of(node: &LayoutNode) -> &nexus_layout_types::TextNode {
        match node {
            LayoutNode::Stack(_, _, c) => match &c[0] {
                LayoutNode::Text(n, _) => n,
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn crumbs_interleave_with_chevrons_and_the_last_is_current() {
        let t = BaseTokens;
        match trail().build(&t) {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("path"));
                assert_eq!((stack.gap, stack.flex_wrap), (FxPx::new(4), true));
                assert_eq!(stack.padding, EdgeInsets::zero());
                assert_eq!(children.len(), 5, "3 crumbs + 2 separators");
                let first = text_of(&children[0]);
                assert_eq!(first.style.color, t.color(ColorToken::LabelSecondary));
                assert_eq!(
                    (first.style.font_size.0, first.style.font_weight),
                    (13, FontWeight::Medium)
                );
                let last = text_of(&children[4]);
                assert_eq!(last.style.color, t.color(ColorToken::Label));
                assert_eq!(last.style.font_weight, FontWeight::Semibold);
                match &children[1] {
                    LayoutNode::Stack(s, v, _) => {
                        assert_eq!(s.min_width, Some(FxPx::new(14)));
                        assert_eq!(v.background, Some(t.color(ColorToken::LabelTertiary)));
                        assert!(matches!(v.shape, ShapeKind::Stroke { .. }));
                    }
                    _ => panic!(),
                }
                // Earlier crumbs are tap targets; the current page is inert.
                let id = |n: &LayoutNode| match n {
                    LayoutNode::Stack(s, _, _) => s.id,
                    _ => None,
                };
                assert_eq!(
                    (id(&children[0]), id(&children[2]), id(&children[4])),
                    (Some("home"), Some("docs"), None)
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn a_crumb_is_a_26_px_capsule() {
        let t = BaseTokens;
        let all = LayoutEngine::new()
            .layout(&trail().build(&t), FxPx::new(600), &BakedTextMeasure)
            .expect("layout")
            .boxes;
        let home = all.iter().find(|b| b.id == Some("home")).expect("crumb");
        assert_eq!(home.rect.height, FxPx::new(26));
        assert_eq!(home.visual.corner_radius.top_left, RadiusToken::Full.px());
    }
}
