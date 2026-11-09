// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `Banner` — the design-system inline status strip (design artifacts `Banner`): a
//! row on the status colour's 16 % wash (no blur, no border, radius md, padding 12×16, gap 12,
//! aligned to the top) with the 18 px status icon, the title (`body` 600) / message
//! (`subheadline`) / action (`subheadline` 600 in the status colour) column, and the 22 px
//! dismiss circle. INLINE — it takes layout space; the floating sibling is `Toast`. A pure
//! `LayoutNode` builder from theme tokens. DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (banner_*)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    PathPoint, PathShape, Rgba8, ShapeKind, Stack, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens};
use nexus_widget_text::Text;

/// Status (design `BannerProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BannerVariant {
    #[default]
    Info,
    Success,
    Warning,
    Destructive,
}

impl BannerVariant {
    /// The status colour: info `accent`, success, warning, destructive `danger`.
    pub fn role(self) -> ColorToken {
        match self {
            Self::Info => ColorToken::Accent,
            Self::Success => ColorToken::Success,
            Self::Warning => ColorToken::Warning,
            Self::Destructive => ColorToken::Danger,
        }
    }

    /// The default icon's polylines (design: `0..1000` of the 24-unit paths, stroke 2).
    fn icon(self) -> &'static [&'static [(u16, u16)]] {
        match self {
            Self::Info => INFO,
            Self::Success => SUCCESS,
            Self::Warning => WARNING,
            Self::Destructive => DESTRUCTIVE,
        }
    }
}

/// Info: the "i" (design `M12 8h.01M11 12h1v4h1`).
const INFO: &[&[(u16, u16)]] = &[&[(500, 333)], &[(458, 500), (500, 500), (500, 667), (542, 667)]];
/// Success: the check (design `M4 12.5l5 5L20 6.5`).
const SUCCESS: &[&[(u16, u16)]] = &[&[(167, 521), (375, 729), (833, 271)]];
/// Destructive: the small cross (design `M15 9l-6 6M9 9l6 6`).
const DESTRUCTIVE: &[&[(u16, u16)]] = &[&[(625, 375), (375, 625)], &[(375, 375), (625, 625)]];
/// Warning: the rounded triangle with "!" (design path, arcs flattened).
const WARNING: &[&[(u16, u16)]] = &[
    &[(500, 375), (500, 542)],
    &[(500, 708)],
    &[
        (429, 162),
        (100, 750),
        (92, 770),
        (89, 791),
        (92, 813),
        (100, 833),
        (113, 850),
        (130, 863),
        (149, 872),
        (171, 875),
        (829, 875),
        (851, 872),
        (870, 863),
        (887, 850),
        (900, 833),
        (908, 813),
        (911, 791),
        (908, 770),
        (900, 750),
        (571, 162),
        (558, 146),
        (541, 134),
        (521, 126),
        (500, 123),
        (479, 126),
        (459, 134),
        (442, 146),
        (429, 162),
    ],
];
/// The dismiss cross (design `M6 6l12 12M18 6L6 18`, stroke 2.4).
const CROSS: &[&[(u16, u16)]] = &[&[(250, 250), (750, 750)], &[(750, 250), (250, 750)]];

/// The status icon's edge (px) and the dismiss button's.
const ICON: i32 = 18;
const DISMISS: i32 = 22;

/// A line glyph in a `size` px box: the polylines stroked `width_milli` with round caps.
fn glyph(paths: &[&[(u16, u16)]], width_milli: u16, size: i32, color: Rgba8) -> LayoutNode {
    let paths = paths
        .iter()
        .map(|p| PathShape {
            points: p.iter().map(|&(x, y)| PathPoint::new(x, y)).collect(),
            closed: false,
        })
        .collect();
    let d = Some(FxPx::new(size));
    LayoutNode::Stack(
        Stack {
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
            ..Stack::default()
        },
        VisualStyle {
            background: Some(color),
            shape: ShapeKind::Stroke { paths, width_milli },
            ..VisualStyle::default()
        },
        Vec::new(),
    )
}

