// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `SubHeader` — the design artifacts' section header (design `SubHeader`, RFC-0116): a row
//! (4 4 8 padding, 8 gap) of an optional `labelSecondary` icon, a title column — the title as an
//! overline (11/14 semibold, +0.06 em, capitals, `labelSecondary`), an optional footnote line
//! under it in `labelSecondary` at 80 % — and an optional trailing action: a subheadline
//! semibold `accent` text button (4 8 padding, capsule) with its own tap target. Place above
//! lists, grids and settings groups. Pure builder producing a `LayoutNode::Stack`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, LayoutNode, Rgba8,
    Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens};
use nexus_widget_icon::with_ink;
use nexus_widget_text::Text;

/// A section header.
#[derive(Debug, Clone, Default)]
pub struct SubHeader {
    title: String,
    secondary: Option<String>,
    action: Option<String>,
    action_id: Option<&'static str>,
    icon: Option<LayoutNode>,
    id: Option<&'static str>,
}

impl SubHeader {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Self::default() }
    }

    /// A footnote line under the title.
    pub fn secondary(mut self, secondary: impl Into<String>) -> Self {
        self.secondary = Some(secondary.into());
        self
    }
    /// The trailing text button's label (`accent`).
    pub fn action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }
    /// The trailing text button's node id (its tap target).
    pub fn action_id(mut self, id: &'static str) -> Self {
        self.action_id = Some(id);
        self
    }
    /// A leading icon; the header inks it `labelSecondary`.
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn column(children: Vec<LayoutNode>) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                min_width: Some(FxPx::ZERO),
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            children,
        )
    }

    /// Build the sub-header node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let secondary_ink = tokens.color(ColorToken::LabelSecondary);
        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = self.icon {
            row.push(with_ink(icon, secondary_ink));
        }
        let mut col = alloc::vec![Text::overline(self.title).build(tokens)];
        if let Some(secondary) = self.secondary {
            let faded = Rgba8 { a: (u32::from(secondary_ink.a) * 80 / 100) as u8, ..secondary_ink };
            let mut text = Text::new(secondary).text_style(TextStyleToken::Footnote).build(tokens);
            if let LayoutNode::Text(t, _) = &mut text {
                t.style.color = faded;
            }
            col.push(text);
        }
        row.push(Self::column(col));
        if let Some(action) = self.action {
            let label = Text::new(action)
                .text_style(TextStyleToken::Subheadline)
                .weight(FontWeight::Semibold)
                .color(ColorToken::Accent)
                .build(tokens);
            row.push(LayoutNode::Stack(
                Stack {
                    id: self.action_id,
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
                alloc::vec![label],
            ));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: EdgeInsets {
                    top: FxPx::new(4),
                    right: FxPx::new(4),
                    bottom: FxPx::new(8),
                    left: FxPx::new(4),
                },
                align: Align::Center,
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
    use nexus_text_baked::measure_text::BakedTextMeasure;
    use nexus_theme_tokens::BaseTokens;

    fn height(node: &LayoutNode) -> i32 {
        let all =
            LayoutEngine::new().layout(node, FxPx::new(360), &BakedTextMeasure).expect("layout");
        all.boxes[0].rect.height.0
    }

    #[test]
    fn the_title_is_an_overline_in_the_secondary_label() {
        let t = BaseTokens;
        let node = SubHeader::new("Allgemein").id("sec").build(&t);
        match &node {
            LayoutNode::Stack(stack, _, children) => {
                assert_eq!(stack.id, Some("sec"));
                assert_eq!(stack.padding.bottom, FxPx::new(8));
                match &children[0] {
                    LayoutNode::Stack(_, _, col) => match &col[0] {
                        LayoutNode::Text(n, _) => {
                            assert_eq!(n.content.as_str(), "ALLGEMEIN");
                            assert_eq!(n.style.color, t.color(ColorToken::LabelSecondary));
                            assert_eq!(n.style.tracking_milli_em, 60);
                        }
                        _ => panic!(),
                    },
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
        // 4 + overline 14 + 8.
        assert_eq!(height(&node), 26);
    }

    #[test]
    fn secondary_sits_in_the_title_column_at_80_percent() {
        let t = BaseTokens;
        let node = SubHeader::new("Geräte").secondary("3 verbunden").build(&t);
        let LayoutNode::Stack(_, _, row) = &node else { panic!() };
        assert_eq!(row.len(), 1, "one title column");
        let LayoutNode::Stack(_, _, col) = &row[0] else { panic!() };
        match &col[1] {
            LayoutNode::Text(n, _) => {
                let ink = t.color(ColorToken::LabelSecondary);
                assert_eq!(n.style.font_size, FxPx::new(12));
                assert_eq!(n.style.color.a, (u32::from(ink.a) * 80 / 100) as u8);
            }
            _ => panic!(),
        }
        assert_eq!(height(&node), 4 + 14 + 16 + 8);
    }

    #[test]
    fn the_action_is_an_accent_capsule_with_its_own_target() {
        let t = BaseTokens;
        let node = SubHeader::new("Geräte").action("Alle").action_id("all").build(&t);
        let LayoutNode::Stack(_, _, row) = &node else { panic!() };
        assert_eq!(row.len(), 2, "title column + action");
        match &row[1] {
            LayoutNode::Stack(s, v, kids) => {
                assert_eq!(s.id, Some("all"));
                assert_eq!(s.padding, EdgeInsets::symmetric(FxPx::new(4), FxPx::new(8)));
                assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
                match &kids[0] {
                    LayoutNode::Text(n, _) => {
                        assert_eq!(n.style.color, t.color(ColorToken::Accent));
                        assert_eq!(
                            (n.style.font_size.0, n.style.font_weight),
                            (13, FontWeight::Semibold)
                        );
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
        // The 26 px button sets the line: 4 + 26 + 8.
        assert_eq!(height(&node), 38);
    }
}
