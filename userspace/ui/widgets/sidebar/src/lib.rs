// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Sidebar` / `SplitView` — the design artifacts' navigation rail (design `Sidebar`/
//! `SplitView`, RFC-0116). A 248 px column: an optional header slot (12 16 4), the item list
//! (4 8 padding, 2 px apart) and an optional footer slot (12, under a hairline). An item is a
//! 36 px row (8 12 padding, 12 gap, radius 12): body medium `label` with its 16 px icon at 85 %;
//! the active item is the accent pill — `onAccent` semibold with the accent glow under it. A
//! section header is an overline in `labelSecondary` (12 12 4 → 30 px). The `panel` look is
//! `glassCard` with a hairline on its trailing edge; `plain` sits on the surface it is placed
//! on. `SplitView` pairs the rail with a flexible, scrolling content pane at full height.
//! Pure builders producing `LayoutNode::Stack`s.

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Border, BoxShadow, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem,
    FontWeight, FxPx, Justify, LayoutNode, Overflow, Rgba8, ScrollAxis, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_icon::with_ink;
use nexus_widget_text::Text;

/// The rail's default width.
pub const SIDEBAR_WIDTH: i32 = 248;

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
    /// A nav item selectable by `value` (its node id).
    pub fn item(value: &'static str, label: impl Into<String>) -> Self {
        Self { value: Some(value), label: label.into(), ..Self::default() }
    }
    /// A non-interactive section header (an overline).
    pub fn header(label: impl Into<String>) -> Self {
        Self { label: label.into(), header: true, ..Self::default() }
    }
    /// The 16 px icon; the item inks it (`label` at 85 %, `onAccent` when active).
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

/// `color` at `pct` % of its own alpha.
fn fade(color: Rgba8, pct: u32) -> Rgba8 {
    Rgba8 { a: (u32::from(color.a) * pct / 100) as u8, ..color }
}

/// A slot: a column with `padding` that stretches what it holds.
fn slot(padding: EdgeInsets) -> Stack {
    Stack { direction: Direction::Column, padding, align: Align::Stretch, ..Stack::default() }
}

impl Sidebar {
    pub fn new(items: Vec<SidebarItem>) -> Self {
        Self { items, ..Self::default() }
    }
    pub fn active(mut self, value: &'static str) -> Self {
        self.active = Some(value);
        self
    }
    /// The header slot (padding 12 16 4).
    pub fn header(mut self, header: LayoutNode) -> Self {
        self.header = Some(header);
        self
    }
    /// The footer slot (padding 12, under a hairline) — e.g. the account row.
    pub fn footer(mut self, footer: LayoutNode) -> Self {
        self.footer = Some(footer);
        self
    }
    /// Transparent (no surface, no edge) — the rail sits directly on the window glass.
    pub fn plain(mut self, plain: bool) -> Self {
        self.plain = plain;
        self
    }
    /// The rail width (default [`SIDEBAR_WIDTH`]).
    pub fn width(mut self, width: i32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn section_header(tokens: &dyn Tokens, label: String) -> LayoutNode {
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                padding: EdgeInsets {
                    top: FxPx::new(12),
                    right: FxPx::new(12),
                    bottom: FxPx::new(4),
                    left: FxPx::new(12),
                },
                align: Align::Center,
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![Text::overline(label).build(tokens)],
        )
    }

