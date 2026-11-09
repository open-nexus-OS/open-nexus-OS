// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `Badge` (split from lib.rs): sizes, the ten looks, counts and dots.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_theme_tokens::{BaseTokens, DarkTokens};

fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(s, v, k) => (s, v, k),
        _ => panic!("Badge must build a Stack"),
    }
}

fn text(node: &LayoutNode) -> &TextNode {
    match &stack(node).2[0] {
        LayoutNode::Text(t, _) => t,
        _ => panic!("badge text"),
    }
}

#[test]
fn sizes_fix_height_min_width_text_and_padding() {
    let t = BaseTokens;
    for (size, h, px, pad) in [
        (ControlSize::Xs, 16, 10, 4),
        (ControlSize::Sm, 18, 10, 4),
        (ControlSize::Md, 22, 11, 8),
        (ControlSize::Lg, 26, 12, 8),
        (ControlSize::Xl, 30, 13, 12),
    ] {
        let node = Badge::new().size(size).label("Neu").build(&t);
        let (s, v, _) = stack(&node);
        assert_eq!(
            (s.min_height, s.max_height, s.min_width),
            (Some(FxPx::new(h)), Some(FxPx::new(h)), Some(FxPx::new(h)))
        );
        assert_eq!(s.padding.left, FxPx::new(pad + 1), "text padding + the 1 px border");
        assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px(), "capsule");
        let label = text(&node);
        assert_eq!(label.style.font_size, FxPx::new(px));
        assert_eq!(label.style.font_weight, FontWeight::Medium);
    }
}

#[test]
fn default_is_primary_and_secondary_the_fill() {
    let t = BaseTokens;
    let d = Badge::new();
    assert_eq!(d.style(&t).visual().background, Some(t.color(ColorToken::Primary)));
    assert_eq!(d.foreground(&t), t.color(ColorToken::OnPrimary));
    let s = Badge::new().variant(BadgeVariant::Secondary);
    assert_eq!(s.style(&t).visual().background, Some(t.color(ColorToken::Fill)));
    assert_eq!(s.foreground(&t), t.color(ColorToken::Label));
    assert_eq!(s.style(&t).visual().border.top, None, "transparent border is not drawn");
}

#[test]
fn status_badges_are_washes_with_the_status_as_text() {
    let t = DarkTokens;
    for (variant, role) in
        [(BadgeVariant::Success, ColorToken::Success), (BadgeVariant::Warning, ColorToken::Warning)]
    {
        let b = Badge::new().variant(variant);
        let v = b.style(&t).visual();
        let base = t.color(role);
        assert_eq!(v.background, Some(wash(base, 16)));
        assert_eq!(v.border.top.map(|e| e.color), Some(wash(base, 30)));
        assert_eq!(b.foreground(&t), base);
    }
}

#[test]
fn destructive_glass_outline_active() {
    let t = BaseTokens;
    let d = Badge::new().variant(BadgeVariant::Destructive);
    assert_eq!(d.style(&t).visual().background, Some(t.color(ColorToken::Danger)));
    assert_eq!(d.foreground(&t), t.color(ColorToken::OnAccent));
    let g = Badge::new().variant(BadgeVariant::Glass).style(&t);
    assert_eq!(g.visual().background, Some(t.color(ColorToken::GlassCard)));
    assert_eq!(g.visual().border.top.map(|e| e.color), Some(t.color(ColorToken::GlassEdge)));
    assert!(g.backdrop_blur().is_none(), "a glass badge never blurs");
    let o = Badge::new().variant(BadgeVariant::Outline).style(&t);
    assert_eq!(o.visual().background, None);
    assert_eq!(o.visual().border.top.map(|e| e.color), Some(t.color(ColorToken::Separator)));
    let a = Badge::new().variant(BadgeVariant::Active);
    assert_eq!(a.style(&t).visual().background, Some(t.color(ColorToken::Accent)));
}

#[test]
fn a_tint_alone_fills_and_tinted_washes() {
    let t = BaseTokens;
    let f = Badge::new().tint(ColorToken::ModeDesktop);
    assert_eq!(f.look(), BadgeVariant::Filled);
    assert_eq!(f.style(&t).visual().background, Some(t.color(ColorToken::ModeDesktop)));
    assert_eq!(f.foreground(&t), t.color(ColorToken::OnAccent));
    assert_eq!(
        Badge::new().tint(ColorToken::Primary).foreground(&t),
        t.color(ColorToken::OnPrimary)
    );
    let w = Badge::new().tint(ColorToken::Chart2).variant(BadgeVariant::Tinted);
    assert_eq!(w.style(&t).visual().background, Some(wash(t.color(ColorToken::Chart2), 16)));
    assert_eq!(w.foreground(&t), t.color(ColorToken::Chart2));
}

#[test]
fn counts_are_semibold_with_number_padding_and_capped() {
    let t = BaseTokens;
    let node = Badge::new().count(120).max(99).build(&t);
    let (s, _, _) = stack(&node);
    assert_eq!(s.padding.left, FxPx::new(4 + 1), "md number padding 4");
    let n = text(&node);
    assert_eq!(n.content.as_str(), "99+");
    assert_eq!(n.style.font_weight, FontWeight::Semibold);
    // A label of digits is a count too.
    assert_eq!(Badge::new().label("7").text().as_deref(), Some("7"));
    assert!(
        matches!(&stack(&Badge::new().label("7").build(&t)).2[0], LayoutNode::Text(l, _) if l.style.font_weight == FontWeight::Semibold)
    );
}

#[test]
fn a_dot_is_a_bare_circle_of_the_size() {
    let t = BaseTokens;
    let node =
        Badge::new().dot(true).variant(BadgeVariant::Success).size(ControlSize::Lg).build(&t);
    let (s, v, kids) = stack(&node);
    assert_eq!((s.min_width, s.max_height), (Some(FxPx::new(12)), Some(FxPx::new(12))));
    assert!(kids.is_empty());
    assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
    assert!(v.border.top.is_some(), "the 1 px variant border");
}

#[test]
fn caller_content_still_builds_a_centred_capsule_with_id() {
    let t = BaseTokens;
    let content = LayoutNode::Stack(Stack::default(), VisualStyle::default(), Vec::new());
    let node = Badge::new().id("count").variant(BadgeVariant::Active).content(content).build(&t);
    let (s, v, kids) = stack(&node);
    assert_eq!(s.id, Some("count"));
    assert_eq!(s.align, Align::Center);
    assert_eq!(kids.len(), 1);
    assert!(v.background.is_some());
}
