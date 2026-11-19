// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! `GlassCheckbox` — the design-system checkbox (the design artifacts' `GlassCheckbox`): a
//! rounded square in `fill` with a `separator` edge and the lit top edge; checked or
//! indeterminate, the tint layer (with its small glow) covers it and carries a stroked ✓ or a
//! dash in the on-colour. Five sizes (box 14/18/22/25/28, radius 6 up to 18 px, 8 above); an
//! optional label sits right of the box (gap 12, 8 below 22 px) in the control label.
//!
//! A pure builder producing a `LayoutNode::Stack`: the box itself, or the [box, label] row
//! when labelled (the row is then the tap target). Hover and press never repaint it; the state
//! draws the disabled dim (0.45) and the focus ring. The compositor hit-tests `id`; the app
//! maps id → toggle (from indeterminate a click reports checked). DSL-emittable.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, Direction, FlexItem, FxPx, Justify, LayoutNode, Stack, TextContent, TextNode,
    VisualStyle,
};
use nexus_style::control::{glow_shadow, label_style, on_tint, Glow};
use nexus_style::{glyph, Dim, InteractionState, Style};
use nexus_theme_tokens::{ColorToken, ControlSize, RadiusToken, ShadowToken, Tokens};

/// The ✓ (`M4 12.5l5 5L20 6.5`, stroke 3).
const CHECK: [&[glyph::GridPoint]; 1] = [&[(40, 125), (90, 175), (200, 65)]];
/// The indeterminate dash (`M5.5 12h13`, stroke 3.2).
const DASH: [&[glyph::GridPoint]; 1] = [&[(55, 120), (185, 120)]];

/// The box side of a size: 14 · 18 · 22 · 25 · 28.
#[must_use]
pub const fn box_px(size: ControlSize) -> i32 {
    match size {
        ControlSize::Xs => 14,
        ControlSize::Sm => 18,
        ControlSize::Md => 22,
        ControlSize::Lg => 25,
        ControlSize::Xl => 28,
    }
}

/// A checkbox.
#[derive(Debug, Clone, Default)]
pub struct GlassCheckbox {
    checked: bool,
    indeterminate: bool,
    state: InteractionState,
    id: Option<&'static str>,
    size: ControlSize,
    tint: Option<ColorToken>,
    label: Option<String>,
}

