// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `PinField` — passcode / one-time-code entry (design artifacts `PinField`): 4–8
//! passcode dots that fill as digits arrive (`mask`) or digit cells on the field surface, the
//! focus outline around the dot row or on the active cell, the danger state and its message.
//! One invisible text input lies over the row, so the keyboard, Backspace and paste route to
//! the field as to any input (the app owns the value). Lives beside `GlassTextField` because it
//! is built from the same field kit. The dot pop, the digit pop, the caret blink and the error
//! shake are motion the host does not have yet (TASK-0353 P7 report).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, Fraction, FxPx,
    Justify, LayoutNode, LineHeight, Position, Rgba8, Stack, TextAlign, TextStyle, VisualStyle,
};
use nexus_style::InteractionState;
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, TextStyleToken, Tokens};

use crate::field::{self, focus_ring, FieldAppearance, FieldState, RING_OFFSET};
use crate::TextField;

/// Opacity of a disabled PIN entry (design: 0.45).
pub const PIN_DISABLED_OPACITY: Fraction = Fraction::new(115);

/// The passcode dot's ring (the design's 1.5 px inset ring, on the whole-pixel border).
const DOT_RING: i32 = 2;

/// A passcode / one-time-code field.
#[derive(Debug, Clone)]
pub struct PinField {
    length: usize,
    value: String,
    mask: bool,
    alphanumeric: bool,
    error: Option<String>,
    invalid: bool,
    size: ControlSize,
    appearance: FieldAppearance,
    capsule: bool,
    tint: Option<ColorToken>,
    state: InteractionState,
    id: Option<&'static str>,
}

impl Default for PinField {
    fn default() -> Self {
        Self {
            length: 4,
            value: String::new(),
            mask: true,
            alphanumeric: false,
            error: None,
            invalid: false,
            size: ControlSize::Md,
            appearance: FieldAppearance::Filled,
            capsule: false,
            tint: None,
            state: InteractionState::Default,
            id: None,
        }
    }
}

impl PinField {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of cells, 4–8 (default 4).
    pub fn length(mut self, length: usize) -> Self {
        self.length = length.clamp(4, 8);
        self
    }
    /// The entered code (app-owned); cleaned to the allowed characters and cut to `length`.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }
    /// Passcode dots (default) or visible digit cells.
    pub fn mask(mut self, mask: bool) -> Self {
        self.mask = mask;
        self
    }
    /// Accept A–Z too (shown upper-case).
    pub fn alphanumeric(mut self, on: bool) -> Self {
        self.alphanumeric = on;
        self
    }
    /// A wrong code with a message under the row (danger dots / cells).
    pub fn error(mut self, message: impl Into<String>) -> Self {
        self.error = Some(message.into());
        self
    }
    /// A wrong code without a message: only the colour.
    pub fn invalid(mut self, on: bool) -> Self {
        self.invalid = on;
        self
    }
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    /// The cells' surface (filled, or glass on a wallpaper).
    pub fn appearance(mut self, appearance: FieldAppearance) -> Self {
        self.appearance = appearance;
        self
    }
    /// Capsule cells instead of the size radius.
    pub fn capsule(mut self, on: bool) -> Self {
        self.capsule = on;
        self
    }
    /// The tint (default: `Label` for dots, `Accent` for cells).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }
    pub fn state(mut self, state: InteractionState) -> Self {
        self.state = state;
        self
    }
    /// Interaction id of the input lying over the row.
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// `raw` cut to what the field accepts: digits (or A–Z upper-cased when alphanumeric), at
    /// most `length` characters.
    pub fn clean(&self, raw: &str) -> String {
        raw.chars()
            .filter(|c| c.is_ascii_digit() || (self.alphanumeric && c.is_ascii_alphabetic()))
            .map(|c| c.to_ascii_uppercase())
            .take(self.length)
            .collect()
    }

    /// Whether every cell holds a character (the design's `onComplete` moment).
    pub fn is_complete(&self) -> bool {
        self.clean(&self.value).chars().count() == self.length
    }

