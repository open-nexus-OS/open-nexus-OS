// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `Radio` — the design-system radio indicator, and [`RadioGroup`], the list of rows it lives
//! in (the design artifacts' `GlassRadioGroup`).
//!
//! The indicator: a circle in `fill` with a `separator` edge and the lit top edge; selected,
//! the tint layer (with its small glow) covers it and carries the on-colour dot
//! (round(r × 0.36)). Five sizes: 14/16/20/22/25. Hover and press never repaint it; the state
//! draws the disabled dim (0.45) and the focus ring. A pure builder producing a
//! `LayoutNode::Stack`. DSL-emittable.

extern crate alloc;

mod group;
pub use group::{Orientation, RadioGroup, RadioOption};

use nexus_layout_types::{Align, Direction, FlexItem, FxPx, Justify, LayoutNode, Stack};
use nexus_style::control::{glow_shadow, on_tint, Glow};
use nexus_style::{Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, ShadowToken, Tokens};

/// The indicator diameter and its dot of a size: 14/5 · 16/6 · 20/7 · 22/8 · 25/9.
#[must_use]
pub const fn diameter(size: ControlSize) -> (i32, i32) {
    let r = match size {
        ControlSize::Xs => 14,
        ControlSize::Sm => 16,
        ControlSize::Md => 20,
        ControlSize::Lg => 22,
        ControlSize::Xl => 25,
    };
    (r, (r * 36 + 50) / 100)
}

/// A radio indicator.
#[derive(Debug, Clone, Default)]
pub struct Radio {
    selected: bool,
    state: InteractionState,
    id: Option<&'static str>,
    size: ControlSize,
    tint: Option<ColorToken>,
}

impl Radio {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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

    /// The selected state's colour role (default accent).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    fn tint_role(&self) -> ColorToken {
        self.tint.unwrap_or(ColorToken::Accent)
    }

    /// The resolved circle [`Style`]: `fill` + `separator` edge + lit top edge, or the tint with
    /// its small glow when selected; the focus ring and the disabled dim.
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let s = Style::new().rounded_token(RadiusToken::Full);
        let mut s = if self.selected {
            s.background(tokens.color(self.tint_role())).shadow(glow_shadow(
                tokens,
                self.tint_role(),
                Glow::Sm,
            ))
        } else {
            s.background_token(tokens, ColorToken::Fill)
                .border_token(tokens, ColorToken::Separator)
                .shadow_token(tokens, ShadowToken::EdgeHighlight)
        };
        if self.state.shows_focus_ring() {
            s = s.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity_as(Dim::Control));
        }
        s
    }

    fn dot(&self, tokens: &dyn Tokens) -> LayoutNode {
        let (_, dot) = diameter(self.size);
        let d = Some(FxPx::new(dot));
        LayoutNode::Stack(
            Stack { min_width: d, max_width: d, min_height: d, max_height: d, ..Stack::default() },
            Style::new()
                .background_token(tokens, on_tint(self.tint_role()))
                .rounded_token(RadiusToken::Full)
                .visual(),
            alloc::vec![],
        )
    }

    /// Build the radio node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let visual = self.style(tokens).visual();
        let children = if self.selected { alloc::vec![self.dot(tokens)] } else { alloc::vec![] };
        let d = Some(FxPx::new(diameter(self.size).0));
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                align: Align::Center,
                justify: Justify::Center,
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            visual,
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{Fraction, VisualStyle};
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    fn parts(node: &LayoutNode) -> (&Stack, &VisualStyle, &[LayoutNode]) {
        match node {
            LayoutNode::Stack(s, v, c) => (s, v, c),
            _ => panic!("a radio is a Stack"),
        }
    }

    #[test]
    fn diameters_and_dots_per_size() {
        let rows = [
            (ControlSize::Xs, (14, 5)),
            (ControlSize::Sm, (16, 6)),
            (ControlSize::Md, (20, 7)),
            (ControlSize::Lg, (22, 8)),
            (ControlSize::Xl, (25, 9)),
        ];
        for (size, expect) in rows {
            assert_eq!(diameter(size), expect, "{size:?}");
        }
    }

    #[test]
    fn selected_is_the_tint_with_glow_and_an_on_colour_dot() {
        let t = DarkTokens;
        let node = Radio::new().selected(true).build(&t);
        let (stack, v, children) = parts(&node);
        assert_eq!(stack.min_width, Some(FxPx::new(20)));
        assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
        assert_eq!(v.shadow, Some(glow_shadow(&t, ColorToken::Accent, Glow::Sm)));
        assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
        assert_eq!(children.len(), 1, "the dot");
        let (dot, dv, _) = parts(&children[0]);
        assert_eq!(dot.min_width, Some(FxPx::new(7)));
        assert_eq!(dv.background, Some(t.color(ColorToken::OnAccent)));
    }

    #[test]
    fn unselected_is_fill_with_separator_edge_and_no_dot() {
        let t = BaseTokens;
        let node = Radio::new().selected(false).build(&t);
        let (_, v, children) = parts(&node);
        assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::Separator)));
        assert_eq!(v.inset_highlight, Some(t.shadow(ShadowToken::EdgeHighlight).color));
        assert!(children.is_empty());
    }

    #[test]
    fn hover_never_repaints_and_disabled_dims() {
        let t = BaseTokens;
        assert_eq!(Radio::new().state(InteractionState::Hover).style(&t), Radio::new().style(&t));
        assert_eq!(
            Radio::new().state(InteractionState::Disabled).style(&t).visual().opacity,
            Some(Fraction::new(115))
        );
        assert!(Radio::new().state(InteractionState::Focused).style(&t).visual().shadow.is_some());
    }
}
