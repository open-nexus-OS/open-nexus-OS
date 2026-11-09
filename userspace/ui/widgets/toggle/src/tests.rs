// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! GlassToggle: switch geometry per size, roles, states, the button appearance.

use super::*;
use alloc::vec;
use nexus_layout::LayoutEngine;
use nexus_layout_types::{
    Fraction, LineLayout, MeasureText, PreparedTextHandle, Spacer, TextStyle,
};
use nexus_style::FOCUS_RING;
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

/// Every box of the node laid out inside a hugging row (box 0 is that row).
fn sizes(node: LayoutNode) -> alloc::vec::Vec<(i32, i32, i32, i32)> {
    let row = LayoutNode::Stack(
        Stack { direction: Direction::Row, ..Stack::default() },
        VisualStyle::default(),
        vec![node],
    );
    LayoutEngine::new()
        .layout(&row, FxPx::new(400), &NoText)
        .expect("lays out")
        .boxes
        .iter()
        .map(|b| (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0))
        .collect()
}

fn visual(node: &LayoutNode) -> &VisualStyle {
    match node {
        LayoutNode::Stack(_, v, _) => v,
        _ => panic!("a toggle is a Stack"),
    }
}

#[test]
fn switch_metrics_follow_the_artifacts_table() {
    // (size, (width, height, inset, thumb, travel))
    let rows = [
        (ControlSize::Xs, (32, 19, 2, 15, 13)),
        (ControlSize::Sm, (38, 22, 2, 18, 16)),
        (ControlSize::Md, (44, 26, 3, 20, 18)),
        (ControlSize::Lg, (51, 31, 3, 25, 20)),
        (ControlSize::Xl, (58, 35, 3, 29, 23)),
    ];
    for (size, (w, h, inset, thumb, travel)) in rows {
        let m = SwitchMetrics::of(size);
        assert_eq!(
            (m.width.0, m.height.0, m.inset.0, m.thumb.0, m.travel.0),
            (w, h, inset, thumb, travel),
            "{size:?}"
        );
    }
    assert_eq!(SwitchMetrics::of(ControlSize::Md).grow.0, 6);
    assert_eq!(SwitchMetrics::of(ControlSize::Md).stretch_milli(), 1300, "(20 + 6) / 20");
}

#[test]
fn the_thumb_sits_at_the_inset_and_travels_when_on() {
    let t = BaseTokens;
    for size in [ControlSize::Xs, ControlSize::Md, ControlSize::Xl] {
        let m = SwitchMetrics::of(size);
        let off = sizes(GlassToggle::new().size(size).build(&t));
        let on = sizes(GlassToggle::new().size(size).checked(true).build(&t));
        assert_eq!((off[1].2, off[1].3), (m.width.0, m.height.0), "{size:?}: the track");
        assert_eq!(off[2], (m.inset.0, m.inset.0, m.thumb.0, m.thumb.0), "{size:?}: thumb off");
        assert_eq!(on[2].0, m.inset.0 + m.travel.0, "{size:?}: thumb on");
    }
}

#[test]
fn the_track_is_fill_off_and_the_tint_with_its_glow_on() {
    let t = DarkTokens;
    let off = GlassToggle::new().build(&t);
    assert_eq!(visual(&off).background, Some(t.color(ColorToken::Fill)));
    assert_eq!(visual(&off).shadow, None);
    assert_eq!(visual(&off).corner_radius.top_left, RadiusToken::Full.px());
    let on = GlassToggle::new().checked(true).tint(ColorToken::ModeAirplane).build(&t);
    assert_eq!(visual(&on).background, Some(t.color(ColorToken::ModeAirplane)));
    assert_eq!(visual(&on).shadow, Some(glow_shadow(&t, ColorToken::ModeAirplane, Glow::Sm)));
}

#[test]
fn the_thumb_is_white_with_shadow1_and_is_child_0() {
    let t = DarkTokens;
    match GlassToggle::new().id("wifi").build(&t) {
        LayoutNode::Stack(stack, _, children) => {
            assert_eq!(stack.id, Some("wifi"));
            assert_eq!(children.len(), 1, "the thumb, +1 in pre-order (press_offset)");
            let thumb = visual(&children[0]);
            assert_eq!(thumb.background, Some(t.color(ColorToken::OnAccent)));
            assert_eq!(thumb.shadow, Some(nexus_style::box_shadow(t.shadow(ShadowToken::Shadow1))));
        }
        _ => panic!("toggle must be a Stack"),
    }
}

#[test]
fn hover_and_press_never_repaint() {
    let t = BaseTokens;
    for checked in [false, true] {
        let rest = GlassToggle::new().checked(checked).build(&t);
        for state in [InteractionState::Hover, InteractionState::Pressed] {
            assert_eq!(GlassToggle::new().checked(checked).state(state).build(&t), rest);
        }
    }
}

#[test]
fn disabled_dims_to_045_and_focus_draws_the_ring() {
    let t = BaseTokens;
    let disabled = GlassToggle::new().state(InteractionState::Disabled).build(&t);
    assert_eq!(visual(&disabled).opacity, Some(Fraction::new(115)));
    let focused = GlassToggle::new().checked(true).state(InteractionState::Focused).build(&t);
    let ring = visual(&focused).shadow.expect("the ring");
    assert_eq!((ring.spread, ring.color), (FOCUS_RING, t.color(ColorToken::Accent)));
}

#[test]
fn a_visible_label_sits_left_of_the_track() {
    let t = BaseTokens;
    match GlassToggle::new().label("WLAN").id("wifi").build(&t) {
        LayoutNode::Stack(stack, _, children) => {
            assert_eq!(stack.id, Some("wifi"), "the row is the target");
            assert_eq!(stack.gap, FxPx::new(12));
            match &children[0] {
                LayoutNode::Text(text, _) => {
                    assert_eq!(text.content.as_str(), "WLAN");
                    assert_eq!(text.style.font_weight, nexus_layout_types::FontWeight::Medium);
                    assert_eq!(text.style.color, t.color(ColorToken::Label));
                }
                _ => panic!("label first"),
            }
            assert!(matches!(children[1], LayoutNode::Stack(..)), "then the track");
        }
        _ => panic!(),
    }
}

#[test]
fn the_button_appearance_is_a_capsule_of_the_control_height() {
    let t = BaseTokens;
    let icon = || {
        LayoutNode::Spacer(Spacer {
            min_size: Some(FxPx::new(16)),
            flex_grow: 0,
            ..Spacer::default()
        })
    };
    let off =
        GlassToggle::new().appearance(ToggleAppearance::Button).label("Nicht stören").build(&t);
    assert_eq!(visual(&off).background, Some(t.color(ColorToken::Fill)));
    assert_eq!(visual(&off).border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
    assert_eq!(sizes(off)[1].3, 36);
    let on =
        GlassToggle::new().appearance(ToggleAppearance::Button).checked(true).label("x").build(&t);
    assert_eq!(visual(&on).background, Some(t.color(ColorToken::Accent)));
    assert_eq!(visual(&on).shadow, Some(glow_shadow(&t, ColorToken::Accent, Glow::Md)));
    let circle = GlassToggle::new()
        .appearance(ToggleAppearance::Button)
        .size(ControlSize::Lg)
        .icon(icon())
        .build(&t);
    let b = sizes(circle);
    assert_eq!((b[1].2, b[1].3), (44, 44), "icon-only: a circle");
}
