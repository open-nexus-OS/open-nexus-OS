// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Segment: track and item geometry per size, the thumb, labels, states.

use super::*;
use nexus_layout::LayoutEngine;
use nexus_layout_types::{Fraction, LineLayout, MeasureText, PreparedTextHandle, Spacer};
use nexus_theme_tokens::{BaseTokens, DarkTokens};

struct NoText;

impl MeasureText for NoText {
    fn prepare(&self, _: &TextContent, _: &TextStyle) -> PreparedTextHandle {
        PreparedTextHandle(0)
    }
    fn measure_width(&self, _: &PreparedTextHandle) -> FxPx {
        FxPx::ZERO
    }
    fn layout_lines(&self, _: &PreparedTextHandle, _: FxPx, _: Option<u32>) -> LineLayout {
        LineLayout { lines: Vec::new(), natural_width: FxPx::ZERO }
    }
}

fn opt() -> LayoutNode {
    LayoutNode::Spacer(Spacer {
        id: None,
        flex_grow: 0,
        min_size: Some(FxPx::new(24)),
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
        _ => panic!("expected a Stack"),
    }
}

#[test]
fn the_track_is_the_control_height_and_items_sit_inside_its_edge() {
    let t = BaseTokens;
    for (size, h, pad, item_h) in [
        (ControlSize::Xs, 24, 2, 18),
        (ControlSize::Sm, 30, 2, 24),
        (ControlSize::Md, 36, 2, 30),
        (ControlSize::Lg, 44, 4, 34),
        (ControlSize::Xl, 52, 4, 42),
    ] {
        let b = boxes(Segment::new().size(size).options(alloc::vec![opt(), opt()]).build(&t));
        assert_eq!(b[1].3, h, "{size:?}: track");
        assert_eq!(b[2].3, item_h, "{size:?}: item");
        assert_eq!(b[2].1 - b[1].1, pad + 1, "{size:?}: inset + edge");
        assert_eq!(SegmentMetrics::of(size).item_height.0, item_h);
    }
}

#[test]
fn the_track_is_a_fill_capsule_with_the_glass_edge() {
    let t = DarkTokens;
    let node = Segment::new().active(1).options(alloc::vec![opt(), opt(), opt()]).build(&t);
    let (_, v, slots) = parts(&node);
    assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
    assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
    assert_eq!(slots.len(), 3);
    let thumb = parts(&slots[1]).1;
    assert_eq!(thumb.background, Some(t.color(ColorToken::SurfaceOverlay)), "the thumb");
    assert_eq!(thumb.shadow, Some(nexus_style::box_shadow(t.shadow(ShadowToken::Shadow1))));
    assert_eq!(parts(&slots[0]).1.background, None, "inactive items are transparent");
}

#[test]
fn labels_are_semibold_active_and_medium_secondary_otherwise() {
    let t = BaseTokens;
    let node = Segment::new().labels(["Tag", "Woche"]).build(&t);
    let (_, _, slots) = parts(&node);
    let label = |slot: &LayoutNode| match &parts(slot).2[0] {
        LayoutNode::Text(text, _) => text.style.clone(),
        _ => panic!("a label"),
    };
    let active = label(&slots[0]);
    assert_eq!(
        (active.font_size.0, active.font_weight),
        (13, FontWeight::Semibold),
        "md = subheadline"
    );
    assert_eq!(active.color, t.color(ColorToken::Label));
    let idle = label(&slots[1]);
    assert_eq!(
        (idle.font_weight, idle.color),
        (FontWeight::Medium, t.color(ColorToken::LabelSecondary))
    );
    let tinted = Segment::new().tint(ColorToken::Success).labels(["a"]).build(&t);
    assert_eq!(label(&parts(&tinted).2[0]).color, t.color(ColorToken::Success));
}

#[test]
fn icon_only_items_are_square_ish() {
    let t = BaseTokens;
    let node = Segment::new().icon_only(true).options(alloc::vec![opt()]).build(&t);
    let (stack, _, _) = parts(&parts(&node).2[0]);
    assert_eq!(stack.min_width, Some(FxPx::new(36)), "item height 30 + 6");
    assert_eq!(stack.padding.left, FxPx::new(8));
}

#[test]
fn disabled_control_and_option_dim() {
    let t = BaseTokens;
    let node = Segment::new().labels(["a", "b"]).disable_option(1).build(&t);
    assert_eq!(parts(&parts(&node).2[1]).1.opacity, Some(Fraction::new(102)));
    let off = Segment::new().labels(["a"]).state(InteractionState::Disabled).build(&t);
    assert_eq!(parts(&off).1.opacity, Some(Fraction::new(115)));
}

#[test]
fn keeps_id_and_legacy_sizes() {
    let t = BaseTokens;
    let node = Segment::new()
        .id("view")
        .size(SegmentSize::Lg)
        .options(alloc::vec![opt(), opt()])
        .build(&t);
    let (stack, _, slots) = parts(&node);
    assert_eq!(stack.id, Some("view"));
    assert_eq!(stack.min_height, Some(FxPx::new(44)));
    assert_eq!(slots.len(), 2);
}
