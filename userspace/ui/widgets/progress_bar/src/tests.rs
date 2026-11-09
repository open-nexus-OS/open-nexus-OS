// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `ProgressBar` (split from lib.rs): the linear track and its split,
//! the pip, sizes, the header, the glow, the circular ring and the disabled dim.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_layout_types::{Position, ShapeKind};
use nexus_theme_tokens::BaseTokens;

fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(s, v, k) => (s, v, k),
        _ => panic!("expected a stack"),
    }
}

fn grow(node: &LayoutNode) -> u32 {
    match node {
        LayoutNode::Stack(s, _, _) => s.item.flex_grow,
        LayoutNode::Spacer(s) => s.flex_grow.max(s.item.flex_grow),
        _ => panic!("bar part"),
    }
}

#[test]
fn the_track_fills_its_container_in_the_control_fill() {
    let t = BaseTokens;
    let node = ProgressBar::new().value(64).build(&t);
    let (s, v, kids) = stack(&node);
    assert_eq!(
        (s.min_width, s.max_width),
        (None, None),
        "no pinned 200 px — the container decides"
    );
    assert_eq!((s.min_height, s.max_height), (Some(FxPx::new(6)), Some(FxPx::new(6))));
    assert_eq!(s.overflow, Overflow::Hidden);
    assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
    // fill : rest = 64 : 36, both on a zero basis.
    assert_eq!((grow(&kids[0]), grow(&kids[1])), (64, 36));
    let (_, fv, _) = stack(&kids[0]);
    assert_eq!(fv.background, Some(t.color(ColorToken::Accent)));
    assert_eq!(
        fv.shadow.map(|s| (s.blur_radius, s.color)),
        Some((FxPx::new(8), t.color(ColorToken::AccentGlow)))
    );
}

#[test]
fn only_the_accent_glows() {
    let t = BaseTokens;
    let node = ProgressBar::new().value(50).tint(ColorToken::Success).build(&t);
    let (_, _, kids) = stack(&node);
    let (_, fv, _) = stack(&kids[0]);
    assert_eq!(fv.background, Some(t.color(ColorToken::Success)));
    assert_eq!(fv.shadow, None);
}

#[test]
fn sizes_set_the_bar_height_and_height_overrides() {
    let heights: Vec<i32> =
        [ControlSize::Xs, ControlSize::Sm, ControlSize::Md, ControlSize::Lg, ControlSize::Xl]
            .iter()
            .map(|s| ProgressBar::new().size(*s).bar_height().0)
            .collect();
    assert_eq!(heights, alloc::vec![3, 4, 6, 8, 10]);
    assert_eq!(ProgressBar::new().size(ControlSize::Xl).height(5).bar_height(), FxPx::new(5));
}

#[test]
fn the_pip_is_40_percent_and_the_first_child() {
    let t = BaseTokens;
    let node = ProgressBar::new().indeterminate().build(&t);
    let (_, _, kids) = stack(&node);
    assert_eq!((grow(&kids[0]), grow(&kids[1])), (40, 60), "the host's sweep moves child 1");
    // A pinned track renders a static frame at `phase`.
    let node = ProgressBar::new().width(200).indeterminate().phase(50).build(&t);
    let (_, _, kids) = stack(&node);
    let (pip, _, _) = stack(&kids[0]);
    assert_eq!(pip.min_width, Some(FxPx::new(80)));
    assert_eq!(pip.item.margin.left, FxPx::new(60), "half of the 120 px travel");
}

#[test]
fn value_clamps_to_100() {
    let t = BaseTokens;
    let node = ProgressBar::new().value(250).build(&t);
    let (_, _, kids) = stack(&node);
    assert_eq!((grow(&kids[0]), grow(&kids[1])), (100, 0));
}

#[test]
fn the_header_carries_label_and_value() {
    let t = BaseTokens;
    let node = ProgressBar::new().value(64).label("Download").show_value(true).id("dl").build(&t);
    let (col, _, kids) = stack(&node);
    assert_eq!(col.id, Some("dl"));
    assert_eq!(col.gap, FxPx::new(8));
    let (head, _, texts) = stack(&kids[0]);
    assert_eq!(head.justify, Justify::SpaceBetween);
    match (&texts[0], &texts[1]) {
        (LayoutNode::Text(l, _), LayoutNode::Text(v, _)) => {
            assert_eq!(l.style.font_size, FxPx::new(13), "md = subheadline");
            assert_eq!(l.style.font_weight, FontWeight::Medium);
            assert_eq!(v.content.as_str(), "64 %");
            assert_eq!(v.style.color, t.color(ColorToken::LabelSecondary));
        }
        _ => panic!("header texts"),
    }
    assert_eq!(ProgressBar::new().indeterminate().value_text(), "…");
    let small = ProgressBar::new().size(ControlSize::Sm).label("x").build(&t);
    assert_eq!(stack(&small).0.gap, FxPx::new(4));
}

#[test]
fn circular_draws_track_arc_and_centre_value() {
    let t = BaseTokens;
    let node = ProgressBar::new()
        .appearance(ProgressAppearance::Circular)
        .value(25)
        .show_value(true)
        .label("Speicher")
        .build(&t);
    let (row, _, kids) = stack(&node);
    assert_eq!(row.gap, FxPx::new(8));
    let (ring, _, layers) = stack(&kids[0]);
    assert_eq!(ring.min_width, Some(FxPx::new(36)));
    assert_eq!(layers.len(), 3, "track · arc · centre value");
    let (track, tv, _) = stack(&layers[0]);
    assert_eq!(track.item.position, Position::Absolute);
    assert_eq!(tv.background, Some(t.color(ColorToken::Fill)));
    match (&tv.shape, &stack(&layers[1]).1.shape) {
        (ShapeKind::Stroke { paths: full, width_milli }, ShapeKind::Stroke { paths: part, .. }) => {
            assert_eq!(*width_milli, 111, "4 px of 36");
            assert_eq!(full[0].points.len(), 101);
            assert_eq!(part[0].points.len(), 26, "0..=25 %");
            // A quarter turn ends at 3 o'clock.
            let end = part[0].points[25];
            assert!(end.x_milli > 900 && (490..=510).contains(&end.y_milli));
        }
        _ => panic!("stroked ring layers"),
    }
    assert_eq!(kids.len(), 2, "ring + label column");
}

#[test]
fn small_rings_put_the_value_beside() {
    let t = BaseTokens;
    let node = ProgressBar::new()
        .appearance(ProgressAppearance::Circular)
        .size(ControlSize::Sm)
        .value(10)
        .show_value(true)
        .build(&t);
    let (_, _, kids) = stack(&node);
    assert_eq!(stack(&kids[0]).2.len(), 2, "track + arc, no centre text below md");
    assert!(
        matches!(&stack(&kids[1]).2[0], LayoutNode::Text(v, _) if v.content.as_str() == "10 %")
    );
}

#[test]
fn disabled_dims_to_half() {
    let node = ProgressBar::new().value(10).disabled(true).build(&BaseTokens);
    assert_eq!(stack(&node).1.opacity, Some(DISABLED_OPACITY));
}
