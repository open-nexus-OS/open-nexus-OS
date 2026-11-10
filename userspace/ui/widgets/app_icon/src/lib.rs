// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `AppIcon` — the design-system app icon (design artifacts `AppIcon`), two modes ×
//! five sizes: `Native` (the icon IS the tile: radius by size, clipped, `shadow2`, no glass) and
//! `Freestanding` (the bare artwork: no clip, no shadow — Stash, the special folders). The
//! notification badge sits on the icon area OUTSIDE the tile's clip (top −4 / right −4; inset
//! for freestanding art), the dock's active dot under the tile, and the label under both (the
//! size's label style at weight 600, `label`, one line, the label halo). A pure builder
//! producing a `LayoutNode::Stack` column around a caller-provided icon node. The press scale
//! is host motion. DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (appicon_*)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Overflow, Position, Rgba8, Stack, TextAlign, TextContent, TextNode, TextShadow, TextStyle,
    VisualStyle, WhiteSpace,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, RadiusToken, ShadowToken, TextStyleToken, Tokens};

/// Rendering mode (design `AppIconProps.variant`; an unknown variant renders native).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppIconVariant {
    /// The icon fills the rounded tile (every app icon is shaped).
    #[default]
    Native,
    /// Bare artwork, no backing, no clip (Stash, pre-composed special-folder icons).
    Freestanding,
}

/// Size preset (design `AppIconProps.size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppIconSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
}

impl AppIconSize {
    /// Tile edge length (px): 40 / 48 / 56 / 72 / 88.
    pub fn dimension(self) -> i32 {
        match self {
            AppIconSize::Xs => 40,
            AppIconSize::Sm => 48,
            AppIconSize::Md => 56,
            AppIconSize::Lg => 72,
            AppIconSize::Xl => 88,
        }
    }

    /// Tile radius: md / md / lg / xl / xl.
    pub fn radius(self) -> RadiusToken {
        match self {
            AppIconSize::Xs | AppIconSize::Sm => RadiusToken::Md,
            AppIconSize::Md => RadiusToken::Lg,
            AppIconSize::Lg | AppIconSize::Xl => RadiusToken::Xl,
        }
    }

    /// The label's text style (set at weight 600).
    pub fn label_style(self) -> TextStyleToken {
        match self {
            AppIconSize::Xs => TextStyleToken::Caption2,
            AppIconSize::Sm => TextStyleToken::Caption,
            AppIconSize::Md => TextStyleToken::Subheadline,
            AppIconSize::Lg => TextStyleToken::Body,
            AppIconSize::Xl => TextStyleToken::Headline,
        }
    }

    /// Gap between the icon and its label: 4 / 6 / 8 / 8 / 12.
    pub fn gap(self) -> i32 {
        match self {
            AppIconSize::Xs => 4,
            AppIconSize::Sm => 6,
            AppIconSize::Md | AppIconSize::Lg => 8,
            AppIconSize::Xl => 12,
        }
    }

    /// The label's widest line: the tile + 16.
    pub fn label_max_width(self) -> i32 {
        self.dimension() + 16
    }
}

/// The notification badge's height and minimum width.
const BADGE: i32 = 18;
/// The active dot's diameter.
const DOT: i32 = 4;

/// An app icon.
#[derive(Debug, Clone)]
pub struct AppIcon {
    variant: AppIconVariant,
    size: AppIconSize,
    badge: Option<u32>,
    active: bool,
    halo: bool,
    id: Option<&'static str>,
    content: Option<LayoutNode>,
    label: Option<String>,
}

impl Default for AppIcon {
    fn default() -> Self {
        Self {
            variant: AppIconVariant::Native,
            size: AppIconSize::Md,
            badge: None,
            active: false,
            halo: true,
            id: None,
            content: None,
            label: None,
        }
    }
}

