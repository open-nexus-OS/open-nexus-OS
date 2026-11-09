// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `TextArea` — the design-system multiline input (design artifacts `TextArea`): an
//! optional label, the field box sized for `rows` lines of the field text (per-size vertical and
//! horizontal padding, the field surface, the size radius or the one-line capsule radius, the
//! focus outline) and a footer with the helper / error / success message and the `n/max`
//! counter. A pure builder producing a `LayoutNode::Stack` column; the value renders as
//! multiline text (the app owns it — a multiline edit primitive is not in the layout yet).
//! DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (textarea)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, Overflow, Stack,
    TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, ControlSize, TextStyleToken, Tokens, HAIRLINE};
use nexus_widget_text_field::field::{
    self, focus_ring, FieldAppearance, FieldMetrics, FieldState, DISABLED_OPACITY, RING_OFFSET,
};
use nexus_widget_text_field::MIN_FIELD_WIDTH;

/// A multiline text input.
#[derive(Debug, Clone)]
pub struct TextArea {
    label: Option<String>,
    value: String,
    placeholder: Option<String>,
    helper: Option<String>,
    error: Option<String>,
    validation: FieldState,
    rows: u32,
    max_length: Option<u32>,
    show_count: bool,
    auto_grow: bool,
    read_only: bool,
    capsule: bool,
    size: ControlSize,
    appearance: FieldAppearance,
    tint: ColorToken,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self {
            label: None,
            value: String::new(),
            placeholder: None,
            helper: None,
            error: None,
            validation: FieldState::Default,
            rows: 4,
            max_length: None,
            show_count: false,
            auto_grow: false,
            read_only: false,
            capsule: false,
            size: ControlSize::Md,
            appearance: FieldAppearance::Filled,
            tint: ColorToken::Accent,
            state: InteractionState::Default,
            id: None,
        }
    }
}

impl TextArea {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
    pub fn helper(mut self, helper: impl Into<String>) -> Self {
        self.helper = Some(helper.into());
        self
    }
    /// Error message — the state becomes `Error` and the message leads the footer.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
    /// Validation state without a message (danger or success border).
    pub fn validation(mut self, state: FieldState) -> Self {
        self.validation = state;
        self
    }
    /// Visible lines (default 4).
    pub fn rows(mut self, rows: u32) -> Self {
        self.rows = rows.max(1);
        self
    }
    /// Character limit; enables the counter denominator.
    pub fn max_length(mut self, max: u32) -> Self {
        self.max_length = Some(max);
        self
    }
    /// Show an `n/limit` counter (requires `max_length`).
    pub fn show_count(mut self, show: bool) -> Self {
        self.show_count = show;
        self
    }
    /// The box grows with the text instead of clipping it at `rows`.
    pub fn auto_grow(mut self, on: bool) -> Self {
        self.auto_grow = on;
        self
    }
    /// Read-only: no fill, a `separator` hairline.
    pub fn read_only(mut self, on: bool) -> Self {
        self.read_only = on;
        self
    }
    /// The one-line capsule radius (a message bubble) instead of the size radius.
    pub fn capsule(mut self, on: bool) -> Self {
        self.capsule = on;
        self
    }
    /// The control size (field text, padding and line height follow it).
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    /// The box surface: filled (default) or the glass card layer.
    pub fn appearance(mut self, appearance: FieldAppearance) -> Self {
        self.appearance = appearance;
        self
    }
    /// The tint (focus outline; default `Accent`).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = tint;
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

    /// The validation state in effect: an error message always wins.
    pub fn field_state(&self) -> FieldState {
        if self.error.is_some() {
            FieldState::Error
        } else {
            self.validation
        }
    }

    /// The box's vertical padding: 4 / 4 / 8 / 12 / 12.
    pub fn pad_y(&self) -> i32 {
        match self.size {
            ControlSize::Xs | ControlSize::Sm => 4,
            ControlSize::Md => 8,
            ControlSize::Lg | ControlSize::Xl => 12,
        }
    }

    /// The box's minimum height: `rows × line + 2 × padY + 2` (the 1 px border on both edges).
    pub fn min_box_height(&self) -> i32 {
        let line = FieldMetrics::of(self.size).line();
        self.rows as i32 * line + 2 * self.pad_y() + 2
    }