impl GlassCheckbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// The mixed state (a dash) — shown while set, whatever `checked` says.
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
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

    /// The on-state's colour role (default accent).
    pub fn tint(mut self, tint: ColorToken) -> Self {
        self.tint = Some(tint);
        self
    }

    /// The visible label, right of the box.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn filled(&self) -> bool {
        self.checked || self.indeterminate
    }

    fn tint_role(&self) -> ColorToken {
        self.tint.unwrap_or(ColorToken::Accent)
    }

    fn labelled(&self) -> bool {
        self.label.is_some()
    }

    /// Focus ring and disabled dim (on the root).
    fn finish(&self, style: Style, tokens: &dyn Tokens) -> Style {
        let mut s = style;
        if self.state.shows_focus_ring() {
            s = s.focus_ring(tokens, ColorToken::Accent);
        }
        if self.state.is_disabled() {
            s = s.opacity(self.state.opacity_as(Dim::Control));
        }
        s
    }

    /// The resolved box [`Style`]: `fill` + `separator` edge + lit top edge off; the tint (it
    /// covers the edge) with its small glow when checked or indeterminate. The focus ring and
    /// the disabled dim land on the box unless a label row carries them.
    pub fn style(&self, tokens: &dyn Tokens) -> Style {
        let side = box_px(self.size);
        let radius = if side <= 18 { 6 } else { RadiusToken::Sm.px().0 };
        // The box keeps its off look; the tint is the on layer over it (`on_layer`), so a change
        // fades instead of snapping (design artifacts: CHK-8, TASK-0354 P10).
        let s = Style::new()
            .rounded(FxPx::new(radius))
            .background_token(tokens, ColorToken::Fill)
            .border_token(tokens, ColorToken::Separator)
            .shadow_token(tokens, ShadowToken::EdgeHighlight);
        if self.labelled() {
            s
        } else {
            self.finish(s, tokens)
        }
    }

    /// The mark: the ✓ or the dash, round(box × 0.6), in the on-colour. Always there (the host
    /// pops it in and out); its own opacity 0 while the box is empty.
    fn mark(&self, tokens: &dyn Tokens) -> LayoutNode {
        let size = FxPx::new((box_px(self.size) * 6 + 5) / 10);
        let ink = tokens.color(on_tint(self.tint_role()));
        let mut mark = if self.indeterminate {
            glyph::stroke(&DASH, false, 32, size, ink)
        } else {
            glyph::stroke(&CHECK, false, 30, size, ink)
        };
        if !self.filled() {
            hide(&mut mark);
        }
        mark
    }

    /// The on layer: the tint with its small glow over the whole box (it covers the edge), the
    /// mark centred in it; its own opacity 0 while the box is empty. `BOX_PARTS` names it and
    /// the mark.
    fn on_layer(&self, tokens: &dyn Tokens) -> LayoutNode {
        let d = Some(FxPx::new(box_px(self.size)));
        let radius = if box_px(self.size) <= 18 { 6 } else { RadiusToken::Sm.px().0 };
        let mut style = Style::new()
            .rounded(FxPx::new(radius))
            .background(tokens.color(self.tint_role()))
            .shadow(glow_shadow(tokens, self.tint_role(), Glow::Sm));
        if !self.filled() {
            style = style.opacity(nexus_layout_types::Fraction(0));
        }
        LayoutNode::Stack(
            Stack {
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
            style.visual(),
            alloc::vec![self.mark(tokens)],
        )
    }

    /// The box's moving parts as child paths from the box: the on layer and the mark in it. The
    /// DSL runtime's motion table names them by these paths.
    pub const BOX_PARTS: [&'static [u32]; 2] = [&[0], &[0, 0]];

    fn check_box(&self, tokens: &dyn Tokens) -> LayoutNode {
        let d = Some(FxPx::new(box_px(self.size)));
        LayoutNode::Stack(
            Stack {
                id: if self.labelled() { None } else { self.id },
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
            self.style(tokens).visual(),
            alloc::vec![self.on_layer(tokens)],
        )
    }

    /// Build the checkbox node.
    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let check_box = self.check_box(tokens);
        let Some(label) = self.label.clone() else {
            return check_box;
        };
        let gap = if box_px(self.size) >= 22 { 12 } else { 8 };
        let text = LayoutNode::Text(
            TextNode {
                content: TextContent::new(label),
                style: label_style(self.size, tokens.color(ColorToken::Label)),
                max_lines: Some(1),
                ..TextNode::default()
            },
            VisualStyle::default(),
        );
        let row_style = self.finish(Style::new().rounded_token(RadiusToken::Sm), tokens);
        let children: Vec<LayoutNode> = alloc::vec![check_box, text];
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(gap),
                align: Align::Center,
                ..Stack::default()
            },
            row_style.visual(),
            children,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{Fraction, ShapeKind};
    use nexus_style::FOCUS_RING;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    fn parts(node: &LayoutNode) -> (&Stack, &VisualStyle, &[LayoutNode]) {
        match node {
            LayoutNode::Stack(s, v, c) => (s, v, c),
            _ => panic!("a checkbox is a Stack"),
        }
    }

    #[test]
    fn box_sizes_and_radii_per_size() {
        let t = BaseTokens;
        for (size, side, radius) in [
            (ControlSize::Xs, 14, 6),
            (ControlSize::Sm, 18, 6),
            (ControlSize::Md, 22, 8),
            (ControlSize::Lg, 25, 8),
            (ControlSize::Xl, 28, 8),
        ] {
            let node = GlassCheckbox::new().size(size).build(&t);
            let (stack, v, _) = parts(&node);
            assert_eq!(stack.min_width, Some(FxPx::new(side)), "{size:?}");
            assert_eq!(stack.max_height, Some(FxPx::new(side)), "{size:?}");
            assert_eq!(v.corner_radius.top_left, FxPx::new(radius), "{size:?}");
        }
    }

    #[test]
    fn off_is_fill_with_separator_edge_and_lit_top() {
        let t = BaseTokens;
        let node = GlassCheckbox::new().build(&t);
        let (_, v, children) = parts(&node);
        assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::Separator)));
        assert_eq!(v.inset_highlight, Some(t.shadow(ShadowToken::EdgeHighlight).color));
        let (_, layer, marks) = parts(&children[0]);
        assert_eq!(layer.opacity, Some(Fraction(0)), "off: the on layer is there, invisible");
        assert_eq!(parts(&marks[0]).1.opacity, Some(Fraction(0)), "and so is the mark");
    }

    #[test]
    fn checked_is_the_tint_with_glow_and_an_on_colour_check() {
        let t = DarkTokens;
        let node = GlassCheckbox::new().checked(true).tint(ColorToken::Success).build(&t);
        let (_, v, children) = parts(&node);
        assert_eq!(v.background, Some(t.color(ColorToken::Fill)), "the box never changes");
        assert_eq!(children.len(), 1, "the on layer");
        let (_, layer, marks) = parts(&children[0]);
        assert_eq!(layer.background, Some(t.color(ColorToken::Success)));
        assert_eq!(layer.opacity, None, "drawn");
        assert_eq!(layer.border.top, None, "the tint layer covers the edge");
        assert_eq!(layer.shadow, Some(glow_shadow(&t, ColorToken::Success, Glow::Sm)));
        let (mark, mv, _) = parts(&marks[0]);
        assert_eq!(mark.min_width, Some(FxPx::new(13)), "round(22 × 0.6)");
        assert_eq!(mv.background, Some(t.color(ColorToken::OnAccent)));
        assert_eq!(mv.opacity, None);
        match &mv.shape {
            ShapeKind::Stroke { paths, .. } => assert_eq!(paths[0].points.len(), 3, "the ✓"),
            other => panic!("a stroked mark, got {other:?}"),
        }
    }

    #[test]
    fn indeterminate_draws_the_dash() {
        let t = BaseTokens;
        let node = GlassCheckbox::new().indeterminate(true).build(&t);
        let (_, layer, marks) = parts(&parts(&node).2[0]);
        assert_eq!(layer.background, Some(t.color(ColorToken::Accent)));
        match &parts(&marks[0]).1.shape {
            ShapeKind::Stroke { paths, .. } => assert_eq!(paths[0].points.len(), 2, "the dash"),
            _ => panic!(),
        }
    }

    /// `BOX_PARTS` lands on the on layer and the mark (the DSL runtime's motion table uses it).
    #[test]
    fn the_box_parts_name_the_layer_and_the_mark() {
        let t = BaseTokens;
        let node = GlassCheckbox::new().checked(true).build(&t);
        let at = |path: &[u32]| path.iter().fold(&node, |n, i| &parts(n).2[*i as usize]);
        assert!(parts(at(GlassCheckbox::BOX_PARTS[0])).1.shadow.is_some(), "the glowing layer");
        assert!(matches!(parts(at(GlassCheckbox::BOX_PARTS[1])).1.shape, ShapeKind::Stroke { .. }));
    }

    #[test]
    fn hover_and_press_never_repaint() {
        let t = BaseTokens;
        for checked in [false, true] {
            let rest = GlassCheckbox::new().checked(checked).build(&t);
            for state in [InteractionState::Hover, InteractionState::Pressed] {
                assert_eq!(GlassCheckbox::new().checked(checked).state(state).build(&t), rest);
            }
        }
    }

    #[test]
    fn focus_and_disabled() {
        let t = BaseTokens;
        assert_eq!(
            GlassCheckbox::new().state(InteractionState::Disabled).style(&t).visual().opacity,
            Some(Fraction::new(115))
        );
        let ring = GlassCheckbox::new().state(InteractionState::Focused).style(&t).visual().shadow;
        assert_eq!(ring.map(|r| r.spread), Some(FOCUS_RING));
    }

    #[test]
    fn a_label_row_puts_the_label_right_of_the_box() {
        let t = BaseTokens;
        let node = GlassCheckbox::new()
            .label("AGB akzeptieren")
            .id("agree")
            .state(InteractionState::Disabled)
            .build(&t);
        let (stack, v, children) = parts(&node);
        assert_eq!(stack.id, Some("agree"));
        assert_eq!(stack.gap, FxPx::new(12));
        assert_eq!(v.opacity, Some(Fraction::new(115)), "the row dims");
        assert_eq!(parts(&children[0]).1.opacity, None, "not twice");
        match &children[1] {
            LayoutNode::Text(text, _) => {
                assert_eq!(text.content.as_str(), "AGB akzeptieren");
                assert_eq!(text.style.font_size, FxPx::new(14));
            }
            _ => panic!("the label"),
        }
        let small = GlassCheckbox::new().size(ControlSize::Sm).label("x").build(&t);
        assert_eq!(parts(&small).0.gap, FxPx::new(8));
    }
}

/// Sets a node's own opacity to 0 (an absent part: laid out, not drawn).
fn hide(node: &mut LayoutNode) {
    if let LayoutNode::Stack(_, visual, _) | LayoutNode::Text(_, visual) = node {
        visual.opacity = Some(nexus_layout_types::Fraction(0));
    }
}
