// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Stepper` — the design-system −/+ stepper (the design artifacts' `Stepper`): a capsule of
//! the control height in `fill` with the glass edge that clips two press targets flanking the
//! value. A target is h + 4 wide and fills the inside of the edge, rounded on its outer side
//! only; its glyph (title2 at md, weight 400) takes the tint (default `label`) and dims to 0.30
//! at the end of the range. The value sits between two `separator` hairlines, at least
//! round(h × 1.06) wide, in the control text style at weight 600. Compact packs round targets
//! (inner − 8) and the value tightly (gap 2, padding 0 × 4, no hairlines).
//!
//! A pure builder producing a `LayoutNode::Stack` row. The −/+ glyphs default to the widget's
//! own text; caller nodes replace them (with their own interaction ids), so the app maps the
//! tapped end → value change. Hover never repaints it; a disabled stepper dims to 0.45.
//! DSL-emittable.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx, Justify,
    LayoutNode, Overflow, Stack, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::{Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, TextStyleToken, Tokens, HAIRLINE};

/// A press target at the end of its range dims to this (the artifacts' 0.3).
const AT_LIMIT: u32 = 77;

/// The geometry of a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepperMetrics {
    /// The press target's width (h + 4; compact: inner − 8, a circle).
    pub button: FxPx,
    /// The value's minimum width.
    pub value_min: FxPx,
    /// The −/+ glyph's text style.
    pub glyph: TextStyleToken,
}

impl StepperMetrics {
    #[must_use]
    pub fn of(size: ControlSize, compact: bool) -> Self {
        let h = size.height().0;
        let inner = h - 2;
        let (glyph, compact_glyph) = match size {
            ControlSize::Xs => (TextStyleToken::Headline, TextStyleToken::Subheadline),
            ControlSize::Sm => (TextStyleToken::Title3, TextStyleToken::Headline),
            ControlSize::Md => (TextStyleToken::Title2, TextStyleToken::Title3),
            ControlSize::Lg => (TextStyleToken::Title2, TextStyleToken::Title3),
            ControlSize::Xl => (TextStyleToken::Title1, TextStyleToken::Title2),
        };
        let font = i32::from(size.label().0.spec().size);
        if compact {
            StepperMetrics {
                button: FxPx::new(inner - 8),
                value_min: FxPx::new((font * 16 + 5) / 10),
                glyph: compact_glyph,
            }
        } else {
            StepperMetrics {
                button: FxPx::new(h + 4),
                value_min: FxPx::new((h * 106 + 50) / 100),
                glyph,
            }
        }
    }
}

/// A −/+ stepper.
#[derive(Debug, Clone, Default)]
pub struct Stepper {
    dec: Option<LayoutNode>,
    value: Option<LayoutNode>,
    value_text: Option<String>,
    inc: Option<LayoutNode>,
    at_min: bool,
    at_max: bool,
    compact: bool,
    size: ControlSize,
    tint: Option<ColorToken>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Stepper {
    pub fn new() -> Self {
        Self::default()
    }

    /// A caller-built decrement glyph (with its own id) in place of the widget's "−".
    pub fn dec(mut self, dec: LayoutNode) -> Self {
        self.dec = Some(dec);
        self
    }

    /// A caller-built value node in place of the widget's value text.
    pub fn value(mut self, value: LayoutNode) -> Self {
        self.value = Some(value);
        self
    }

    /// The formatted value, set by the widget (control text style, weight 600, `label`).
    pub fn value_text(mut self, value: impl Into<String>) -> Self {
        self.value_text = Some(value.into());
        self
    }

    /// A number within `min..=max`: its text, and the ends of the range dim their target.
    pub fn number(mut self, value: i64, min: i64, max: i64) -> Self {
        self.value_text = Some(value.to_string());
        self.at_min = value <= min;
        self.at_max = value >= max;
        self
    }

    /// A caller-built increment glyph (with its own id) in place of the widget's "+".
    pub fn inc(mut self, inc: LayoutNode) -> Self {
        self.inc = Some(inc);
        self
    }

    /// The value is at the bottom of its range (the − target dims to 0.30).
    pub fn at_min(mut self, at_min: bool) -> Self {
        self.at_min = at_min;
        self
    }

    /// The value is at the top of its range (the + target dims to 0.30).
    pub fn at_max(mut self, at_max: bool) -> Self {
        self.at_max = at_max;
        self
    }

    /// Round targets packed tightly into the capsule (quantity pickers in lists).
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// The −/+ glyphs' colour role (default `label`).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    fn text(text: String, style: TextStyle) -> LayoutNode {
        LayoutNode::Text(
            TextNode {
                content: TextContent::new(text),
                style,
                max_lines: Some(1),
                ..TextNode::default()
            },
            VisualStyle::default(),
        )
    }

