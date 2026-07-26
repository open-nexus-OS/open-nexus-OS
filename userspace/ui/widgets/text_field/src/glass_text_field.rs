// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `GlassTextField` — the design-system text field (handoff `TextField`): an
//! optional label, a bordered glass input box (leading icon · input · trailing
//! node), and optional helper/error text — all resolved from theme tokens
//! (error turns the border danger; focus shows the ring). A pure builder
//! producing a `LayoutNode::Stack` column. The inner input is the low-level
//! [`TextField`] primitive (app owns the value). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode, LineHeight,
    Overflow, Rgba8, Stack, TextAlign, TextContent, TextNode, TextStyle, VisualStyle, WhiteSpace,
};
use nexus_style::{InteractionState, Style};
use nexus_theme_tokens::{ColorToken, LengthToken, Tokens, TypographyToken};

use crate::TextField;

/// Size preset (handoff `TextFieldProps.size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl FieldSize {
    fn input_size(self) -> TypographyToken {
        match self {
            FieldSize::Sm => TypographyToken::Sm,
            FieldSize::Md => TypographyToken::Base,
            FieldSize::Lg => TypographyToken::Md,
        }
    }
    fn padding(self) -> EdgeInsets {
        let (v, h) = match self {
            FieldSize::Sm => (6, 10),
            FieldSize::Md => (8, 12),
            FieldSize::Lg => (10, 14),
        };
        EdgeInsets::symmetric(FxPx::new(v), FxPx::new(h))
    }
}

/// Height of the glass pill and, as its radius, what makes it a pill
/// (handoff: 44px tall, `border-radius: 9999px`). 44 is also the minimum
/// hit target the handoff requires of every control.
pub const PILL_HEIGHT: i32 = 44;
const PILL_RADIUS: i32 = 9999;

/// How the field draws its own box (RFC-0082).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldVariant {
    /// The design-system default: opaque `Surface`, medium radius, 1px border.
    #[default]
    Boxed,
    /// A glass PILL — `material(panel)`, radius `full`, 44px tall. The lock
    /// surface and search fields that sit on a wallpaper.
    Glass,
    /// NO box at all: just the input run. For composing a field INTO a
    /// surrounding surface (a pill that also holds a submit button), where a
    /// second box inside the first would double every border and blur.
    Bare,
}

/// A labelled text field.
#[derive(Debug, Clone, Default)]
pub struct GlassTextField {
    label: Option<String>,
    value: String,
    placeholder: Option<String>,
    helper: Option<String>,
    error: Option<String>,
    leading: Option<LayoutNode>,
    trailing: Option<LayoutNode>,
    size: FieldSize,
    state: InteractionState,
    id: Option<&'static str>,
    secure: bool,
    variant: FieldVariant,
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
    pub fn helper(mut self, helper: impl Into<String>) -> Self {
        self.helper = Some(helper.into());
        self
    }
    /// Error message — turns the border danger and replaces the helper line.
    pub fn error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
    pub fn leading(mut self, leading: LayoutNode) -> Self {
        self.leading = Some(leading);
        self
    }
    pub fn trailing(mut self, trailing: LayoutNode) -> Self {
        self.trailing = Some(trailing);
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
    /// Password field: bullets instead of the value; no IME preview/learning
    /// (RFC-0075).
    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }
    /// How the field draws its own box — see [`FieldVariant`].
    pub fn variant(mut self, variant: FieldVariant) -> Self {
        self.variant = variant;
        self
    }
    /// Sugar for the glass pill (`FieldVariant::Glass`).
    pub fn pill(self) -> Self {
        self.variant(FieldVariant::Glass)
    }

    /// The border color for the current state (danger on error, ring on focus).
    pub fn border_color(&self, tokens: &dyn Tokens) -> Rgba8 {
        let role = if self.error.is_some() {
            ColorToken::Danger
        } else if self.state.shows_focus_ring() {
            ColorToken::FocusRing
        } else {
            ColorToken::Border
        };
        tokens.color(role)
    }

    fn text(content: &str, style: TextStyle) -> LayoutNode {
        LayoutNode::Text(
            TextNode {
                id: None,
                content: TextContent::new(String::from(content)),
                style,
                item: FlexItem::default(),
                max_lines: Some(1),
                min_width: None,
                max_width: None,
            },
            VisualStyle::default(),
        )
    }

    fn caption_style(tokens: &dyn Tokens, color: ColorToken) -> TextStyle {
        TextStyle {
            font_size: tokens.type_size(TypographyToken::Sm),
            font_weight: FontWeight::Regular,
            // RFC-0082: the baked face's own line height. This was
            // `Relative(150)`, which the measurer never honored — now that
            // `Relative` IS honored it would silently grow every field, so
            // the intent is stated as "let the face decide" instead.
            line_height: LineHeight::Absolute(FxPx::new(20)),
            text_align: TextAlign::Left,
            color: tokens.color(color),
            white_space: WhiteSpace::NoWrap,
        }
    }

    /// Build the field column (label · input box · helper/error).
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        // Input box. `Bare` draws none at all — the caller's surface is the
        // box, and a second one inside it would double the border and the
        // backdrop blur.
        let mut box_style = match self.variant {
            FieldVariant::Boxed => Style::new()
                .background(tokens.color(ColorToken::Surface))
                .rounded(tokens.length(LengthToken::RadiusMedium))
                .border(tokens.length(LengthToken::BorderThin), self.border_color(tokens)),
            FieldVariant::Glass => Style::new()
                .glass(nexus_theme_tokens::MaterialToken::Panel, tokens)
                .rounded(FxPx::new(PILL_RADIUS))
                .border(tokens.length(LengthToken::BorderThin), self.border_color(tokens)),
            FieldVariant::Bare => Style::new(),
        };
        if self.state.is_disabled() {
            box_style = box_style.opacity(self.state.opacity());
        }

