// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `TabBar` — the design-system bottom tab bar (handoff `TabBar`): a frosted
//! glass row of icon+label tabs; the active tab tints accent. A pure builder
//! producing a `LayoutNode::Stack`. Tab icons are caller nodes; labels are
//! owned. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};
use nexus_widget_text::Text;

/// One tab (icon is a caller node; label is owned).
#[derive(Debug, Clone, Default)]
pub struct TabItem {
    pub label: String,
    pub icon: Option<LayoutNode>,
}

impl TabItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), icon: None }
    }
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// A bottom tab bar.
#[derive(Debug, Clone, Default)]
pub struct TabBar {
    tabs: Vec<TabItem>,
    active: usize,
    floating: bool,
    id: Option<&'static str>,
}

impl TabBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tabs(mut self, tabs: Vec<TabItem>) -> Self {
        self.tabs = tabs;
        self
    }
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }
    /// Floating centered pill (true) vs full-width bar (false).
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn slot(tokens: &dyn Tokens, index: usize, active: usize, tab: TabItem) -> LayoutNode {
        let color = if index == active { ColorToken::Accent } else { ColorToken::OnSurfaceVariant };
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = tab.icon {
            col.push(icon);
        }
        col.push(Text::new(tab.label).size(TypographyToken::Xs).color(color).build(tokens));
        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::new(2),
                padding: EdgeInsets::symmetric(FxPx::new(6), FxPx::new(12)),
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
            col,
        )
    }

    /// Build the tab-bar node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let radius =
            if self.floating { tokens.length(LengthToken::RadiusLarge) } else { FxPx::ZERO };
        let style =
            Style::new().background(tokens.color(ColorToken::SurfaceVariant)).rounded(radius);

        let active = self.active;
        let mut slots: Vec<LayoutNode> = Vec::with_capacity(self.tabs.len());
        for (i, tab) in self.tabs.into_iter().enumerate() {
            slots.push(Self::slot(tokens, i, active, tab));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(4),
                padding: EdgeInsets::all(FxPx::new(4)),
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
            style.visual(),
            slots,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn active_tab_label_is_accent_others_muted() {
        let t = BaseTokens;
        let bar = TabBar::new()
            .active(1)
            .tabs(alloc::vec![
                TabItem::new("Start"),
                TabItem::new("Nachrichten"),
                TabItem::new("Profil")
            ])
            .build(&t);
        let label_color = |slot: &LayoutNode| match slot {
            LayoutNode::Stack(_, _, col) => match col.last().unwrap() {
                LayoutNode::Text(n, _) => n.style.color,
                _ => panic!(),
            },
            _ => panic!(),
        };
        match bar {
            LayoutNode::Stack(_, _, slots) => {
                assert_eq!(slots.len(), 3);
                assert_eq!(label_color(&slots[1]), t.color(ColorToken::Accent));
                assert_eq!(label_color(&slots[0]), t.color(ColorToken::OnSurfaceVariant));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn floating_rounds_the_bar() {
        let t = BaseTokens;
        match TabBar::new().floating(true).tabs(alloc::vec![TabItem::new("A")]).build(&t) {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(v.corner_radius.top_left, t.length(LengthToken::RadiusLarge))
            }
            _ => panic!(),
        }
    }
}
