// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `AppIcon` (split from lib.rs): sizes, the two modes, the badge
//! outside the clip, the active dot and the label.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_theme_tokens::BaseTokens;

fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(s, v, k) => (s, v, k),
        _ => panic!("expected a stack"),
    }
}

/// (column, icon area, tile) of a built icon.
fn parts(node: &LayoutNode) -> (&Vec<LayoutNode>, &Vec<LayoutNode>, (&Stack, &VisualStyle)) {
    let (_, _, col) = stack(node);
    let (_, _, area) = stack(&col[0]);
    let (ts, tv, _) = stack(&area[0]);
    (col, area, (ts, tv))
}

#[test]
fn sizes_set_tile_and_radius() {
    let t = BaseTokens;
    for (size, dim, radius) in [
        (AppIconSize::Xs, 40, 12),
        (AppIconSize::Sm, 48, 12),
        (AppIconSize::Md, 56, 16),
        (AppIconSize::Lg, 72, 24),
        (AppIconSize::Xl, 88, 24),
    ] {
        let node = AppIcon::new().size(size).build(&t);
        let (_, _, (tile, v)) = parts(&node);
        assert_eq!((tile.min_width, tile.max_height), (Some(FxPx::new(dim)), Some(FxPx::new(dim))));
        assert_eq!(v.corner_radius.top_left, FxPx::new(radius), "{size:?}");
    }
}

#[test]
fn native_is_a_clipped_tile_with_shadow2_and_no_glass() {
    let t = BaseTokens;
    let node = AppIcon::new().build(&t);
    let (_, _, (tile, v)) = parts(&node);
    assert_eq!(tile.overflow, Overflow::Hidden);
    assert_eq!(v.background, None, "the icon IS the tile — no backing");
    assert_eq!(v.material, None);
    let s2 = Style::new().shadow_token(&t, ShadowToken::Shadow2).visual().shadow;
    assert_eq!(v.shadow, s2);
    assert!(AppIcon::new().style(&t).backdrop_blur().is_none());
}

#[test]
fn freestanding_is_bare() {
    let t = BaseTokens;
    let node = AppIcon::new().variant(AppIconVariant::Freestanding).build(&t);
    let (_, _, (tile, v)) = parts(&node);
    assert_eq!(tile.overflow, Overflow::Visible);
    assert_eq!(v.shadow, None);
    assert_eq!(v.corner_radius.top_left, FxPx::ZERO);
}

#[test]
fn the_badge_sits_outside_the_clip_at_the_top_right() {
    let t = BaseTokens;
    let node = AppIcon::new().badge(120).build(&t);
    let (_, area, _) = parts(&node);
    assert_eq!(area.len(), 2, "tile + badge layer, siblings — never inside the clip");
    let (layer, _, badge) = stack(&area[1]);
    assert_eq!(layer.item.position, Position::Absolute);
    assert_eq!((layer.item.margin.top, layer.item.margin.right), (FxPx::new(-4), FxPx::new(-4)));
    assert_eq!(layer.justify, Justify::End);
    let (b, bv, text) = stack(&badge[0]);
    assert_eq!((b.min_width, b.max_height), (Some(FxPx::new(18)), Some(FxPx::new(18))));
    assert_eq!(bv.background, Some(t.color(ColorToken::Danger)));
    assert!(bv.shadow.is_some(), "shadow1");
    match &text[0] {
        LayoutNode::Text(n, _) => {
            assert_eq!(n.content.as_str(), "99+");
            assert_eq!(n.style.font_weight, FontWeight::Bold);
            assert_eq!(n.style.font_size, FxPx::new(10));
            assert_eq!(n.style.color, t.color(ColorToken::OnAccent));
        }
        _ => panic!("badge text"),
    }
    // Freestanding art insets the badge: 12 % from the top, 2 % from the right.
    let node = AppIcon::new().variant(AppIconVariant::Freestanding).badge(3).build(&t);
    let (_, area, _) = parts(&node);
    let (layer, _, _) = stack(&area[1]);
    assert_eq!((layer.item.margin.top, layer.item.margin.right), (FxPx::new(7), FxPx::new(1)));
    // Zero = hidden.
    assert_eq!(AppIcon::new().badge(0).badge_count(), None);
}

#[test]
fn active_dot_and_label_follow_the_tile() {
    let t = BaseTokens;
    let node = AppIcon::new().active(true).label("Einstellungen").id("app").build(&t);
    let (col_stack, _, col) = stack(&node);
    assert_eq!(col_stack.id, Some("app"));
    assert_eq!(col_stack.gap, FxPx::new(8));
    assert_eq!(col.len(), 3, "icon area · dot · label");
    let (dot, dv, _) = stack(&col[1]);
    assert_eq!(dot.min_width, Some(FxPx::new(4)));
    assert_eq!(dot.item.margin.top, FxPx::new(-4));
    assert!(dv.background.is_some_and(|c| c.a < t.color(ColorToken::Label).a));
    match &col[2] {
        LayoutNode::Text(l, v) => {
            assert_eq!(l.style.font_size, FxPx::new(13), "md label = subheadline");
            assert_eq!(l.style.font_weight, FontWeight::Semibold);
            assert_eq!(l.style.text_align, TextAlign::Center);
            assert_eq!(l.max_width, Some(FxPx::new(72)));
            assert_eq!(v.text_shadow.map(|s| s.color), Some(t.color(ColorToken::LabelShadow)));
        }
        _ => panic!("label"),
    }
    match &stack(&AppIcon::new().label("x").halo(false).build(&t)).2[1] {
        LayoutNode::Text(_, v) => assert_eq!(v.text_shadow, None),
        _ => panic!("label"),
    }
}