        let input = {
            let mut tf = TextField::new()
                .style(Style::new())
                .text_style(TextStyle {
                    font_size: tokens.type_size(self.size.input_size()),
                    font_weight: FontWeight::Regular,
                    // RFC-0082: the baked face's own line height. This was
                    // `Relative(150)`, which the measurer never honored — now that
                    // `Relative` IS honored it would silently grow every field, so
                    // the intent is stated as "let the face decide" instead.
                    line_height: LineHeight::Absolute(FxPx::new(20)),
                    text_align: TextAlign::Left,
                    color: tokens.color(ColorToken::OnSurface),
                    white_space: WhiteSpace::NoWrap,
                })
                .value(self.value.clone())
                .secure(self.secure);
            if let Some(p) = &self.placeholder {
                tf = tf.placeholder(p.clone());
            }
            if let Some(id) = self.id {
                tf = tf.id(id);
            }
            tf.build()
        };

        let mut row_children: Vec<LayoutNode> = Vec::new();
        if let Some(leading) = self.leading {
            row_children.push(leading);
        }
        row_children.push(input);
        if let Some(trailing) = self.trailing {
            row_children.push(trailing);
        }
        let field_box = LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: self.size.padding(),
                align: Align::Center,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: Some(FxPx::new(180)),
                max_width: None,
                // A pill is 44px tall — the handoff's height AND the minimum
                // hit target it requires of every control.
                min_height: (self.variant == FieldVariant::Glass).then(|| FxPx::new(PILL_HEIGHT)),
                max_height: None,
                item: FlexItem::default(),
            },
            box_style.visual(),
            row_children,
        );

        // Column: label? / field box / helper-or-error?
        let mut col: Vec<LayoutNode> = Vec::new();
        if let Some(label) = &self.label {
            col.push(Self::text(label, Self::caption_style(tokens, ColorToken::OnSurfaceVariant)));
        }
        col.push(field_box);
        if let Some(err) = &self.error {
            col.push(Self::text(err, Self::caption_style(tokens, ColorToken::Danger)));
        } else if let Some(helper) = &self.helper {
            col.push(Self::text(helper, Self::caption_style(tokens, ColorToken::OnSurfaceVariant)));
        }

        LayoutNode::Stack(
            Stack {
                id: None,
                direction: Direction::Column,
                gap: FxPx::new(4),
                padding: EdgeInsets::zero(),
                align: Align::Start,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            col,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    #[test]
    fn error_turns_border_danger_and_adds_a_line() {
        let t = BaseTokens;
        let f = GlassTextField::new().label("E-Mail").error("Zu kurz");
        assert_eq!(f.border_color(&t), t.color(ColorToken::Danger));
        match f.build(&t) {
            LayoutNode::Stack(_, _, col) => assert_eq!(col.len(), 3, "label + box + error"),
            _ => panic!(),
        }
    }

    #[test]
    fn plain_field_is_label_plus_box() {
        let t = BaseTokens;
        assert_eq!(GlassTextField::new().border_color(&t), t.color(ColorToken::Border));
        match GlassTextField::new().value("hi").build(&t) {
            LayoutNode::Stack(_, _, col) => assert_eq!(col.len(), 1, "just the box"),
            _ => panic!(),
        }
    }

    #[test]
    fn pill_is_glass_full_radius_and_44_tall() {
        let t = BaseTokens;
        match GlassTextField::new().placeholder("Passwort").pill().build(&t) {
            LayoutNode::Stack(_, _, col) => match &col[0] {
                LayoutNode::Stack(stack, v, _) => {
                    assert_eq!(stack.min_height, Some(FxPx::new(PILL_HEIGHT)));
                    assert_eq!(v.corner_radius.top_left, FxPx::new(9999), "radius full = pill");
                    assert!(
                        matches!(v.material, nexus_layout_types::SurfaceMaterial::Glass(_)),
                        "a pill is glass, not an opaque surface"
                    );
                    assert!(v.inset_highlight.is_some(), "the inset top-shine");
                }
                _ => panic!("expected the field box"),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn bare_draws_no_box_at_all() {
        // Composing a field INTO a surrounding pill: a second box inside the
        // first would double the border and the backdrop blur.
        let t = BaseTokens;
        match GlassTextField::new().variant(FieldVariant::Bare).build(&t) {
            LayoutNode::Stack(_, _, col) => match &col[0] {
                LayoutNode::Stack(_, v, _) => {
                    assert_eq!(v.background, None);
                    assert_eq!(v.border, nexus_layout_types::EdgeBorder::none());
                    assert_eq!(v.material, nexus_layout_types::SurfaceMaterial::Opaque);
                }
                _ => panic!("expected the field box"),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn focus_shows_ring_disabled_dims() {
        let t = BaseTokens;
        assert_eq!(
            GlassTextField::new().state(InteractionState::Focused).border_color(&t),
            t.color(ColorToken::FocusRing)
        );
        // Disabled dims the box (first/only child).
        match GlassTextField::new().state(InteractionState::Disabled).build(&t) {
            LayoutNode::Stack(_, _, col) => match &col[0] {
                LayoutNode::Stack(_, v, _) => {
                    assert_eq!(v.opacity, Some(InteractionState::Disabled.opacity()))
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }
}
