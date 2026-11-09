// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `TabBar` — the design artifacts' tab bar (design `TabBar`, RFC-0116), also the base of the
//! window action bar and of an app's navigation rail. Items are columns of an icon and a
//! caption2 semibold label (8 12 padding, 4 gap, min 60 wide): `label` at 62 % at rest, `accent`
//! when active, `danger` for a destructive item; the only background is the `fill` capsule
//! under the active item (the design's sliding indicator — its slide is the runtime's). The bar
//! is a capsule when floating (24 when full width or vertical), 2 px between items, on one of
//! three surfaces: `glass` (glassCard, a separator hairline, shadow2 when floating, the lit top
//! edge), `overlay` (the opaque floating surface) or `none`. Badges hang off the icon's
//! top-right corner; dividers are hairlines. Pure builder producing a `LayoutNode::Stack`.

extern crate alloc;

#[cfg(test)]
mod tests;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx, Justify,
    LayoutNode, Position, Rgba8, Stack, VisualStyle,
};
use nexus_style::box_shadow;
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_icon::with_ink;
use nexus_widget_text::Text;

/// One tab (icon is a caller node; label is owned), or a divider.
#[derive(Debug, Clone, Default)]
pub struct TabItem {
    pub label: String,
    pub icon: Option<LayoutNode>,
    badge: Option<String>,
    danger: bool,
    divider: bool,
    id: Option<&'static str>,
}

impl TabItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), ..Self::default() }
    }
    /// A hairline between groups of items.
    pub fn divider() -> Self {
        Self { divider: true, ..Self::default() }
    }
    /// The icon; the item inks it with its label.
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    /// A count badge off the icon's top-right corner (16 px capsule, `danger`).
    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }
    /// A destructive item: `danger` at full strength.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }
    /// The item's node id (its tap target).
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
}

/// The bar's surface (design `TabBar.surface`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabBarSurface {
    /// On a blur surface: glassCard, a separator hairline, the lit top edge (+ shadow2 floating).
    #[default]
    Glass,
    /// Floating above content: the opaque overlay surface, glass edge, shadow3, lit top edge.
    Overlay,
    /// Bare (a rail inside a window).
    None,
}

/// A tab bar.
#[derive(Debug, Clone)]
pub struct TabBar {
    tabs: Vec<TabItem>,
    active: usize,
    floating: bool,
    vertical: bool,
    surface: TabBarSurface,
    show_labels: bool,
    id: Option<&'static str>,
}

impl Default for TabBar {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            floating: true,
            vertical: false,
            surface: TabBarSurface::Glass,
            show_labels: true,
            id: None,
        }
    }
}

/// `color` at `pct` % of its own alpha.
fn fade(color: Rgba8, pct: u32) -> Rgba8 {
    Rgba8 { a: (u32::from(color.a) * pct / 100) as u8, ..color }
}

