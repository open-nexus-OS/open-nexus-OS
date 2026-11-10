// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `GlassTextField` — the design-system text field (design artifacts `TextField`): an
//! optional label, the field box on the control height of its size (leading icon · prefix ·
//! input · suffix · clear disc · reveal eye · success check · trailing node) and an optional
//! helper / error / success line. Resting surface = the control fill (or the glass card layer on
//! a wallpaper); a validation state recolours the 1 px border; focus draws the 2 px outline
//! outside the box. A pure builder producing a `LayoutNode::Stack` column; the inner input is
//! the low-level [`TextField`] primitive (the app owns the value). DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (textfield_*); dsl runtime greeter_surface
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Overflow, Rgba8, Stack, VisualStyle,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, Tokens};

use crate::field::{
    self, box_padding, field_radius, focus_ring, glyph, FieldAppearance, FieldMetrics, FieldState,
    DISABLED_OPACITY, RING_OFFSET, STROKE_2, STROKE_2_5,
};
use crate::TextField;

/// Size preset — the control sizes (design `TextFieldProps.size`): xs 24 · sm 30 · md 36 ·
/// lg 44 · xl 52.
pub type FieldSize = ControlSize;

/// The narrowest a field box gets, so a field in a row never collapses to its placeholder.
pub const MIN_FIELD_WIDTH: i32 = 180;

/// How the field draws its own box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldVariant {
    /// The design's `filled` field: the control fill, 1 px transparent border, size radius.
    #[default]
    Boxed,
    /// The design's `glass` field (on a wallpaper or a blur surface — the lock screen): the
    /// glass card layer, its edge, the edge highlight and `shadow1`. Never blurs.
    Glass,
    /// NO box at all (a repo composition aid): just the input run on the control height. For
    /// composing a field INTO a surrounding surface (a pill that also holds a submit button),
    /// where a second box inside the first would double every border.
    Bare,
}

/// A labelled text field.
#[derive(Debug, Clone)]
pub struct GlassTextField {
    label: Option<String>,
    value: String,
    placeholder: Option<String>,
    helper: Option<String>,
    error: Option<String>,
    validation: FieldState,
    leading: Option<LayoutNode>,
    trailing: Option<LayoutNode>,
    prefix: Option<String>,
    suffix: Option<String>,
    size: FieldSize,
    state: InteractionState,
    id: Option<&'static str>,
    secure: bool,
    revealable: bool,
    revealed: bool,
    reveal_id: Option<&'static str>,
    clearable: bool,
    clear_id: Option<&'static str>,
    variant: FieldVariant,
    capsule: bool,
    read_only: bool,
    tint: ColorToken,
    max_length: Option<u32>,
}

impl Default for GlassTextField {
    fn default() -> Self {
        Self {
            label: None,
            value: String::new(),
            placeholder: None,
            helper: None,
            error: None,
            validation: FieldState::Default,
            leading: None,
            trailing: None,
            prefix: None,
            suffix: None,
            size: FieldSize::Md,
            state: InteractionState::Default,
            id: None,
            secure: false,
            revealable: false,
            revealed: false,
            reveal_id: None,
            clearable: false,
            clear_id: None,
            variant: FieldVariant::Boxed,
            capsule: false,
            read_only: false,
            tint: ColorToken::Accent,
            max_length: None,
        }
    }
}

