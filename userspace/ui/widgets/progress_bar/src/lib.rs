// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `ProgressBar` — the design-system progress indicator (design artifacts
//! `ProgressBar`). Linear (default): a capsule track in the control fill as wide as its
//! container, the tinted fill at `value` % of it (the accent fill glows), or the 40 % pip while
//! indeterminate (the host's sweep loop moves it — the pip is the track's first child); bar
//! height by size; an optional header with the label and "64 %". Circular: the ring of
//! [`ring`]. A pure `LayoutNode` builder from theme tokens. DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (progress_*); dsl runtime kit_widgets
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

mod ring;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, BoxShadow, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, Fraction, FxPx,
    Justify, LayoutNode, Overflow, Spacer, Stack, TextContent, TextNode, TextStyle, VisualStyle,
    WhiteSpace,
};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, TextStyleToken, Tokens};

/// Opacity of a disabled progress indicator (design: 0.5).
pub const DISABLED_OPACITY: Fraction = Fraction::new(128);
/// The indeterminate pip's share of the track (percent).
pub const PIP_PERCENT: u32 = 40;

/// Linear bar or ring (design `appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressAppearance {
    #[default]
    Linear,
    Circular,
}

/// A progress indicator.
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// 0–100; ignored when `indeterminate`.
    value: u32,
    indeterminate: bool,
    /// Pip phase 0–100 on a pinned-width track (a static frame; the host animates otherwise).
    phase: u32,
    width: Option<FxPx>,
    height: Option<FxPx>,
    size: ControlSize,
    color: ColorToken,
    label: Option<String>,
    show_value: bool,
    appearance: ProgressAppearance,
    disabled: bool,
    id: Option<&'static str>,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            value: 0,
            indeterminate: false,
            phase: 0,
            width: None,
            height: None,
            size: ControlSize::Md,
            color: ColorToken::Accent,
            label: None,
            show_value: false,
            appearance: ProgressAppearance::Linear,
            disabled: false,
            id: None,
        }
    }
}