    /// The box radius: the size radius, or half the one-line box (`(line + 2·padY + 2) / 2`).
    pub fn radius(&self) -> FxPx {
        if self.capsule {
            FxPx::new((FieldMetrics::of(self.size).line() + 2 * self.pad_y() + 2) / 2)
        } else {
            self.size.field_radius().px()
        }
    }

    /// The footer: message (state colour) and the counter, footnote, inset 2 px.
    fn footer(&self, state: FieldState, tokens: &dyn Tokens) -> Option<LayoutNode> {
        let message = self.error.as_ref().or(self.helper.as_ref());
        let counter = (self.show_count && self.max_length.is_some()).then(|| {
            let n = self.value.chars().count();
            let style =
                TextStyleToken::Footnote.text_style(tokens.color(ColorToken::LabelSecondary), None);
            field::text_node(format!("{}/{}", n, self.max_length.unwrap_or(0)), style)
        });
        if message.is_none() && counter.is_none() {
            return None;
        }
        let mut kids: Vec<LayoutNode> = Vec::new();
        if let Some(message) = message {
            let style = TextStyleToken::Footnote.text_style(tokens.color(state.message()), None);
            kids.push(field::text_node(message.as_str(), style));
        }
        let mut row = Stack {
            direction: Direction::Row,
            gap: FxPx::new(8),
            align: Align::Center,
            justify: Justify::SpaceBetween,
            ..Stack::default()
        };
        row.item.margin.left = FxPx::new(2);
        if let Some(counter) = counter {
            if kids.is_empty() {
                // `margin-left: auto`: a lone counter still sits at the end.
                row.justify = Justify::End;
            }
            kids.push(counter);
        }
        Some(LayoutNode::Stack(row, VisualStyle::default(), kids))
    }

    /// Build the text-area column (label · box · footer).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = FieldMetrics::of(self.size);
        let state = self.field_state();
        let radius = self.radius();
        let style = field::surface(self.appearance, state, self.read_only, tokens).rounded(radius);
        // Body: the value in `label`, or the placeholder in `labelTertiary`.
        let (body_text, body_color) = if self.value.is_empty() {
            (self.placeholder.clone().unwrap_or_default(), ColorToken::LabelTertiary)
        } else {
            (self.value.clone(), ColorToken::Label)
        };
        let body = LayoutNode::Text(
            TextNode {
                id: self.id,
                content: TextContent::new(body_text),
                style: TextStyle {
                    white_space: WhiteSpace::Normal,
                    ..m.text.text_style(tokens.color(body_color), Some(FontWeight::Medium))
                },
                max_lines: (!self.auto_grow).then_some(self.rows),
                ..TextNode::default()
            },
            VisualStyle::default(),
        );
        let pad_y = FxPx::new(self.pad_y());
        let padding = EdgeInsets::symmetric(pad_y + HAIRLINE, m.pad + HAIRLINE);
        let mut kids = alloc::vec![body];
        if self.state.shows_focus_ring() {
            kids.push(focus_ring(
                tokens.color(state.ring(self.tint)),
                radius,
                padding,
                RING_OFFSET,
            ));
        }
        let field_box = LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                padding,
                align: Align::Stretch,
                overflow: if self.auto_grow { Overflow::Visible } else { Overflow::Hidden },
                min_width: Some(FxPx::new(MIN_FIELD_WIDTH)),
                min_height: Some(FxPx::new(self.min_box_height())),
                ..Stack::default()
            },
            style.visual(),
            kids,
        );
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(label) = &self.label {
            col.push(field::label_node(label, &m, tokens));
        }
        col.push(field_box);
        if let Some(footer) = self.footer(state, tokens) {
            col.push(footer);
        }
        let mut visual = VisualStyle::default();
        if self.state.is_disabled() {
            visual.opacity = Some(DISABLED_OPACITY);
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                gap: FxPx::new(8),
                align: Align::Stretch,
                item: FlexItem::default(),
                ..Stack::default()
            },
            visual,
            col,
        )
    }
}

#[cfg(test)]
mod tests;
