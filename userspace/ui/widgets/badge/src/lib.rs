// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `Badge` — the design-system status chip and count (design artifacts `Badge`): a
//! capsule on a fixed height per size (min width = height), always a 1 px border in the
//! variant's colour, text at weight 500 or a count at weight 600 capped by `max` ("99+"), or a
//! bare status `dot`. Status variants are tint washes (16 % fill, 30 % border, the status as
//! text); `tint` paints any role filled, or as a wash with `Tinted`. A pure builder producing a
//! `LayoutNode::Stack`. DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (badge_*, sidebar, a11y)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Rgba8, Stack, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, TextStyleToken, Tokens, HAIRLINE};

/// Visual variant (design `BadgeProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeVariant {
    /// `primary` with `onPrimary`.
    #[default]
    Default,
    /// The control fill with `label`.
    Secondary,
    /// The glass card layer with its edge (never blurs).
    Glass,
    /// `danger` with `onAccent`.
    Destructive,
    /// A `success` wash with the status as text.
    Success,
    /// A `warning` wash with the status as text.
    Warning,
    /// Transparent with a `separator` edge.
    Outline,
    /// `accent` with `onAccent`.
    Active,
    /// Filled in the tint (the look of a tint without a variant).
    Filled,
    /// A wash of the tint with the tint as text.
    Tinted,
}

/// One size's badge geometry: (height = min width, text style, text padding, number padding,
/// dot).
fn metrics(size: ControlSize) -> (i32, TextStyleToken, i32, i32, i32) {
    match size {
        ControlSize::Xs => (16, TextStyleToken::Caption2, 4, 4, 6),
        ControlSize::Sm => (18, TextStyleToken::Caption2, 4, 4, 8),
        ControlSize::Md => (22, TextStyleToken::Caption, 8, 4, 10),
        ControlSize::Lg => (26, TextStyleToken::Footnote, 8, 8, 12),
        ControlSize::Xl => (30, TextStyleToken::Subheadline, 12, 8, 14),
    }
}

/// `color` at `percent` % of its own alpha (the design's tint wash).
const fn wash(color: Rgba8, percent: u32) -> Rgba8 {
    Rgba8::new(color.r, color.g, color.b, ((color.a as u32 * percent + 50) / 100) as u8)
}

/// Text on a filled tint: `onPrimary` on `primary` / `label`, `onAccent` otherwise.
fn on_tint(tint: ColorToken) -> ColorToken {
    match tint {
        ColorToken::Primary | ColorToken::Label => ColorToken::OnPrimary,
        _ => ColorToken::OnAccent,
    }
}

/// A status chip or count.
#[derive(Debug, Clone, Default)]
pub struct Badge {
    variant: Option<BadgeVariant>,
    tint: Option<ColorToken>,
    size: ControlSize,
    dot: bool,
    max: Option<u32>,
    id: Option<&'static str>,
    content: Option<LayoutNode>,
    label: Option<String>,
    count: Option<u32>,
}