/// `color` at `percent` % of its own alpha (the design's tint wash).
const fn wash(color: Rgba8, percent: u32) -> Rgba8 {
    Rgba8::new(color.r, color.g, color.b, ((color.a as u32 * percent + 50) / 100) as u8)
}

/// An inline status strip.
#[derive(Debug, Clone, Default)]
pub struct Banner {
    title: Option<String>,
    message: Option<String>,
    variant: BannerVariant,
    /// Replaces the status icon.
    icon: Option<LayoutNode>,
    action: Option<String>,
    dismissible: bool,
    id: Option<&'static str>,
    action_id: Option<&'static str>,
    dismiss_id: Option<&'static str>,
}

impl Banner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
    pub fn variant(mut self, variant: BannerVariant) -> Self {
        self.variant = variant;
        self
    }
    /// A caller icon in place of the status icon.
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Action label (design `action`) under the message; give it an id for hit-testing.
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }
    /// Shows the dismiss circle (design `onDismiss` presence).
    pub fn dismissible(mut self, on: bool) -> Self {
        self.dismissible = on;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
    pub fn action_id(mut self, id: &'static str) -> Self {
        self.action_id = Some(id);
        self
    }
    pub fn dismiss_id(mut self, id: &'static str) -> Self {
        self.dismiss_id = Some(id);
        self
    }

    /// The text column: title · message · action (margin-top 4, aligned start).
    fn text_column(&self, status: ColorToken, tokens: &dyn Tokens) -> LayoutNode {
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(title) = &self.title {
            col.push(
                Text::new(title.as_str())
                    .text_style(TextStyleToken::Body)
                    .weight(FontWeight::Semibold)
                    .lines(None)
                    .build(tokens),
            );
        }
        if let Some(message) = &self.message {
            col.push(
                Text::new(message.as_str())
                    .text_style(TextStyleToken::Subheadline)
                    .lines(None)
                    .build(tokens),
            );
        }
        if let Some(action) = &self.action {
            let label = Text::new(action.as_str())
                .text_style(TextStyleToken::Subheadline)
                .weight(FontWeight::Semibold)
                .color(status)
                .build(tokens);
            col.push(LayoutNode::Stack(
                Stack {
                    id: self.action_id,
                    direction: Direction::Row,
                    item: FlexItem {
                        align_self: Some(Align::Start),
                        margin: EdgeInsets { top: FxPx::new(4), ..EdgeInsets::zero() },
                        ..FlexItem::default()
                    },
                    ..Stack::default()
                },
                VisualStyle {
                    corner_radius: CornerRadius::uniform(RadiusToken::Sm.px()),
                    ..VisualStyle::default()
                },
                alloc::vec![label],
            ));
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                gap: FxPx::new(2),
                align: Align::Stretch,
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle::default(),
            col,
        )
    }

    /// The dismiss circle: 22 px, transparent, the 12 px cross (stroke 2.4) in `labelSecondary`.
    fn dismiss(&self, tokens: &dyn Tokens) -> LayoutNode {
        let d = Some(FxPx::new(DISMISS));
        LayoutNode::Stack(
            Stack {
                id: self.dismiss_id,
                align: Align::Center,
                justify: Justify::Center,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![glyph(CROSS, 100, 12, tokens.color(ColorToken::LabelSecondary))],
        )
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let status = self.variant.role();
        let mut icon = match &self.icon {
            Some(icon) => icon.clone(),
            None => glyph(self.variant.icon(), 83, ICON, tokens.color(status)),
        };
        if let LayoutNode::Stack(s, _, _) = &mut icon {
            s.item.margin.top = FxPx::new(1);
        }
        let mut row = alloc::vec![icon, self.text_column(status, tokens)];
        if self.dismissible {
            row.push(self.dismiss(tokens));
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(12),
                padding: EdgeInsets::symmetric(FxPx::new(12), FxPx::new(16)),
                align: Align::Start,
                ..Stack::default()
            },
            VisualStyle {
                background: Some(wash(tokens.color(status), 16)),
                corner_radius: CornerRadius::uniform(RadiusToken::Md.px()),
                ..VisualStyle::default()
            },
            row,
        )
    }
}

#[cfg(test)]
mod tests;