impl AppIcon {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn variant(mut self, variant: AppIconVariant) -> Self {
        self.variant = variant;
        self
    }
    pub fn size(mut self, size: AppIconSize) -> Self {
        self.size = size;
        self
    }
    /// Notification count (`0` = hidden; above 99 it reads "99+").
    pub fn badge(mut self, count: u32) -> Self {
        self.badge = Some(count);
        self
    }
    /// The dock's active-app dot under the tile.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    /// The label under the icon.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// The label halo (on by default — icons sit on the wallpaper; off on a glass surface).
    pub fn halo(mut self, on: bool) -> Self {
        self.halo = on;
        self
    }
    /// Interaction id of the whole icon (tile and label).
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
    /// The icon node (caller-provided artwork, sized to the tile).
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }

    /// The badge count shown (`None` when 0 / unset).
    pub fn badge_count(&self) -> Option<u32> {
        self.badge.filter(|&n| n > 0)
    }

    /// Whether the active dot shows.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The tile's [`Style`]: native = the size radius and `shadow2`; freestanding = nothing.
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        match self.variant {
            AppIconVariant::Native => Style::new()
                .rounded(self.size.radius().px())
                .shadow_token(tokens, ShadowToken::Shadow2),
            AppIconVariant::Freestanding => Style::new(),
        }
    }

    /// The badge: an 18 px `danger` capsule (min width 18, padding 0×4, `shadow1`) with the
    /// count in `caption2` weight 700 `onAccent`.
    fn badge_node(count: u32, tokens: &dyn Tokens) -> LayoutNode {
        let text = if count > 99 { String::from("99+") } else { format!("{count}") };
        let style = TextStyle {
            white_space: WhiteSpace::NoWrap,
            ..TextStyleToken::Caption2
                .text_style(tokens.color(ColorToken::OnAccent), Some(FontWeight::Bold))
        };
        let h = Some(FxPx::new(BADGE));
        let shadow = Style::new().shadow_token(tokens, ShadowToken::Shadow1).visual().shadow;
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(4)),
                align: Align::Center,
                justify: Justify::Center,
                min_width: h,
                min_height: h,
                max_height: h,
                ..Stack::default()
            },
            VisualStyle {
                background: Some(tokens.color(ColorToken::Danger)),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                shadow,
                ..VisualStyle::default()
            },
            alloc::vec![LayoutNode::Text(
                TextNode {
                    content: TextContent::new(text),
                    style,
                    max_lines: Some(1),
                    ..TextNode::default()
                },
                VisualStyle::default(),
            )],
        )
    }

    /// The layer that pins the badge to the icon area's top-right corner without measuring
    /// it: an absolute, filling row (`justify: End`) whose margins place the corner — native
    /// 4 px past the tile, freestanding inset 12 % from the top and 2 % from the right.
    fn badge_layer(&self, badge: LayoutNode) -> LayoutNode {
        let dim = self.size.dimension();
        let (top, right) = match self.variant {
            AppIconVariant::Native => (-4, -4),
            AppIconVariant::Freestanding => ((dim * 12 + 50) / 100, (dim * 2 + 50) / 100),
        };
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                align: Align::Start,
                justify: Justify::End,
                item: FlexItem {
                    position: Position::Absolute,
                    flex_grow: 1,
                    z_index: 10,
                    margin: EdgeInsets {
                        top: FxPx::new(top),
                        right: FxPx::new(right),
                        ..EdgeInsets::zero()
                    },
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle::default(),
            alloc::vec![badge],
        )
    }

    fn label_node(&self, label: &str, tokens: &dyn Tokens) -> LayoutNode {
        let max = Some(FxPx::new(self.size.label_max_width()));
        let style = TextStyle {
            text_align: TextAlign::Center,
            white_space: WhiteSpace::NoWrap,
            ..self
                .size
                .label_style()
                .text_style(tokens.color(ColorToken::Label), Some(FontWeight::Semibold))
        };
        let halo = match self.halo {
            true => TextShadow::halo(tokens.color(ColorToken::LabelShadow)).map(Some),
            false => [None; 2],
        };
        LayoutNode::Text(
            TextNode {
                content: TextContent::new(label),
                style,
                max_lines: Some(1),
                max_width: max,
                ..TextNode::default()
            },
            VisualStyle { text_shadows: halo, ..VisualStyle::default() },
        )
    }

    /// Build the icon: icon area (tile + badge) · active dot · label.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let dim = Some(FxPx::new(self.size.dimension()));
        let overflow = match self.variant {
            AppIconVariant::Native => Overflow::Hidden,
            AppIconVariant::Freestanding => Overflow::Visible,
        };
        let square = Stack {
            direction: Direction::Row,
            align: Align::Center,
            justify: Justify::Center,
            min_width: dim,
            max_width: dim,
            min_height: dim,
            max_height: dim,
            ..Stack::default()
        };
        let tile = LayoutNode::Stack(
            Stack { overflow, ..square.clone() },
            self.style(tokens).visual(),
            self.content.clone().into_iter().collect(),
        );
        let mut area = alloc::vec![tile];
        if let Some(count) = self.badge_count() {
            area.push(self.badge_layer(Self::badge_node(count, tokens)));
        }
        let mut col: Vec<LayoutNode> = alloc::vec![LayoutNode::Stack(
            Stack { overflow: Overflow::Visible, ..square },
            VisualStyle::default(),
            area,
        )];
        if self.active {
            let d = Some(FxPx::new(DOT));
            let label = tokens.color(ColorToken::Label);
            col.push(LayoutNode::Stack(
                Stack {
                    min_width: d,
                    max_width: d,
                    min_height: d,
                    max_height: d,
                    item: FlexItem {
                        margin: EdgeInsets { top: FxPx::new(-4), ..EdgeInsets::zero() },
                        ..FlexItem::default()
                    },
                    ..Stack::default()
                },
                VisualStyle {
                    // `label` at opacity .7.
                    background: Some(Rgba8::new(
                        label.r,
                        label.g,
                        label.b,
                        (u32::from(label.a) * 7 / 10) as u8,
                    )),
                    corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                    ..VisualStyle::default()
                },
                Vec::new(),
            ));
        }
        if let Some(label) = &self.label {
            col.push(self.label_node(label, tokens));
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Column,
                gap: FxPx::new(self.size.gap()),
                align: Align::Center,
                ..Stack::default()
            },
            VisualStyle::default(),
            col,
        )
    }
}

#[cfg(test)]
mod tests;
