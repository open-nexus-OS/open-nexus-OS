// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Select` — the design-system dropdown trigger (the design artifacts' `Select`): a value
//! field of the control height in `fill` with the glass edge, rounded by size (8/8/12/12/16),
//! padded 0 × (padX − 4), at least round(h × 3.3) wide; the value in the control label (`label`)
//! or the placeholder in `labelTertiary`, and a stroked chevron in `labelSecondary` that turns
//! up while open. Open, the trigger carries the persistent `fill` layer.
//!
//! A pure builder producing the trigger `LayoutNode::Stack`; the option panel is an app-owned
//! overlay (the opaque floating surface), so the app opens it on the trigger's `id`. Hover
//! never repaints it; the state draws the disabled dim (0.45) and the focus ring. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Stack, TextContent,
    TextNode, VisualStyle,
};
use nexus_style::control::{label_style, metrics, over};
use nexus_style::{glyph, Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, Tokens};

/// The chevron (`M6 9l6 6 6-6`, stroke 2.2) and its open (turned) form.
const CHEVRON_DOWN: [&[glyph::GridPoint]; 1] = [&[(60, 90), (120, 150), (180, 90)]];
const CHEVRON_UP: [&[glyph::GridPoint]; 1] = [&[(60, 150), (120, 90), (180, 150)]];

/// A dropdown select trigger.
#[derive(Debug, Clone, Default)]
pub struct Select {
    value: Option<String>,
    placeholder: Option<String>,
    state: InteractionState,
    id: Option<&'static str>,
    size: ControlSize,
    open: bool,
}

impl Select {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
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

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// The option panel is open: the persistent `fill` layer, the chevron turned up.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    fn label_node(&self, tokens: &dyn Tokens) -> LayoutNode {
        // The value in `label`; the placeholder in `labelTertiary`.
        let (text, color) = match (&self.value, &self.placeholder) {
            (Some(v), _) => (v.clone(), ColorToken::Label),
            (None, Some(p)) => (p.clone(), ColorToken::LabelTertiary),
            (None, None) => (String::new(), ColorToken::LabelTertiary),
        };
        LayoutNode::Text(
            TextNode {
                content: TextContent::new(text),
                style: label_style(self.size, tokens.color(color)),
                // The value takes the room; the chevron keeps the trailing edge.
                item: FlexItem { flex_grow: 1, ..FlexItem::default() },
                max_lines: Some(1),
                ..TextNode::default()
            },
            VisualStyle::default(),
        )
    }

    fn chevron(&self, tokens: &dyn Tokens) -> LayoutNode {
        let size = FxPx::new((metrics(self.size).icon.0 - 2).max(12));
        let path = if self.open { &CHEVRON_UP } else { &CHEVRON_DOWN };
        glyph::stroke(path, false, 22, size, tokens.color(ColorToken::LabelSecondary))
    }

    /// The resolved trigger [`Style`].
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let fill = tokens.color(ColorToken::Fill);
        let background = if self.open { over(fill, fill) } else { fill };
        let mut s = Style::new()
            .background(background)
            .border_token(tokens, ColorToken::GlassEdge)
            .rounded_token(self.size.field_radius());
        if self.state.shows_focus_ring() {
            s = s.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity_as(Dim::Control));
        }
        s
    }

    /// Build the select trigger node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let m = metrics(self.size);
        let h = m.height;
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(8),
                padding: EdgeInsets::symmetric(FxPx::ZERO, FxPx::new(m.pad_x.0 - 4)),
                align: Align::Center,
                justify: Justify::Start,
                min_width: Some(FxPx::new((h.0 * 33 + 5) / 10)),
                min_height: Some(h),
                max_height: Some(h),
                ..Stack::default()
            },
            self.style(tokens).visual(),
            alloc::vec![self.label_node(tokens), self.chevron(tokens)],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{Fraction, ShapeKind};
    use nexus_theme_tokens::{BaseTokens, DarkTokens, RadiusToken};

    fn parts(node: &LayoutNode) -> (&Stack, &VisualStyle, &[LayoutNode]) {
        match node {
            LayoutNode::Stack(s, v, c) => (s, v, c),
            _ => panic!("Select must build a Stack"),
        }
    }

    #[test]
    fn a_value_field_of_the_control_height_rounded_by_size() {
        let t = BaseTokens;
        for (size, h, pad, min_w, radius) in [
            (ControlSize::Xs, 24, 4, 79, RadiusToken::Sm),
            (ControlSize::Sm, 30, 8, 99, RadiusToken::Sm),
            (ControlSize::Md, 36, 12, 119, RadiusToken::Md),
            (ControlSize::Lg, 44, 16, 145, RadiusToken::Md),
            (ControlSize::Xl, 52, 20, 172, RadiusToken::Lg),
        ] {
            let node = Select::new().size(size).value("Deutsch").build(&t);
            let (stack, v, _) = parts(&node);
            assert_eq!(
                (stack.min_height, stack.max_height),
                (Some(FxPx::new(h)), Some(FxPx::new(h))),
                "{size:?}"
            );
            assert_eq!(stack.padding.left, FxPx::new(pad), "{size:?}");
            assert_eq!(stack.min_width, Some(FxPx::new(min_w)), "{size:?}");
            assert_eq!(v.corner_radius.top_left, radius.px(), "{size:?}");
        }
    }

    #[test]
    fn builds_trigger_with_value_and_chevron() {
        let t = DarkTokens;
        let node = Select::new().id("lang").value("Deutsch").build(&t);
        let (stack, visual, children) = parts(&node);
        assert_eq!(stack.id, Some("lang"));
        assert_eq!(visual.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(visual.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
        assert_eq!(children.len(), 2, "label + chevron");
        match &children[0] {
            LayoutNode::Text(text, _) => {
                assert_eq!(text.content.as_str(), "Deutsch");
                assert_eq!(text.style.color, t.color(ColorToken::Label));
                assert_eq!(text.style.font_weight, nexus_layout_types::FontWeight::Medium);
            }
            _ => panic!("the value"),
        }
        let (chevron, cv, _) = parts(&children[1]);
        assert_eq!(chevron.min_width, Some(FxPx::new(14)), "md: icon 16 − 2");
        assert_eq!(cv.background, Some(t.color(ColorToken::LabelSecondary)));
        assert!(matches!(cv.shape, ShapeKind::Stroke { .. }));
    }

    #[test]
    fn placeholder_uses_tertiary_ink() {
        let t = BaseTokens;
        match Select::new().placeholder("Sprache").label_node(&t) {
            LayoutNode::Text(node, _) => {
                assert_eq!(node.style.color, t.color(ColorToken::LabelTertiary))
            }
            _ => panic!(),
        }
    }

    #[test]
    fn open_adds_the_fill_layer_and_turns_the_chevron() {
        let t = BaseTokens;
        let closed = Select::new().value("a").build(&t);
        let open = Select::new().value("a").open(true).build(&t);
        let fill = t.color(ColorToken::Fill);
        assert_eq!(parts(&open).1.background, Some(over(fill, fill)));
        assert_ne!(parts(&closed).2[1], parts(&open).2[1], "the chevron turns");
    }

    #[test]
    fn hover_never_repaints_disabled_dims_focus_rings() {
        let t = BaseTokens;
        assert_eq!(Select::new().state(InteractionState::Hover).style(&t), Select::new().style(&t));
        assert_eq!(
            Select::new().state(InteractionState::Disabled).style(&t).visual().opacity,
            Some(Fraction::new(115))
        );
        assert!(Select::new().state(InteractionState::Focused).style(&t).visual().shadow.is_some());
    }
}