    fn has_error(&self) -> bool {
        self.error.is_some() || self.invalid
    }

    /// The dot diameter: `round(control font × 0.95)` — 10 / 11 / 13 / 14 / 16.
    pub fn dot_size(&self) -> i32 {
        let font = i32::from(self.size.label().0.spec().size);
        (font * 95 + 50) / 100
    }

    /// The cell (width, height): height = control height + 14, width = `round(h × 0.84)`.
    pub fn cell_size(&self) -> (i32, i32) {
        let h = self.size.height().0 + 14;
        ((h * 84 + 50) / 100, h)
    }

    fn big(&self) -> bool {
        matches!(self.size, ControlSize::Lg | ControlSize::Xl)
    }

    fn gap(&self) -> i32 {
        match (self.mask, self.big()) {
            (true, true) => 24,
            (true, false) => 20,
            (false, true) => 12,
            (false, false) => 8,
        }
    }

    fn digit_style(&self) -> TextStyleToken {
        match self.size {
            ControlSize::Xs => TextStyleToken::Headline,
            ControlSize::Sm => TextStyleToken::Title3,
            ControlSize::Md => TextStyleToken::Title2,
            ControlSize::Lg => TextStyleToken::Title1,
            ControlSize::Xl => TextStyleToken::LargeTitle,
        }
    }

    fn dot(&self, filled: bool, tint: Rgba8) -> LayoutNode {
        let d = Some(FxPx::new(self.dot_size()));
        LayoutNode::Stack(
            Stack { min_width: d, max_width: d, min_height: d, max_height: d, ..Stack::default() },
            VisualStyle {
                background: filled.then_some(tint),
                border: EdgeBorder::all(FxPx::new(DOT_RING), tint),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            Vec::new(),
        )
    }

    fn cell(&self, ch: Option<char>, active: bool, tint: Rgba8, tokens: &dyn Tokens) -> LayoutNode {
        let (w, h) = self.cell_size();
        let radius = field::field_radius(self.size, self.capsule);
        let state = if self.has_error() { FieldState::Error } else { FieldState::Default };
        let style = field::surface(self.appearance, state, false, tokens).rounded(radius);
        let mut kids: Vec<LayoutNode> = Vec::new();
        if let Some(ch) = ch {
            let color =
                tokens.color(if self.has_error() { ColorToken::Danger } else { ColorToken::Label });
            let digit = TextStyle {
                text_align: TextAlign::Center,
                ..self.digit_style().text_style(color, Some(FontWeight::Semibold))
            };
            let mut s = String::new();
            s.push(ch);
            kids.push(field::text_node(s, digit));
        } else if active {
            // The empty active cell's caret: 2 × round(h × 0.42) in the tint.
            let caret_h = Some(FxPx::new((h * 42 + 50) / 100));
            let caret_w = Some(FxPx::new(2));
            kids.push(LayoutNode::Stack(
                Stack {
                    min_width: caret_w,
                    max_width: caret_w,
                    min_height: caret_h,
                    max_height: caret_h,
                    ..Stack::default()
                },
                VisualStyle {
                    background: Some(tint),
                    corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                    ..VisualStyle::default()
                },
                Vec::new(),
            ));
        }
        if active {
            kids.push(focus_ring(tint, radius, EdgeInsets::zero(), RING_OFFSET));
        }
        let (w, h) = (Some(FxPx::new(w)), Some(FxPx::new(h)));
        LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Center,
                min_width: w,
                max_width: w,
                min_height: h,
                max_height: h,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            style.visual(),
            kids,
        )
    }

    /// The invisible input over the row: transparent text (and so a transparent caret), sized
    /// to the row's content box so a tap anywhere on the dots or cells focuses the field.
    fn hidden_input(
        &self,
        value: &str,
        width: i32,
        height: i32,
        tokens: &dyn Tokens,
    ) -> LayoutNode {
        let style = TextStyle {
            color: Rgba8::TRANSPARENT,
            line_height: LineHeight::Absolute(FxPx::new(height)),
            ..field::input_style(&field::FieldMetrics::of(self.size), tokens)
        };
        let mut tf = TextField::new()
            .text_style(style)
            .value(value)
            .secure(self.mask)
            .min_width(width)
            .max_length(self.length as u32);
        if let Some(id) = self.id {
            tf = tf.id(id);
        }
        let mut node = tf.build();
        if let LayoutNode::TextInput(input, _) = &mut node {
            input.max_width = Some(FxPx::new(width));
            input.item.position = Position::Absolute;
        }
        node
    }