    fn glyph(&self, tokens: &dyn Tokens, glyph: &str) -> LayoutNode {
        let m = StepperMetrics::of(self.size, self.compact);
        let ink = tokens.color(self.tint.unwrap_or(ColorToken::Label));
        let style = TextStyle {
            white_space: WhiteSpace::NoWrap,
            ..m.glyph.text_style(ink, Some(FontWeight::Normal))
        };
        Self::text(String::from(glyph), style)
    }

    /// A press target: W × inner (compact: a W circle), rounded on its outer side.
    fn target(&self, child: LayoutNode, start: bool, at_limit: bool) -> LayoutNode {
        let m = StepperMetrics::of(self.size, self.compact);
        let full = RadiusToken::Full.px();
        let corners = if self.compact {
            CornerRadius::uniform(full)
        } else if start {
            CornerRadius { top_left: full, bottom_left: full, ..CornerRadius::none() }
        } else {
            CornerRadius { top_right: full, bottom_right: full, ..CornerRadius::none() }
        };
        let height = if self.compact { m.button } else { FxPx::new(self.size.height().0 - 2) };
        let visual = VisualStyle {
            corner_radius: corners,
            opacity: at_limit.then(|| nexus_layout_types::Fraction::new(AT_LIMIT)),
            ..VisualStyle::default()
        };
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(m.button),
                max_width: Some(m.button),
                min_height: Some(height),
                max_height: Some(height),
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            visual,
            alloc::vec![child],
        )
    }

    fn value_cell(&self, tokens: &dyn Tokens, child: LayoutNode) -> LayoutNode {
        let m = StepperMetrics::of(self.size, self.compact);
        let separator = tokens.color(ColorToken::Separator);
        let border = if self.compact {
            EdgeBorder::none()
        } else {
            let edge = Some(nexus_layout_types::Border { width: HAIRLINE, color: separator });
            EdgeBorder { left: edge, right: edge, ..EdgeBorder::none() }
        };
        let h = Some(FxPx::new(self.size.height().0 - 2));
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(
                    FxPx::ZERO,
                    FxPx::new(if self.compact { 2 } else { 4 }),
                ),
                align: Align::Center,
                justify: Justify::Center,
                min_width: Some(m.value_min),
                min_height: h,
                max_height: h,
                ..Stack::default()
            },
            VisualStyle { border, ..VisualStyle::default() },
            alloc::vec![child],
        )
    }

    /// Build the stepper node.
    pub fn build(mut self, tokens: &dyn Tokens) -> LayoutNode {
        let mut style = Style::new()
            .background_token(tokens, ColorToken::Fill)
            .border_token(tokens, ColorToken::GlassEdge)
            .rounded_token(RadiusToken::Full);
        if self.state.shows_focus_ring() {
            style = style.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            style = style.opacity(self.state.opacity_as(Dim::Control));
        }
        let value_style = TextStyle {
            white_space: WhiteSpace::NoWrap,
            ..self
                .size
                .label()
                .0
                .text_style(tokens.color(ColorToken::Label), Some(FontWeight::Semibold))
        };
        let dec = self.dec.take().unwrap_or_else(|| self.glyph(tokens, "\u{2212}"));
        let inc = self.inc.take().unwrap_or_else(|| self.glyph(tokens, "+"));
        let value = match (self.value.take(), self.value_text.take()) {
            (Some(node), _) => Some(node),
            (None, Some(text)) => Some(Self::text(text, value_style)),
            (None, None) => None,
        };
        let mut children: Vec<LayoutNode> = alloc::vec![self.target(dec, true, self.at_min)];
        if let Some(value) = value {
            children.push(self.value_cell(tokens, value));
        }
        children.push(self.target(inc, false, self.at_max));
        let h = Some(self.size.height());
        // Inside the 1 px edge (an edge takes no layout space here); compact pads 0 × 4 more.
        let padding = if self.compact {
            EdgeInsets::symmetric(FxPx::new(1), FxPx::new(5))
        } else {
            EdgeInsets::all(FxPx::new(1))
        };
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(if self.compact { 2 } else { 0 }),
                padding,
                align: Align::Center,
                overflow: Overflow::Hidden,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            style.visual(),
            children,
        )
    }
}

#[cfg(test)]
mod tests;
