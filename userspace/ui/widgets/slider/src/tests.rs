// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slider: the control bar's split, cap, glyph and well; the thumb slider's geometry.

use super::*;
use nexus_layout::LayoutEngine;
use nexus_layout_types::{
    Fraction, LineLayout, MeasureText, PreparedTextHandle, TextContent, TextStyle,
};
use nexus_style::control::glow;
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
        LineLayout { lines: alloc::vec::Vec::new(), natural_width: FxPx::ZERO }
    }
}

fn children(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Stack(_, _, children) => children,
        _ => panic!("the slider builds a Stack"),
    }
}

fn stack(node: &LayoutNode) -> &Stack {
    match node {
        LayoutNode::Stack(stack, _, _) => stack,
        _ => panic!("expected a Stack"),
    }
}

fn visual(node: &LayoutNode) -> &VisualStyle {
    match node {
        LayoutNode::Stack(_, v, _) => v,
        _ => panic!("expected a Stack"),
    }
}

/// Every box of the slider laid out across a 220 px row (box 0 is the row).
fn boxes(node: LayoutNode) -> alloc::vec::Vec<(i32, i32, i32, i32)> {
    let row = LayoutNode::Stack(
        Stack {
            direction: Direction::Row,
            min_width: Some(FxPx::new(220)),
            max_width: Some(FxPx::new(220)),
            ..Stack::default()
        },
        VisualStyle::default(),
        alloc::vec![node],
    );
    LayoutEngine::new()
        .layout(&row, FxPx::new(220), &NoText)
        .expect("lays out")
        .boxes
        .iter()
        .map(|b| (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0))
        .collect()
}

fn glyph() -> LayoutNode {
    LayoutNode::Stack(
        Stack { id: Some("glyph"), ..Default::default() },
        VisualStyle {
            background: Some(nexus_layout_types::Rgba8::new(0, 0, 0, 255)),
            ..VisualStyle::default()
        },
        alloc::vec![],
    )
}

#[test]
fn fill_weight_tracks_value() {
    assert_eq!(Slider::new().value(0).fill_weight(), 0);
    assert_eq!(Slider::new().value(100).fill_weight(), 100);
    assert_eq!(Slider::new().value(200).value(50).fill_weight(), 50, "clamps");
}

/// The fill and the remainder always add up to the whole track, whatever
/// width the row hands down. That is the property a pixel-computing
/// builder could not have.
#[test]
fn fill_and_rest_weights_sum_to_the_whole_track() {
    let t = BaseTokens;
    for value in [0, 1, 37, 50, 99] {
        let node = Slider::new().value(value).build(&t);
        let kids = children(&node);
        assert_eq!(kids.len(), 2, "value {value}: fill + rest");
        let fill = stack(&kids[0]).item.flex_grow;
        let rest = stack(&kids[1]).item.flex_grow;
        assert_eq!(fill + rest, 100, "value {value}: weights must partition the track");
        assert_eq!(fill, u32::from(value));
    }
}

/// At zero the fill is still THERE — it is the handle.
#[test]
fn the_fill_survives_value_zero_as_the_handle() {
    let t = BaseTokens;
    let node = Slider::new().value(0).build(&t);
    let fill = stack(&children(&node)[0]);
    assert_eq!(fill.item.flex_grow, 0, "no share of the free space…");
    assert_eq!(fill.min_width, Some(FxPx::new(MIN_FILL)), "…but never narrower than the cap");
}

#[test]
fn full_value_has_no_rest_bar() {
    let t = BaseTokens;
    let node = Slider::new().value(100).build(&t);
    assert_eq!(children(&node).len(), 1, "the fill IS the track at 100");
}

