// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Toolbar` — the design artifacts' bar for panels and sheets (design `Toolbar`, RFC-0116;
//! never a window's title bar — window chrome is the window kit's). A 52 px row (0 16 padding,
//! 12 between its three parts): a leading cluster, a flexible title column (headline `label`,
//! an optional caption `labelSecondary` subtitle; leading or centred) and a trailing cluster,
//! the clusters 8 px between their items and at least 60 wide when the title is centred so it
//! stays in the middle. The `panel` variant is `glassCard` with a separator hairline under it;
//! `transparent` sits on the content. Pure builder producing a `LayoutNode::Stack` row.

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Border, Direction, EdgeBorder, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Stack,
    TextAlign, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_text::Text;

/// The bar's height (the xl control height).
pub const TOOLBAR_HEIGHT: i32 = 52;

/// Surface treatment (design `ToolbarProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarVariant {
    /// `glassCard` with a separator hairline along the bottom.
    #[default]
    Panel,
    /// Transparent (over content).
    Transparent,
}

/// A panel / sheet title bar.
#[derive(Debug, Clone, Default)]
pub struct Toolbar {
    title: Option<String>,
    subtitle: Option<String>,
    leading: Vec<LayoutNode>,
    trailing: Vec<LayoutNode>,
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
    /// Add an item to the leading cluster (each call appends; 8 px apart).
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading.push(leading);
        self
    }
    /// Add an item to the trailing cluster (each call appends; 8 px apart).
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing.push(trailing);
        self
    }
    /// Centre the title between the clusters.
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

    /// A cluster: its items 8 px apart, never shrunk, at least 60 wide around a centred title.
    fn cluster(items: Vec<LayoutNode>, end: bool, centred: bool) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: FxPx::new(8),
                align: Align::Center,
                justify: if end { Justify::End } else { Justify::Start },
                min_width: centred.then_some(FxPx::new(60)),
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            items,
        )
    }

    fn title_column(&self, tokens: &dyn Tokens) -> LayoutNode {
        let align = if self.center_title { TextAlign::Center } else { TextAlign::Left };
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(title) = self.title.clone() {
            col.push(
                Text::new(title).text_style(TextStyleToken::Headline).align(align).build(tokens),
            );
        }
        if let Some(subtitle) = self.subtitle.clone() {
            col.push(
                Text::new(subtitle)
                    .text_style(TextStyleToken::Caption)
                    .color(ColorToken::LabelSecondary)
                    .align(align)
                    .build(tokens),
            );
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                align: if self.center_title { Align::Center } else { Align::Start },
                justify: Justify::Center,
                min_width: Some(FxPx::ZERO),
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            col,
        )
    }

    /// Build the toolbar node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let visual = match self.variant {
            ToolbarVariant::Panel => VisualStyle {
                background: Some(tokens.color(ColorToken::GlassCard)),
                border: EdgeBorder {
                    bottom: Some(Border {
                        width: HAIRLINE,
                        color: tokens.color(ColorToken::Separator),
                    }),
                    ..EdgeBorder::none()
                },
                ..VisualStyle::default()
            },
            ToolbarVariant::Transparent => VisualStyle::default(),
        };
        let title = self.title_column(tokens);
        let centred = self.center_title;
        let h = Some(FxPx::new(TOOLBAR_HEIGHT));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(12),
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(16)),
                align: Align::Center,
                min_height: h,
                max_height: h,
                ..Stack::default()
            },
            visual,
            alloc::vec![
                Self::cluster(self.leading, false, centred),
                title,
                Self::cluster(self.trailing, true, centred),
            ],
        )
    }
}
