// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Stepper: capsule, targets, value cell and compact geometry per size.

use super::*;
use nexus_layout::LayoutEngine;
use nexus_layout_types::{Fraction, LineLayout, MeasureText, PreparedTextHandle, Spacer};
use nexus_theme_tokens::{BaseTokens, DarkTokens};

struct NoText;

impl MeasureText for NoText {
    fn prepare(&self, _: &TextContent, _: &TextStyle) -> nexus_layout_types::PreparedTextHandle {
        PreparedTextHandle(0)
    }
    fn measure_width(&self, _: &PreparedTextHandle) -> FxPx {
        FxPx::ZERO
    }
    fn layout_lines(&self, _: &PreparedTextHandle, _: FxPx, _: Option<u32>) -> LineLayout {
        LineLayout { lines: Vec::new(), natural_width: FxPx::ZERO }
    }
}

fn glyph() -> LayoutNode {
    LayoutNode::Spacer(Spacer {
        id: None,
        flex_grow: 0,
        min_size: Some(FxPx::new(12)),
        item: FlexItem::default(),
    })
}

fn boxes(node: LayoutNode) -> Vec<(i32, i32, i32, i32)> {
    let row = LayoutNode::Stack(
        Stack { direction: Direction::Row, ..Stack::default() },
        VisualStyle::default(),
        alloc::vec![node],
    );
    LayoutEngine::new()
        .layout(&row, FxPx::new(600), &NoText)
        .expect("lays out")
        .boxes
        .iter()
        .map(|b| (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0))
        .collect()
}

fn parts(node: &LayoutNode) -> (&Stack, &VisualStyle, &[LayoutNode]) {
    match node {
        LayoutNode::Stack(s, v, c) => (s, v, c),
        _ => panic!("Stepper must build a Stack"),
    }
}

#[test]
fn metrics_follow_the_artifacts_table() {
    for (size, w, min, cw, cmin) in [
        (ControlSize::Xs, 28, 25, 14, 18),
        (ControlSize::Sm, 34, 32, 20, 19),
        (ControlSize::Md, 40, 38, 26, 22),
        (ControlSize::Lg, 48, 47, 34, 24),
        (ControlSize::Xl, 56, 55, 42, 27),
    ] {
        let m = StepperMetrics::of(size, false);
        assert_eq!((m.button.0, m.value_min.0), (w, min), "{size:?}");
        let c = StepperMetrics::of(size, true);
        assert_eq!((c.button.0, c.value_min.0), (cw, cmin), "{size:?} compact");
    }
    assert_eq!(StepperMetrics::of(ControlSize::Md, false).glyph, TextStyleToken::Title2);
    assert_eq!(StepperMetrics::of(ControlSize::Md, true).glyph, TextStyleToken::Title3);
}

#[test]
fn a_clipping_fill_capsule_of_the_control_height() {
    let t = DarkTokens;
    let node = Stepper::new().id("qty").number(2, 0, 9).build(&t);
    let (stack, v, children) = parts(&node);
    assert_eq!(stack.id, Some("qty"));
    assert_eq!(stack.overflow, Overflow::Hidden);
    assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
    assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
    assert_eq!(children.len(), 3);
    let b = boxes(node);
    assert_eq!(b[1].3, 36, "the capsule");
    assert_eq!((b[2].2, b[2].3), (40, 34), "− target: h + 4 × inner");
    assert!(b[4].2 >= 38, "value cell ≥ round(36 × 1.06)");
}

#[test]
fn targets_round_their_outer_side_and_the_value_sits_between_hairlines() {
    let t = BaseTokens;
    let node = Stepper::new().number(1, 0, 9).build(&t);
    let (_, _, children) = parts(&node);
    let dec = parts(&children[0]).1.corner_radius;
    assert_eq!((dec.top_left, dec.top_right), (RadiusToken::Full.px(), FxPx::ZERO));
    let inc = parts(&children[2]).1.corner_radius;
    assert_eq!((inc.top_left, inc.bottom_right), (FxPx::ZERO, RadiusToken::Full.px()));
    let cell = parts(&children[1]).1;
    assert_eq!(cell.border.left.map(|b| b.color), Some(t.color(ColorToken::Separator)));
    assert_eq!(cell.border.top, None);
    match &parts(&children[1]).2[0] {
        LayoutNode::Text(text, _) => {
            assert_eq!(text.content.as_str(), "1");
            assert_eq!(
                (text.style.font_size.0, text.style.font_weight),
                (14, FontWeight::Semibold)
            );
        }
        _ => panic!("the value"),
    }
    match &parts(&children[0]).2[0] {
        LayoutNode::Text(text, _) => {
            assert_eq!(text.content.as_str(), "\u{2212}");
            assert_eq!((text.style.font_size.0, text.style.font_weight), (20, FontWeight::Normal));
            assert_eq!(text.style.color, t.color(ColorToken::Label), "default tint label");
        }
        _ => panic!("the − glyph"),
    }
}

#[test]
fn the_end_of_the_range_dims_its_target() {
    let t = BaseTokens;
    let node = Stepper::new().number(0, 0, 3).build(&t);
    let (_, _, children) = parts(&node);
    assert_eq!(parts(&children[0]).1.opacity, Some(Fraction::new(77)));
    assert_eq!(parts(&children[2]).1.opacity, None);
}

#[test]
fn compact_packs_round_targets() {
    let t = BaseTokens;
    let node = Stepper::new().compact(true).dec(glyph()).value_text("4").inc(glyph()).build(&t);
    let (stack, _, children) = parts(&node);
    assert_eq!(stack.gap, FxPx::new(2));
    assert_eq!(stack.padding.left, FxPx::new(5), "0 × 4 inside the edge");
    let (target, tv, _) = parts(&children[0]);
    assert_eq!((target.min_width, target.max_height), (Some(FxPx::new(26)), Some(FxPx::new(26))));
    assert_eq!(tv.corner_radius.top_right, RadiusToken::Full.px(), "a circle");
    assert_eq!(parts(&children[1]).1.border.left, None, "no hairlines");
}

#[test]
fn caller_glyphs_and_the_disabled_dim() {
    let t = BaseTokens;
    let node = Stepper::new()
        .state(InteractionState::Disabled)
        .dec(glyph())
        .value(glyph())
        .inc(glyph())
        .build(&t);
    let (_, v, children) = parts(&node);
    assert_eq!(v.opacity, Some(Fraction::new(115)));
    assert_eq!(children.len(), 3);
    assert!(matches!(parts(&children[0]).2[0], LayoutNode::Spacer(_)), "the caller's glyph");
}