impl Badge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = Some(variant);
        self
    }
    /// A tint role: filled in it, or its wash with [`BadgeVariant::Tinted`].
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }
    /// The size (xs 16 · sm 18 · md 22 · lg 26 · xl 30).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    /// A bare status dot instead of a chip.
    pub fn dot(mut self, on: bool) -> Self {
        self.dot = on;
        self
    }
    /// Cap a count: above `max` it reads `max+`.
    pub fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
    /// Caller content (coloured with [`foreground`](Self::foreground) by the caller).
    pub fn content(mut self, content: LayoutNode) -> Self {
        self.content = Some(content);
        self
    }
    /// A text label in the size's text style (weight 500; a string of digits is a count).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// A count (weight 600, number padding, capped by `max`).
    pub fn count(mut self, count: u32) -> Self {
        self.count = Some(count);
        self
    }

    /// The look in effect: the variant, else `Filled` for a tint, else `Default`.
    pub fn look(&self) -> BadgeVariant {
        self.variant.unwrap_or(if self.tint.is_some() {
            BadgeVariant::Filled
        } else {
            BadgeVariant::Default
        })
    }

    /// `(background, text, border)` of the look; `None` = transparent.
    fn colors(&self, tokens: &dyn Tokens) -> (Option<Rgba8>, Rgba8, Option<Rgba8>) {
        let tint = self.tint.unwrap_or(ColorToken::Accent);
        let c = |role: ColorToken| tokens.color(role);
        let status = |role: ColorToken| (Some(wash(c(role), 16)), c(role), Some(wash(c(role), 30)));
        match self.look() {
            BadgeVariant::Default => (Some(c(ColorToken::Primary)), c(ColorToken::OnPrimary), None),
            BadgeVariant::Secondary => (Some(c(ColorToken::Fill)), c(ColorToken::Label), None),
            BadgeVariant::Glass => (
                Some(c(ColorToken::GlassCard)),
                c(ColorToken::Label),
                Some(c(ColorToken::GlassEdge)),
            ),
            BadgeVariant::Destructive => {
                (Some(c(ColorToken::Danger)), c(ColorToken::OnAccent), None)
            }
            BadgeVariant::Success => status(ColorToken::Success),
            BadgeVariant::Warning => status(ColorToken::Warning),
            BadgeVariant::Outline => (None, c(ColorToken::Label), Some(c(ColorToken::Separator))),
            BadgeVariant::Active => (Some(c(ColorToken::Accent)), c(ColorToken::OnAccent), None),
            BadgeVariant::Filled => (Some(c(tint)), c(on_tint(tint)), None),
            BadgeVariant::Tinted => status(tint),
        }
    }

    /// The resolved foreground (label) colour.
    pub fn foreground(&self, tokens: &dyn Tokens) -> Rgba8 {
        self.colors(tokens).1
    }

    /// The resolved container [`Style`]: the capsule, its fill and its 1 px border.
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let (bg, _, border) = self.colors(tokens);
        let mut s = Style::new().rounded(RadiusToken::Full.px());
        if let Some(bg) = bg {
            s = s.background(bg);
        }
        if let Some(border) = border {
            s = s.border(HAIRLINE, border);
        }
        s
    }

    /// The count, if the badge shows one: `count`, or a label made of digits.
    fn number(&self) -> Option<u32> {
        self.count.or_else(|| {
            let l = self.label.as_deref()?;
            (!l.is_empty() && l.bytes().all(|b| b.is_ascii_digit())).then(|| l.parse().ok())?
        })
    }

    /// The text the badge shows (`99+` above `max`).
    pub fn text(&self) -> Option<String> {
        match (self.number(), self.max) {
            (Some(n), Some(max)) if n > max => Some(format!("{max}+")),
            (Some(n), _) => Some(format!("{n}")),
            (None, _) => self.label.clone(),
        }
    }

    /// Build the badge (or the dot).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let (h, text_style, pad, npad, dot) = metrics(self.size);
        let visual = self.style(tokens).visual();
        if self.dot {
            let d = Some(FxPx::new(dot));
            return LayoutNode::Stack(
                Stack {
                    id: self.id,
                    min_width: d,
                    max_width: d,
                    min_height: d,
                    max_height: d,
                    item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                    ..Stack::default()
                },
                visual,
                Vec::new(),
            );
        }
        let numeric = self.number().is_some();
        let mut children: Vec<LayoutNode> = Vec::new();
        if let Some(text) = self.text() {
            let weight = if numeric { FontWeight::Semibold } else { FontWeight::Medium };
            children.push(LayoutNode::Text(
                TextNode {
                    content: TextContent::new(text),
                    style: TextStyle {
                        white_space: WhiteSpace::NoWrap,
                        ..text_style.text_style(self.foreground(tokens), Some(weight))
                    },
                    max_lines: Some(1),
                    ..TextNode::default()
                },
                VisualStyle::default(),
            ));
        } else if let Some(content) = self.content {
            children.push(content);
        }
        // Border-box: the 1 px border is always there (transparent on most looks), so the
        // content inset reserves it.
        let side = FxPx::new(if numeric { npad } else { pad }) + HAIRLINE;
        let h = Some(FxPx::new(h));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::ZERO, side),
                align: Align::Center,
                justify: Justify::Center,
                min_width: h,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle { corner_radius: CornerRadius::uniform(RadiusToken::Full.px()), ..visual },
            children,
        )
    }
}

#[cfg(test)]
mod tests;