    /// The count badge: 18 px capsule (min 18, 0 4 padding), caption bold `onAccent` on `danger`
    /// — on the active pill a 16 % wash of `onAccent` instead.
    fn badge(tokens: &dyn Tokens, count: u32, active: bool) -> LayoutNode {
        let on = tokens.color(ColorToken::OnAccent);
        let bg = if active { fade(on, 16) } else { tokens.color(ColorToken::Danger) };
        let text = Text::new(alloc::format!("{count}"))
            .text_style(TextStyleToken::Caption)
            .weight(FontWeight::Bold)
            .color(ColorToken::OnAccent)
            .build(tokens);
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(4)),
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(FxPx::new(18)),
                min_height: Some(FxPx::new(18)),
                max_height: Some(FxPx::new(18)),
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(bg),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![text],
        )
    }

    fn row(&self, tokens: &dyn Tokens, item: SidebarItem) -> LayoutNode {
        if item.header {
            return Self::section_header(tokens, item.label);
        }
        let active = item.value.is_some() && item.value == self.active;
        let (ink, weight) = if active {
            (ColorToken::OnAccent, FontWeight::Semibold)
        } else {
            (ColorToken::Label, FontWeight::Medium)
        };

        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = item.icon {
            let ink = tokens.color(ink);
            row.push(with_ink(icon, if active { ink } else { fade(ink, 85) }));
        }
        let mut label = Text::new(item.label).weight(weight).color(ink).build(tokens);
        if let LayoutNode::Text(t, _) = &mut label {
            t.item.flex_grow = 1;
        }
        row.push(label);
        if let Some(count) = item.badge {
            row.push(Self::badge(tokens, count, active));
        }

        let visual = if active {
            VisualStyle {
                background: Some(tokens.color(ColorToken::Accent)),
                corner_radius: CornerRadius::uniform(RadiusToken::Md.px()),
                shadow: Some(BoxShadow {
                    offset_x: FxPx::ZERO,
                    offset_y: FxPx::new(2),
                    blur_radius: FxPx::new(8),
                    spread: FxPx::ZERO,
                    color: tokens.color(ColorToken::AccentGlow),
                }),
                ..VisualStyle::default()
            }
        } else {
            VisualStyle {
                corner_radius: CornerRadius::uniform(RadiusToken::Md.px()),
                ..VisualStyle::default()
            }
        };
        LayoutNode::Stack(
            Stack {
                id: item.value,
                direction: Direction::Row,
                gap: FxPx::new(12),
                padding: EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12)),
                align: Align::Center,
                min_height: Some(FxPx::new(36)),
                ..Stack::default()
            },
            visual,
            row,
        )
    }

    /// Build the sidebar node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let separator = tokens.color(ColorToken::Separator);
        let visual = if self.plain {
            VisualStyle::default()
        } else {
            VisualStyle {
                background: Some(tokens.color(ColorToken::GlassCard)),
                border: EdgeBorder {
                    right: Some(Border { width: HAIRLINE, color: separator }),
                    ..EdgeBorder::none()
                },
                ..VisualStyle::default()
            }
        };
        let width = FxPx::new(self.width.unwrap_or(SIDEBAR_WIDTH));

        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(header) = self.header.clone() {
            let pad = EdgeInsets {
                top: FxPx::new(12),
                right: FxPx::new(16),
                bottom: FxPx::new(4),
                left: FxPx::new(16),
            };
            col.push(LayoutNode::Stack(slot(pad), VisualStyle::default(), alloc::vec![header]));
        }
        let rows: Vec<LayoutNode> =
            self.items.clone().into_iter().map(|item| self.row(tokens, item)).collect();
        col.push(LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                gap: FxPx::new(2),
                padding: EdgeInsets::symmetric(FxPx::new(4), FxPx::new(8)),
                align: Align::Stretch,
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            rows,
        ));
        if let Some(footer) = self.footer.clone() {
            col.push(LayoutNode::Stack(
                slot(EdgeInsets::all(FxPx::new(12))),
                VisualStyle {
                    border: EdgeBorder {
                        top: Some(Border { width: HAIRLINE, color: separator }),
                        ..EdgeBorder::none()
                    },
                    ..VisualStyle::default()
                },
                alloc::vec![footer],
            ));
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                align: Align::Stretch,
                min_width: Some(width),
                max_width: Some(width),
                ..Stack::default()
            },
            visual,
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

    /// Build the split-view row: the rail at full height, the content pane taking the rest
    /// (it may shrink to nothing) and scrolling vertically.
    pub fn build(self) -> LayoutNode {
        let content = LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                align: Align::Stretch,
                overflow: Overflow::Scroll(ScrollAxis::Vertical),
                min_width: Some(FxPx::ZERO),
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![self.content],
        );
        let mut sidebar = self.sidebar;
        if let LayoutNode::Stack(s, _, _) = &mut sidebar {
            s.item.flex_shrink = 0;
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                align: Align::Stretch,
                overflow: Overflow::Hidden,
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![sidebar, content],
        )
    }
}