impl GlassTextField {
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
    /// Helper text under the field (green while the state is `Success`).
    pub fn helper(mut self, helper: impl Into<String>) -> Self {
        self.helper = Some(helper.into());
        self
    }
    /// Error message — the state becomes `Error` (danger border) and the message replaces the
    /// helper line.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
    /// Validation state without a message: `Error` = the danger border only, `Success` = the
    /// green border, the check and a green helper.
    pub fn validation(mut self, state: FieldState) -> Self {
        self.validation = state;
        self
    }
    /// Leading node (an icon, in `labelSecondary`).
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(trailing);
        self
    }
    /// Text before the input (field text, weight 500, `labelSecondary`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
    /// Text after the input (a unit, a currency sign).
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }
    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
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
    /// Password field: bullets instead of the value; no IME preview/learning (RFC-0075).
    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }
    /// Show the eye button on a password field (opt-in: the app wires the toggle by
    /// [`reveal_id`](Self::reveal_id) and passes the result back as [`revealed`](Self::revealed)).
    pub fn revealable(mut self, on: bool) -> Self {
        self.revealable = on;
        self
    }
    /// The password is shown in clear (still a secure field for the IME).
    pub fn revealed(mut self, on: bool) -> Self {
        self.revealed = on;
        self
    }
    /// Interaction id of the eye button.
    pub fn reveal_id(mut self, id: &'static str) -> Self {
        self.reveal_id = Some(id);
        self
    }
    /// Show the clear disc while the field holds text (editable fields only).
    pub fn clearable(mut self, on: bool) -> Self {
        self.clearable = on;
        self
    }
    /// Interaction id of the clear disc (the app empties the value).
    pub fn clear_id(mut self, id: &'static str) -> Self {
        self.clear_id = Some(id);
        self
    }
    /// How the field draws its own box — see [`FieldVariant`].
    pub fn variant(mut self, variant: FieldVariant) -> Self {
        self.variant = variant;
        self
    }
    /// A capsule box instead of the size radius — the lock-screen password exception.
    pub fn capsule(mut self, on: bool) -> Self {
        self.capsule = on;
        self
    }
    /// The lock-screen pill: the glass field as a capsule.
    pub fn pill(self) -> Self {
        self.variant(FieldVariant::Glass).capsule(true)
    }
    /// Read-only: no fill, a `separator` hairline, no clear disc.
    pub fn read_only(mut self, on: bool) -> Self {
        self.read_only = on;
        self
    }
    /// The tint (focus outline; default `Accent`).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = tint;
        self
    }
    /// Maximum input length.
    pub fn max_length(mut self, max: u32) -> Self {
        self.max_length = Some(max);
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

    /// The visible 1 px border for the current state (`None` = transparent at rest).
    pub fn border_color(&self, tokens: &dyn Tokens) -> Option<Rgba8> {
        let appearance = self.appearance()?;
        let v = field::surface(appearance, self.field_state(), self.read_only, tokens).visual();
        v.border.top.map(|b| b.color)
    }

    /// The focus outline's colour: the tint, or the state colour while invalid / valid.
    pub fn ring_color(&self, tokens: &dyn Tokens) -> Rgba8 {
        tokens.color(self.field_state().ring(self.tint))
    }

    /// The surface the variant draws (`None` for `Bare`).
    fn appearance(&self) -> Option<FieldAppearance> {
        match self.variant {
            FieldVariant::Boxed => Some(FieldAppearance::Filled),
            FieldVariant::Glass => Some(FieldAppearance::Glass),
            FieldVariant::Bare => None,
        }
    }

    fn editable(&self) -> bool {
        !self.read_only && !self.state.is_disabled()
    }

    /// The eye button: a transparent circle (`icon + 8`) pulled 4 px into its neighbours'
    /// gaps, the eye (masked) or the struck eye (revealed) in `labelSecondary`.
    fn reveal_button(&self, m: &FieldMetrics, tokens: &dyn Tokens) -> LayoutNode {
        let paths = if self.revealed { field::EYE_OFF } else { field::EYE };
        let eye = glyph(paths, STROKE_2, m.icon, tokens.color(ColorToken::LabelSecondary));
        let d = Some(FxPx::new(m.reveal_button()));
        LayoutNode::Stack(
            Stack {
                id: self.reveal_id,
                align: Align::Center,
                justify: Justify::Center,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem {
                    flex_shrink: 0,
                    margin: EdgeInsets {
                        left: FxPx::new(-4),
                        right: FxPx::new(-4),
                        ..EdgeInsets::zero()
                    },
                    ..FlexItem::default()
                },
                ..Stack::default()
            },
            VisualStyle {
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![eye],
        )
    }

    fn affix(text: &str, m: &FieldMetrics, tokens: &dyn Tokens) -> LayoutNode {
        let style =
            m.text.text_style(tokens.color(ColorToken::LabelSecondary), Some(FontWeight::Medium));
        field::text_node(text, style)
    }

    /// The input box's children, in the design's order.
    fn box_children(&self, m: &FieldMetrics, tokens: &dyn Tokens) -> Vec<LayoutNode> {
        let mut out: Vec<LayoutNode> = Vec::new();
        if let Some(leading) = &self.leading {
            out.push(leading.clone());
        }
        if let Some(prefix) = &self.prefix {
            out.push(Self::affix(prefix, m, tokens));
        }
        let mut tf = TextField::new()
            .text_style(field::input_style(m, tokens))
            .value(self.value.clone())
            .fill_row()
            .secure(self.secure)
            .revealed(self.revealed)
            .caret_color(tokens.color(self.tint));
        if let Some(p) = &self.placeholder {
            // The design's placeholder role; the lock screen's capsule field reads on the
            // wallpaper and takes the stronger `labelSecondary`.
            let role =
                if self.capsule { ColorToken::LabelSecondary } else { ColorToken::LabelTertiary };
            tf = tf.placeholder(p.clone()).placeholder_color(tokens.color(role));
        }
        if let Some(id) = self.id {
            tf = tf.id(id);
        }
        if let Some(max) = self.max_length {
            tf = tf.max_length(max);
        }
        out.push(tf.build());
        if let Some(suffix) = &self.suffix {
            out.push(Self::affix(suffix, m, tokens));
        }
        if self.clearable && self.editable() && !self.value.is_empty() {
            out.push(field::clear_disc(m.clear_disc(), self.clear_id, tokens));
        }
        if self.secure && self.revealable {
            out.push(self.reveal_button(m, tokens));
        }
        if self.field_state() == FieldState::Success && self.trailing.is_none() {
            out.push(glyph(field::CHECK, STROKE_2_5, m.icon, tokens.color(ColorToken::Success)));
        }
        if let Some(trailing) = &self.trailing {
            out.push(trailing.clone());
        }
        out
    }

    /// Build the field column (label · input box · message).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = FieldMetrics::of(self.size);
        let state = self.field_state();
        let radius = field_radius(self.size, self.capsule);
        // `Bare` draws no box — the caller's surface is the box — and so reserves no border.
        let (box_style, padding) = match self.appearance() {
            Some(appearance) => (
                field::surface(appearance, state, self.read_only, tokens).rounded(radius),
                box_padding(m.pad, FxPx::ZERO),
            ),
            None => (Style::new(), EdgeInsets::symmetric(FxPx::ZERO, m.pad)),
        };
        let mut children = self.box_children(&m, tokens);
        if self.state.shows_focus_ring() && self.appearance().is_some() {
            children.push(focus_ring(self.ring_color(tokens), radius, padding, RING_OFFSET));
        }
        let field_box = LayoutNode::Stack(
            Stack {
                direction: Direction::Row,
                gap: m.gap,
                padding,
                align: Align::Center,
                overflow: Overflow::Visible,
                min_width: Some(FxPx::new(MIN_FIELD_WIDTH)),
                min_height: Some(m.height),
                max_height: Some(m.height),
                ..Stack::default()
            },
            box_style.visual(),
            children,
        );

        // Column: label? / field box / message?
        // `Align::Stretch` (not `Start`) is what carries a `.grow(1)` from the page down to the
        // field box: `.grow()` lands on this outer column only, and a `Start`-aligned column
        // leaves its children at their intrinsic width — so a field told to fill a 663px row
        // painted its chrome at the minimum width and left the rest of the strip inert.
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(label) = &self.label {
            col.push(field::label_node(label, &m, tokens));
        }
        col.push(field_box);
        if let Some(message) = self.error.as_ref().or(self.helper.as_ref()) {
            col.push(field::message_node(message, state, tokens));
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
                ..Stack::default()
            },
            visual,
            col,
        )
    }
}

#[cfg(test)]
#[path = "glass_text_field_tests.rs"]
mod tests;