    /// Build the field: the dot / cell row, then the error message.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let value = self.clean(&self.value);
        let filled = value.chars().count();
        let tint_role = if self.has_error() {
            ColorToken::Danger
        } else {
            self.tint.unwrap_or(if self.mask { ColorToken::Label } else { ColorToken::Accent })
        };
        let tint = tokens.color(tint_role);
        let focused = self.state.shows_focus_ring();
        let active = filled.min(self.length - 1);
        let gap = self.gap();
        let n = self.length as i32;
        let (unit_w, unit_h) =
            if self.mask { (self.dot_size(), self.dot_size()) } else { self.cell_size() };
        let mut row: Vec<LayoutNode> = Vec::new();
        let mut chars = value.chars();
        for i in 0..self.length {
            let ch = chars.next();
            row.push(if self.mask {
                self.dot(ch.is_some(), tint)
            } else {
                self.cell(ch, focused && i == active, tint, tokens)
            });
        }
        row.push(self.hidden_input(&value, n * unit_w + (n - 1) * gap, unit_h, tokens));
        let padding = if self.mask {
            EdgeInsets::symmetric(FxPx::new(8), FxPx::new(4))
        } else {
            EdgeInsets::zero()
        };
        if self.mask && focused {
            // The ring around the dot row: accent when the dots are `label`, 4 px out.
            let ring = if tint_role == ColorToken::Label { ColorToken::Accent } else { tint_role };
            row.push(focus_ring(tokens.color(ring), RadiusToken::Full.px(), padding, 4));
        }
        let row = LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: FxPx::new(gap),
                padding,
                align: Align::Center,
                ..Stack::default()
            },
            VisualStyle {
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            row,
        );
        let mut col = alloc::vec![row];
        if let Some(message) = &self.error {
            let style = TextStyleToken::Subheadline
                .text_style(tokens.color(ColorToken::Danger), Some(FontWeight::Medium));
            col.push(field::text_node(message.as_str(), style));
        }
        let mut visual = VisualStyle::default();
        if self.state.is_disabled() {
            visual.opacity = Some(PIN_DISABLED_OPACITY);
        }
        LayoutNode::Stack(
            Stack {
                direction: Direction::Column,
                gap: FxPx::new(12),
                align: Align::Center,
                ..Stack::default()
            },
            visual,
            col,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    fn row_of(node: LayoutNode) -> (Stack, Vec<LayoutNode>) {
        match node {
            LayoutNode::Stack(_, _, col) => match col.into_iter().next() {
                Some(LayoutNode::Stack(s, _, kids)) => (s, kids),
                _ => panic!("the row"),
            },
            _ => panic!("the column"),
        }
    }

    #[test]
    fn dots_and_cells_follow_the_size_table() {
        let sizes =
            [ControlSize::Xs, ControlSize::Sm, ControlSize::Md, ControlSize::Lg, ControlSize::Xl];
        let dots: Vec<i32> = sizes.iter().map(|s| PinField::new().size(*s).dot_size()).collect();
        assert_eq!(dots, alloc::vec![10, 11, 13, 14, 16]);
        let cells: Vec<(i32, i32)> =
            sizes.iter().map(|s| PinField::new().size(*s).cell_size()).collect();
        assert_eq!(cells, alloc::vec![(32, 38), (37, 44), (42, 50), (49, 58), (55, 66)]);
    }

    #[test]
    fn the_value_is_cleaned_and_cut_to_the_length() {
        let f = PinField::new().length(6);
        assert_eq!(f.clean("12a-34 5678"), "123456");
        assert_eq!(PinField::new().alphanumeric(true).clean("ab1"), "AB1");
        assert_eq!(PinField::new().length(12).length, 8, "at most 8");
        assert!(PinField::new().value("1234").is_complete());
        assert!(!PinField::new().value("12").is_complete());
    }

    #[test]
    fn dots_fill_as_digits_arrive() {
        let t = BaseTokens;
        let (row, kids) = row_of(PinField::new().value("12").build(&t));
        assert_eq!(row.gap, FxPx::new(20));
        assert_eq!(row.padding, EdgeInsets::symmetric(FxPx::new(8), FxPx::new(4)));
        let fill = |k: &LayoutNode| match k {
            LayoutNode::Stack(_, v, _) => v.background,
            _ => panic!("dot"),
        };
        let label = t.color(ColorToken::Label);
        assert_eq!(fill(&kids[0]), Some(label), "filled in the tint (label)");
        assert_eq!(fill(&kids[2]), None, "an empty dot is the ring only");
        assert_eq!(kids.len(), 5, "4 dots + the input");
        assert_eq!(kids[4].item().position, Position::Absolute);
    }

    #[test]
    fn the_hidden_input_covers_the_row_and_paints_nothing() {
        let (_, kids) = row_of(PinField::new().id("pin").value("1").build(&BaseTokens));
        match &kids[4] {
            LayoutNode::TextInput(i, _) => {
                assert_eq!(i.id, Some("pin"));
                assert!(i.secure, "a passcode is a secret for the IME");
                assert_eq!(i.style.color.a, 0);
                // 4 dots of 13 + 3 gaps of 20.
                assert_eq!(
                    (i.min_width, i.max_width),
                    (Some(FxPx::new(112)), Some(FxPx::new(112)))
                );
            }
            _ => panic!("hidden input"),
        }
    }

    #[test]
    fn an_error_turns_dots_danger_and_adds_the_message() {
        let t = BaseTokens;
        let node = PinField::new().value("1234").error("Falscher Code").build(&t);
        match &node {
            LayoutNode::Stack(_, _, col) => {
                assert_eq!(col.len(), 2);
                match &col[1] {
                    LayoutNode::Text(msg, _) => {
                        assert_eq!(msg.style.color, t.color(ColorToken::Danger));
                        assert_eq!(msg.style.font_weight, FontWeight::Medium);
                    }
                    _ => panic!("message"),
                }
            }
            _ => panic!(),
        }
        let (_, kids) = row_of(node);
        match &kids[0] {
            LayoutNode::Stack(_, v, _) => {
                assert_eq!(v.background, Some(t.color(ColorToken::Danger)))
            }
            _ => panic!(),
        }
    }

    #[test]
    fn cells_show_digits_and_the_active_cell_carries_caret_and_ring() {
        let t = BaseTokens;
        let f = PinField::new().mask(false).value("7").state(InteractionState::Focused);
        let (row, kids) = row_of(f.build(&t));
        assert_eq!(row.gap, FxPx::new(8));
        match &kids[0] {
            LayoutNode::Stack(s, v, k) => {
                assert_eq!((s.min_width, s.min_height), (Some(FxPx::new(42)), Some(FxPx::new(50))));
                assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
                assert_eq!(v.corner_radius.top_left, FxPx::new(12));
                assert!(
                    matches!(&k[0], LayoutNode::Text(d, _) if d.content.as_str() == "7" && d.style.font_size == FxPx::new(20))
                );
            }
            _ => panic!("cell"),
        }
        // The second cell is active: caret (2 × 21) + the ring.
        match &kids[1] {
            LayoutNode::Stack(_, _, k) => {
                assert_eq!(k.len(), 2);
                assert_eq!(k[1].item().position, Position::Absolute);
            }
            _ => panic!("active cell"),
        }
    }

    #[test]
    fn disabled_dims_to_045() {
        match PinField::new().state(InteractionState::Disabled).build(&BaseTokens) {
            LayoutNode::Stack(_, v, _) => assert_eq!(v.opacity, Some(PIN_DISABLED_OPACITY)),
            _ => panic!(),
        }
    }
}