impl ProgressBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Determinate progress, clamped to 0–100 (design `value`).
    pub fn value(mut self, percent: u32) -> Self {
        self.value = percent.min(100);
        self.indeterminate = false;
        self
    }
    /// Indeterminate: the sliding pip (linear) or the turning arc (circular).
    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
        self
    }
    /// Pip position 0–100 on a pinned-width track (a static frame).
    pub fn phase(mut self, phase: u32) -> Self {
        self.phase = phase.min(100);
        self
    }
    /// Pin the track width (px); by default the bar is as wide as its container.
    pub fn width(mut self, px: i32) -> Self {
        self.width = Some(FxPx::new(px.max(8)));
        self
    }
    /// Bar height override (px); by default the size's 3 / 4 / 6 / 8 / 10.
    pub fn height(mut self, px: i32) -> Self {
        self.height = Some(FxPx::new(px.max(2)));
        self
    }
    /// The control size (bar height, ring, text styles).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    /// The tint (default `Accent`, which glows).
    pub fn tint(self, tint: ColorToken) -> Self {
        self.color(tint)
    }
    /// The fill role (legacy name of [`tint`](Self::tint)).
    pub fn color(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }
    /// The label above the bar (beside the ring).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// Show "64 %" (in the header; in the ring from md up, beside it below md).
    pub fn show_value(mut self, on: bool) -> Self {
        self.show_value = on;
        self
    }
    pub fn appearance(mut self, appearance: ProgressAppearance) -> Self {
        self.appearance = appearance;
        self
    }
    /// Dim to 0.5.
    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// The bar height of the size: 3 / 4 / 6 / 8 / 10 (or the override).
    pub fn bar_height(&self) -> FxPx {
        self.height.unwrap_or_else(|| {
            FxPx::new(match self.size {
                ControlSize::Xs => 3,
                ControlSize::Sm => 4,
                ControlSize::Md => 6,
                ControlSize::Lg => 8,
                ControlSize::Xl => 10,
            })
        })
    }

    /// The header / label text style: caption / footnote / subheadline / body / headline.
    fn label_style(&self) -> TextStyleToken {
        match self.size {
            ControlSize::Xs => TextStyleToken::Caption,
            ControlSize::Sm => TextStyleToken::Footnote,
            ControlSize::Md => TextStyleToken::Subheadline,
            ControlSize::Lg => TextStyleToken::Body,
            ControlSize::Xl => TextStyleToken::Headline,
        }
    }

    /// "64 %" (with the space), or "…" while indeterminate.
    pub fn value_text(&self) -> String {
        if self.indeterminate {
            String::from("…")
        } else {
            format!("{} %", self.value)
        }
    }

    /// The fill's glow: `0 0 8px accentGlow`, for the accent tint only.
    fn glow(&self, tokens: &dyn Tokens) -> Option<BoxShadow> {
        (self.color == ColorToken::Accent).then(|| BoxShadow {
            offset_x: FxPx::ZERO,
            offset_y: FxPx::ZERO,
            blur_radius: FxPx::new(8),
            spread: FxPx::ZERO,
            color: tokens.color(ColorToken::AccentGlow),
        })
    }

    /// A capsule in the tint (the fill or the pip).
    fn bar(&self, item: FlexItem, width: Option<FxPx>, tokens: &dyn Tokens) -> LayoutNode {
        let h = Some(self.bar_height());
        LayoutNode::Stack(
            Stack {
                min_width: width,
                max_width: width,
                min_height: h,
                max_height: h,
                item,
                ..Stack::default()
            },
            VisualStyle {
                background: Some(tokens.color(self.color)),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                shadow: self.glow(tokens),
                ..VisualStyle::default()
            },
            Vec::new(),
        )
    }

    /// The share `[fill, rest]` split by flex growth: the bar as `grow` parts of the track,
    /// the rest a spacer — exact at any track width, no measuring.
    fn split(&self, grow: u32, rest: u32, tokens: &dyn Tokens) -> [LayoutNode; 2] {
        let basis =
            FlexItem { flex_grow: grow, flex_basis: Some(FxPx::ZERO), ..FlexItem::default() };
        let spacer = LayoutNode::Spacer(Spacer {
            flex_grow: rest,
            item: FlexItem { flex_grow: rest, flex_basis: Some(FxPx::ZERO), ..FlexItem::default() },
            ..Spacer::default()
        });
        [self.bar(basis, None, tokens), spacer]
    }

    /// The linear track: the fill (or the pip) is its FIRST child — the host's sweep loop
    /// translates exactly that child.
    fn track(&self, tokens: &dyn Tokens) -> LayoutNode {
        let h = Some(self.bar_height());
        let children: Vec<LayoutNode> = match (self.indeterminate, self.width) {
            (true, Some(w)) => {
                // A pinned track: the pip at `phase` along its travel (a static frame).
                let pip = w.0 * PIP_PERCENT as i32 / 100;
                let left = (w.0 - pip) * self.phase as i32 / 100;
                let item = FlexItem {
                    margin: EdgeInsets { left: FxPx::new(left), ..EdgeInsets::zero() },
                    flex_shrink: 0,
                    ..FlexItem::default()
                };
                alloc::vec![self.bar(item, Some(FxPx::new(pip)), tokens)]
            }
            (true, None) => self.split(PIP_PERCENT, 100 - PIP_PERCENT, tokens).into(),
            (false, _) => self.split(self.value, 100 - self.value, tokens).into(),
        };
        LayoutNode::Stack(
            Stack {
                id: if self.has_header() { None } else { self.id },
                direction: Direction::Row,
                align: Align::Center,
                overflow: Overflow::Hidden,
                min_width: self.width,
                max_width: self.width,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(tokens.color(ColorToken::Fill)),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            children,
        )
    }

    fn has_header(&self) -> bool {
        self.label.is_some() || self.show_value
    }

    /// The header: label (weight 500, `label`) · value (`labelSecondary`), space between.
    fn header(&self, tokens: &dyn Tokens) -> LayoutNode {
        let style = self.label_style();
        let mut kids: Vec<LayoutNode> = Vec::new();
        if let Some(label) = &self.label {
            let s = style.text_style(tokens.color(ColorToken::Label), Some(FontWeight::Medium));
            kids.push(text(label.as_str(), s));
        }
        if self.show_value {
            let s = style.text_style(tokens.color(ColorToken::LabelSecondary), None);
            kids.push(text(self.value_text(), s));
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: FxPx::new(12),
                align: Align::End,
                justify: if self.label.is_some() { Justify::SpaceBetween } else { Justify::End },
                ..Stack::default()
            },
            VisualStyle::default(),
            kids,
        )
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let mut node = match self.appearance {
            ProgressAppearance::Circular => ring::build(&self, tokens),
            ProgressAppearance::Linear if self.has_header() => {
                let gap =
                    if matches!(self.size, ControlSize::Xs | ControlSize::Sm) { 4 } else { 8 };
                LayoutNode::Stack(
                    Stack {
                        id: self.id,
                        direction: Direction::Column,
                        gap: FxPx::new(gap),
                        align: Align::Stretch,
                        ..Stack::default()
                    },
                    VisualStyle::default(),
                    alloc::vec![self.header(tokens), self.track(tokens)],
                )
            }
            ProgressAppearance::Linear => self.track(tokens),
        };
        if self.disabled {
            if let LayoutNode::Stack(_, v, _) = &mut node {
                v.opacity = Some(DISABLED_OPACITY);
            }
        }
        node
    }
}

/// A one-line text node.
fn text(content: impl Into<String>, style: TextStyle) -> LayoutNode {
    LayoutNode::Text(
        TextNode {
            content: TextContent::new(content.into()),
            style: TextStyle { white_space: WhiteSpace::NoWrap, ..style },
            max_lines: Some(1),
            ..TextNode::default()
        },
        VisualStyle::default(),
    )
}

#[cfg(test)]
mod tests;