impl TabBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tabs(mut self, tabs: Vec<TabItem>) -> Self {
        self.tabs = tabs;
        self
    }
    /// The active tab's index in `tabs`.
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }
    /// A floating capsule (default) vs a full-width bar whose items share the width.
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }
    /// A vertical rail (items stacked, 56 wide).
    pub fn vertical(mut self, vertical: bool) -> Self {
        self.vertical = vertical;
        self
    }
    pub fn surface(mut self, surface: TabBarSurface) -> Self {
        self.surface = surface;
        self
    }
    /// Show the labels (default); an icon-only rail hides them.
    pub fn show_labels(mut self, show: bool) -> Self {
        self.show_labels = show;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The radius of the bar: a capsule floating, 24 full width or vertical.
    fn radius(&self) -> FxPx {
        if !self.vertical && self.floating {
            RadiusToken::Full.px()
        } else {
            RadiusToken::Xl.px()
        }
    }

    /// The radius of an item and of the indicator: a capsule, or concentric in a vertical bar.
    fn item_radius(&self) -> FxPx {
        if self.vertical {
            FxPx::new(RadiusToken::Xl.px().0 - 4)
        } else {
            RadiusToken::Full.px()
        }
    }

    fn divider(&self, tokens: &dyn Tokens) -> LayoutNode {
        let (w, h, margin) = if self.vertical {
            (None, Some(FxPx::ONE), EdgeInsets::symmetric(FxPx::new(4), FxPx::new(8)))
        } else {
            (Some(FxPx::ONE), None, EdgeInsets::symmetric(FxPx::new(8), FxPx::new(4)))
        };
        LayoutNode::Stack(
            Stack {
                min_width: w,
                max_width: w,
                min_height: h,
                max_height: h,
                item: FlexItem {
                    flex_shrink: 0,
                    margin,
                    align_self: Some(Align::Stretch),
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(tokens.color(ColorToken::Separator)),
                ..VisualStyle::default()
            },
            alloc::vec![],
        )
    }

    /// The badge off the icon's corner: 16 px capsule (min 16, 0 4 padding), caption2 bold
    /// `onAccent` on `danger`, its top 6 px above the icon, its left 7 px inside the icon's edge.
    fn badge(tokens: &dyn Tokens, text: String, icon_w: i32) -> LayoutNode {
        let label = Text::new(text)
            .text_style(TextStyleToken::Caption2)
            .weight(FontWeight::Bold)
            .color(ColorToken::OnAccent)
            .build(tokens);
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(4)),
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(FxPx::new(16)),
                min_height: Some(FxPx::new(16)),
                max_height: Some(FxPx::new(16)),
                item: FlexItem {
                    position: Position::Absolute,
                    margin: EdgeInsets {
                        top: FxPx::new(-6),
                        left: FxPx::new(icon_w - 7),
                        ..EdgeInsets::zero()
                    },
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(tokens.color(ColorToken::Danger)),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![label],
        )
    }

    fn slot(&self, tokens: &dyn Tokens, active: bool, tab: TabItem) -> LayoutNode {
        let role = if tab.danger {
            ColorToken::Danger
        } else if active {
            ColorToken::Accent
        } else {
            ColorToken::Label
        };
        // At rest the content is dimmed to 62 %; active and destructive items are full strength.
        let lit = active || tab.danger;
        let ink = if lit { tokens.color(role) } else { fade(tokens.color(role), 62) };

        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(icon) = tab.icon {
            let icon_w = icon.min_width().map_or(20, |w| w.0);
            let mut holder = alloc::vec![with_ink(icon, ink)];
            if let Some(badge) = tab.badge {
                holder.push(Self::badge(tokens, badge, icon_w));
            }
            col.push(LayoutNode::Stack(
                Stack {
                    direction: Direction::Row,
                    align: Align::Center,
                    justify: Justify::Center,
                    ..Stack::default()
                },
                VisualStyle::default(),
                holder,
            ));
        }
        if self.show_labels {
            let mut label = Text::new(tab.label)
                .text_style(TextStyleToken::Caption2)
                .weight(FontWeight::Semibold)
                .build(tokens);
            if let LayoutNode::Text(t, _) = &mut label {
                t.style.color = ink;
            }
            col.push(label);
        }

        let full_width = !self.vertical && !self.floating;
        LayoutNode::Stack(
            Stack {
                id: tab.id,
                direction: Direction::Column,
                gap: FxPx::new(4),
                padding: if self.vertical {
                    EdgeInsets::symmetric(FxPx::new(8), FxPx::new(4))
                } else {
                    EdgeInsets::symmetric(FxPx::new(8), FxPx::new(12))
                },
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(FxPx::new(if self.vertical { 56 } else { 60 })),
                min_height: (self.vertical && !self.show_labels).then_some(FxPx::new(44)),
                item: FlexItem {
                    flex_grow: u32::from(full_width),
                    flex_basis: full_width.then_some(FxPx::ZERO),
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle {
                // The indicator: the `fill` capsule under the active item.
                background: active.then(|| tokens.color(ColorToken::Fill)),
                corner_radius: CornerRadius::uniform(self.item_radius()),
                ..VisualStyle::default()
            },
            col,
        )
    }

    fn surface_style(&self, tokens: &dyn Tokens) -> VisualStyle {
        let lit = Some(tokens.shadow(ShadowToken::EdgeHighlight).color);
        let radius = CornerRadius::uniform(self.radius());
        match self.surface {
            TabBarSurface::Glass => VisualStyle {
                background: Some(tokens.color(ColorToken::GlassCard)),
                border: EdgeBorder::all(HAIRLINE, tokens.color(ColorToken::Separator)),
                shadow: self.floating.then(|| box_shadow(tokens.shadow(ShadowToken::Shadow2))),
                inset_highlight: lit,
                corner_radius: radius,
                ..VisualStyle::default()
            },
            TabBarSurface::Overlay => VisualStyle {
                background: Some(tokens.color(ColorToken::SurfaceOverlay)),
                border: EdgeBorder::all(HAIRLINE, tokens.color(ColorToken::GlassEdge)),
                shadow: Some(box_shadow(tokens.shadow(ShadowToken::Shadow3))),
                inset_highlight: lit,
                corner_radius: radius,
                ..VisualStyle::default()
            },
            TabBarSurface::None => VisualStyle { corner_radius: radius, ..VisualStyle::default() },
        }
    }

    /// Build the tab-bar node.
    pub fn build(mut self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.surface_style(tokens);
        // The design's 1 px border (transparent on a bare bar) sits outside the 4 px padding;
        // the painter draws borders inside the box, so the padding carries that pixel.
        let pad = FxPx::new(if self.surface == TabBarSurface::None { 1 } else { 5 });
        let justify = if self.vertical {
            Justify::Start
        } else if self.floating {
            Justify::Center
        } else {
            Justify::SpaceAround
        };

        let active = self.active;
        let tabs = core::mem::take(&mut self.tabs);
        let mut slots: Vec<LayoutNode> = Vec::with_capacity(tabs.len());
        for (i, tab) in tabs.into_iter().enumerate() {
            if tab.divider {
                slots.push(self.divider(tokens));
            } else {
                slots.push(self.slot(tokens, i == active, tab));
            }
        }

        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: if self.vertical { Direction::Column } else { Direction::Row },
                gap: FxPx::new(2),
                padding: EdgeInsets::all(pad),
                align: Align::Stretch,
                justify,
                ..Stack::default()
            },
            style,
            slots,
        )
    }
}
