// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Sidebar` / `SplitView` — the design-system navigation rail (handoff
//! `Sidebar`/`SplitView`): a vertical list of nav items with an active accent
//! highlight, optional section headers, and header/footer slots; `SplitView`
//! pairs it with a flexible content pane. Pure builders producing
//! `LayoutNode::Stack`s. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};
use nexus_widget_badge::{Badge, BadgeVariant};
use nexus_widget_text::Text;

/// One sidebar entry (a nav item, or a non-interactive section header).
#[derive(Debug, Clone, Default)]
pub struct SidebarItem {
    value: Option<&'static str>,
    label: String,
    icon: Option<LayoutNode>,
    badge: Option<u32>,
    header: bool,
}

impl SidebarItem {
    /// A nav item selectable by `value`.
    pub fn item(value: &'static str, label: impl Into<String>) -> Self {
        Self { value: Some(value), label: label.into(), ..Self::default() }
    }
    /// A non-interactive section header.
    pub fn header(label: impl Into<String>) -> Self {
        Self { label: label.into(), header: true, ..Self::default() }
    }
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn badge(mut self, badge: u32) -> Self {
        self.badge = Some(badge);
        self
    }
}

/// A navigation rail.
#[derive(Debug, Clone, Default)]
pub struct Sidebar {
    items: Vec<SidebarItem>,
    active: Option<&'static str>,
    header: Option<LayoutNode>,
    footer: Option<LayoutNode>,
    plain: bool,
    width: Option<i32>,
    id: Option<&'static str>,
}

impl Sidebar {
    pub fn new(items: Vec<SidebarItem>) -> Self {
        Self { items, ..Self::default() }
    }
    pub fn active(mut self, value: &'static str) -> Self {
        self.active = Some(value);
        self
    }
    pub fn header(mut self, header: LayoutNode) -> Self {
        self.header = Some(header);
        self
    }
    pub fn footer(mut self, footer: LayoutNode) -> Self {
        self.footer = Some(footer);
        self
    }
    /// Transparent (no surface/border) — for embedding in a window that has its own surface.
    pub fn plain(mut self, plain: bool) -> Self {
        self.plain = plain;
        self
    }
    pub fn width(mut self, width: i32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn row(&self, tokens: &dyn Tokens, item: SidebarItem) -> LayoutNode {
        if item.header {
            return LayoutNode::Stack(
                Stack {
                    id: None,
                    direction: Direction::Row,
                    gap: FxPx::ZERO,
                    padding: EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12)),
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
                alloc::vec![Text::new(item.label)
                    .size(TypographyToken::Xs)
                    .weight(FontWeight::Semibold)
                    .color(ColorToken::OnSurfaceVariant)
                    .build(tokens)],
            );
        }

        let active = item.value.is_some() && item.value == self.active;
        let mut style = Style::new().rounded(tokens.length(LengthToken::RadiusMedium));
        if active {
            style = style.background(tokens.color(ColorToken::SurfaceVariant));
        }
        let label_color = if active { ColorToken::Accent } else { ColorToken::OnSurface };

        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = item.icon {
            row.push(icon);
        }
        row.push(Text::new(item.label).color(label_color).build(tokens));
        row.push(LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 1,
            min_size: Some(FxPx::new(8)),
            item: FlexItem::default(),
        }));
        if let Some(count) = item.badge {
            let text = Text::new(alloc::format!("{count}")).color(ColorToken::OnAccent).build(tokens);
            row.push(Badge::new().variant(BadgeVariant::Active).content(text).build(tokens));
        }

        LayoutNode::Stack(
            Stack {
                id: item.value,
                direction: Direction::Row,
                gap: FxPx::new(10),
                padding: EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12)),
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
            style.visual(),
            row,
        )
    }

    /// Build the sidebar node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let style = if self.plain {
            Style::new()
        } else {
            Style::new()
                .background(tokens.color(ColorToken::SurfaceVariant))
                .border(tokens.length(LengthToken::BorderThin), tokens.color(ColorToken::Border))
        };
        let width = self.width.map(FxPx::new);

        let header = self.header.clone();
        let footer = self.footer.clone();
        let id = self.id;
        let items = self.items.clone();

        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(header) = header {
            col.push(header);
        }
        for item in items {
            col.push(self.row(tokens, item));
        }
        col.push(LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 1,
            min_size: Some(FxPx::new(8)),
            item: FlexItem::default(),
        }));
        if let Some(footer) = footer {
            col.push(footer);
        }

        LayoutNode::Stack(
            Stack {
                id,
                direction: Direction::Column,
                gap: FxPx::new(2),
                padding: EdgeInsets::all(FxPx::new(8)),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: width,
                max_width: width,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            style.visual(),
            col,
        )
    }
}

/// A two-pane sidebar + content layout.
#[derive(Debug, Clone)]
pub struct SplitView {
    sidebar: LayoutNode,
    content: LayoutNode,
    id: Option<&'static str>,
}

impl SplitView {
    pub fn new(sidebar: LayoutNode, content: LayoutNode) -> Self {
        Self { sidebar, content, id: None }
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Build the split-view row (sidebar + flexible content pane).
    pub fn build(self) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::ZERO,
                padding: EdgeInsets::zero(),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Hidden,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            alloc::vec![self.sidebar, self.content],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn active_item_highlights_and_tints_accent() {
        let t = BaseTokens;
        let bar = Sidebar::new(alloc::vec![
            SidebarItem::header("Bibliothek"),
            SidebarItem::item("all", "Alle Dateien"),
            SidebarItem::item("shared", "Geteilt").badge(2),
        ])
        .active("all")
        .build(&t);
        match bar {
            LayoutNode::Stack(_, v, col) => {
                assert_eq!(v.background, Some(t.color(ColorToken::SurfaceVariant)));
                // header + 2 items + spacer = 4.
                assert_eq!(col.len(), 4);
                // active item ("all") has the highlight bg + carries its value id.
                match &col[1] {
                    LayoutNode::Stack(s, iv, _) => {
                        assert_eq!(s.id, Some("all"));
                        assert_eq!(iv.background, Some(t.color(ColorToken::SurfaceVariant)));
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }

    #[test]
    fn plain_sidebar_has_no_surface_and_splitview_pairs_panes() {
        let t = BaseTokens;
        match Sidebar::new(alloc::vec![SidebarItem::item("a", "A")]).plain(true).build(&t) {
            LayoutNode::Stack(_, v, _) => assert_eq!(v.background, None),
            _ => panic!(),
        }
        let sv = SplitView::new(
            Sidebar::new(alloc::vec![SidebarItem::item("a", "A")]).build(&t),
            LayoutNode::Spacer(Spacer { id: None, flex_grow: 1, min_size: None, item: FlexItem::default() }),
        )
        .build();
        match sv {
            LayoutNode::Stack(_, _, panes) => assert_eq!(panes.len(), 2),
            _ => panic!(),
        }
    }
}