#[test]
fn the_bar_is_a_fill_well_with_a_white_lifted_fill() {
    let t = DarkTokens;
    let node = Slider::new().value(60).build(&t);
    let track = visual(&node);
    assert_eq!(track.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(track.inset_highlight, Some(t.shadow(ShadowToken::Inset).color), "the well");
    let fill = visual(&children(&node)[0]);
    assert_eq!(fill.background, Some(t.color(ColorToken::OnAccent)), "white in both themes");
    assert_eq!(fill.shadow, Some(nexus_style::box_shadow(t.shadow(ShadowToken::Shadow1))));
    let tinted = Slider::new().value(60).tint(ColorToken::Warning).build(&t);
    assert_eq!(visual(&children(&tinted)[0]).background, Some(t.color(ColorToken::Warning)));
}

/// The glyph sits inside the FILL at a fixed 7 px inset (the fill sweeps over it) in
/// `island` at 50 %, or the tint's on-colour at 85 % on a tinted fill.
#[test]
fn the_glyph_rides_inside_the_fill_at_a_fixed_inset() {
    let t = BaseTokens;
    let node = Slider::new().value(70).leading(glyph()).build(&t);
    let fill = &children(&node)[0];
    assert_eq!(stack(fill).padding.left, FxPx::new(GLYPH_INSET));
    assert_eq!(GLYPH_INSET, 7);
    let g = &children(fill)[0];
    assert_eq!(stack(g).id, Some("glyph"));
    let island = t.color(ColorToken::Island);
    assert_eq!(
        visual(g).background,
        Some(nexus_layout_types::Rgba8::new(island.r, island.g, island.b, 128))
    );
    let tinted = Slider::new().value(70).tint(ColorToken::Accent).leading(glyph()).build(&t);
    let on = visual(&children(&children(&tinted)[0])[0]).background.expect("glyph");
    assert_eq!(on.a, 217, "onAccent at 85 %");
}

#[test]
fn the_glyph_is_optional_and_the_id_reaches_the_track() {
    let t = BaseTokens;
    let node = Slider::new().value(40).id("vol").build(&t);
    assert!(children(&children(&node)[0]).is_empty());
    assert_eq!(stack(&node).id, Some("vol"));
}

#[test]
fn disabled_dims_to_045_and_hover_never_repaints() {
    let t = BaseTokens;
    let node = Slider::new().state(InteractionState::Disabled).build(&t);
    assert_eq!(visual(&node).opacity, Some(Fraction::new(115)));
    assert_eq!(
        Slider::new().value(30).state(InteractionState::Hover).build(&t),
        Slider::new().value(30).build(&t)
    );
}

#[test]
fn the_thumb_slider_runs_its_thumb_across_the_travel() {
    let t = BaseTokens;
    for (value, thumb_x) in [(0, 0), (50, 100), (100, 200)] {
        let b = boxes(Slider::new().variant(SliderVariant::Thumb).value(value).build(&t));
        assert_eq!((b[1].2, b[1].3), (220, 28), "the hit track fills the row, 28 high");
        let thumb = b.iter().find(|r| r.2 == 20 && r.3 == 20).expect("the 20 px thumb");
        assert_eq!(thumb.0, thumb_x, "value {value}");
        assert_eq!(thumb.1, 4, "centred on the 28 px track");
    }
}

#[test]
fn the_thumb_slider_paints_tint_fill_white_thumb_and_fill_rail() {
    let t = DarkTokens;
    let node =
        Slider::new().variant(SliderVariant::Thumb).value(40).tint(ColorToken::Success).build(&t);
    let kids = children(&node);
    let filled = visual(&children(&kids[0])[0]);
    assert_eq!(filled.background, Some(t.color(ColorToken::Success)));
    assert_eq!(
        filled.shadow.map(|s| (s.blur_radius.0, s.color)),
        Some((8, glow(&t, ColorToken::Success)))
    );
    let thumb = visual(&kids[1]);
    assert_eq!(thumb.background, Some(t.color(ColorToken::OnAccent)));
    assert_eq!(thumb.shadow, Some(nexus_style::box_shadow(t.shadow(ShadowToken::Shadow1))));
    let rest = visual(&children(&kids[2])[0]);
    assert_eq!(rest.background, Some(t.color(ColorToken::Fill)));
    let b = boxes(Slider::new().variant(SliderVariant::Thumb).value(40).build(&t));
    assert!(b.iter().any(|r| r.3 == 6), "the 6 px rail");
}

#[test]
fn ticks_span_the_thumb_travel_below_the_track() {
    let t = BaseTokens;
    let b = boxes(Slider::new().variant(SliderVariant::Thumb).value(0).ticks(5).build(&t));
    let ticks: alloc::vec::Vec<_> = b.iter().filter(|r| r.2 == 2 && r.3 == 5).collect();
    assert_eq!(ticks.len(), 5);
    assert_eq!(ticks[0].0, 9, "the first tick centred on the thumb's start (10)");
    assert_eq!(ticks[4].0 + 1, 220 - 10, "the last one on its end");
    assert!(ticks.iter().all(|r| r.1 == 22));
}
