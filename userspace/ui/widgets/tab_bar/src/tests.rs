// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for the tab bar — item geometry on the real layout engine (53 px with a
//! 20 px icon, min 60 wide, the bar's 4 px padding and 2 px gaps), the dimmed / active / danger
//! inks, the indicator, the three surfaces, dividers, badges and the full-width share.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: TabBar
//! TEST_SCENARIOS: geometry, inks, indicator, surfaces, divider, badge, full width, rail
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use super::*;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::{BaseTokens, DarkTokens};
use nexus_widget_icon::{Icon, LucideSymbol};

/// Lays `node` out as the child of a row (the engine's root fills the viewport).
fn boxes(node: &LayoutNode, width: i32) -> Vec<LayoutBox> {
    let host = LayoutNode::Stack(
        Stack { direction: Direction::Row, ..Stack::default() },
        VisualStyle::default(),
        alloc::vec![node.clone()],
    );
    LayoutEngine::new().layout(&host, FxPx::new(width), &BakedTextMeasure).expect("layout").boxes
}

fn by_id<'a>(all: &'a [LayoutBox], id: &str) -> &'a LayoutBox {
    all.iter().find(|b| b.id == Some(id)).expect("box with id")
}

fn icon(t: &dyn Tokens) -> LayoutNode {
    Icon::lucide(LucideSymbol::House).size(20).build(t)
}

fn bar(t: &dyn Tokens) -> TabBar {
    TabBar::new().active(1).tabs(alloc::vec![
        TabItem::new("Start").icon(icon(t)).id("start"),
        TabItem::new("Nachrichten").icon(icon(t)).badge("3").id("msg"),
        TabItem::divider(),
        TabItem::new("Löschen").icon(icon(t)).danger(true).id("del"),
    ])
}

fn slot_parts(node: &LayoutNode, i: usize) -> (&Stack, &VisualStyle, &[LayoutNode]) {
    match node {
        LayoutNode::Stack(_, _, slots) => match &slots[i] {
            LayoutNode::Stack(s, v, c) => (s, v, c),
            _ => panic!("slot"),
        },
        _ => panic!("bar"),
    }
}

fn label_color(parts: &[LayoutNode]) -> Rgba8 {
    match parts.last() {
        Some(LayoutNode::Text(n, _)) => n.style.color,
        _ => panic!("label"),
    }
}

#[test]
fn items_are_53_tall_with_a_20_px_icon_and_the_bar_pads_4() {
    let t = BaseTokens;
    let all = boxes(&bar(&t).id("bar").build(&t), 800);
    let start = by_id(&all, "start");
    assert_eq!(start.rect.height, FxPx::new(8 + 20 + 4 + 13 + 8));
    assert!(start.rect.width >= FxPx::new(60));
    // 1 px edge + 4 px padding around the items.
    let b = by_id(&all, "bar");
    assert_eq!(b.rect.height, start.rect.height + FxPx::new(10));
    assert_eq!(start.rect.x - b.rect.x, FxPx::new(5));
    let msg = by_id(&all, "msg");
    assert_eq!(msg.rect.x, start.rect.x + start.rect.width + FxPx::new(2), "2 px apart");
}

#[test]
fn inks_dim_at_rest_and_light_up_active_or_danger() {
    let t = BaseTokens;
    let node = bar(&t).build(&t);
    let (_, rest_v, rest) = slot_parts(&node, 0);
    let (_, active_v, active) = slot_parts(&node, 1);
    let (_, _, danger) = slot_parts(&node, 3);
    assert_eq!(label_color(rest), fade(t.color(ColorToken::Label), 62));
    assert_eq!(label_color(active), t.color(ColorToken::Accent));
    assert_eq!(label_color(danger), t.color(ColorToken::Danger));
    // The indicator is the active item's fill capsule; a resting item has no background.
    assert_eq!(active_v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(active_v.corner_radius.top_left, RadiusToken::Full.px());
    assert_eq!(rest_v.background, None);
    // Labels: caption2 semibold, tracked +0.02 em.
    match rest.last() {
        Some(LayoutNode::Text(n, _)) => {
            assert_eq!((n.style.font_size.0, n.style.font_weight), (10, FontWeight::Semibold));
            assert_eq!(n.style.tracking_milli_em, 20);
        }
        _ => panic!(),
    }
    // The icon takes the item's ink.
    match &rest[0] {
        LayoutNode::Stack(_, _, holder) => match &holder[0] {
            LayoutNode::Stack(_, iv, _) => {
                assert_eq!(iv.background, Some(fade(t.color(ColorToken::Label), 62)))
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn surfaces_glass_overlay_none() {
    let t = BaseTokens;
    match bar(&t).build(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
            assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::Separator)));
            assert!(v.shadow.is_some() && v.inset_highlight.is_some(), "floating: shadow2 + lit");
            assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
        }
        _ => panic!(),
    }
    match bar(&t).floating(false).build(&t) {
        LayoutNode::Stack(s, v, _) => {
            assert!(v.shadow.is_none() && v.inset_highlight.is_some());
            assert_eq!(v.corner_radius.top_left, FxPx::new(24));
            assert_eq!(s.justify, Justify::SpaceAround);
        }
        _ => panic!(),
    }
    let d = DarkTokens;
    match bar(&d).surface(TabBarSurface::Overlay).build(&d) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(d.color(ColorToken::SurfaceOverlay)));
            assert_eq!(v.border.top.map(|b| b.color), Some(d.color(ColorToken::GlassEdge)));
            assert_eq!(v.shadow.map(|s| s.blur_radius), Some(FxPx::new(30)));
        }
        _ => panic!(),
    }
    match bar(&t).surface(TabBarSurface::None).build(&t) {
        LayoutNode::Stack(s, v, _) => {
            assert_eq!((v.background, v.shadow, v.border.top), (None, None, None));
            assert_eq!(s.padding, EdgeInsets::all(FxPx::ONE));
        }
        _ => panic!(),
    }
}

#[test]
fn dividers_and_badges() {
    let t = BaseTokens;
    let node = bar(&t).build(&t);
    let (div, div_v, _) = slot_parts(&node, 2);
    assert_eq!((div.min_width, div.max_width), (Some(FxPx::ONE), Some(FxPx::ONE)));
    assert_eq!(div.item.margin, EdgeInsets::symmetric(FxPx::new(8), FxPx::new(4)));
    assert_eq!(div_v.background, Some(t.color(ColorToken::Separator)));
    let (_, _, msg) = slot_parts(&node, 1);
    match &msg[0] {
        LayoutNode::Stack(_, _, holder) => match &holder[1] {
            LayoutNode::Stack(bs, bv, _) => {
                assert_eq!(bs.item.position, Position::Absolute);
                assert_eq!(
                    (bs.item.margin.top, bs.item.margin.left),
                    (FxPx::new(-6), FxPx::new(13))
                );
                assert_eq!(bs.max_height, Some(FxPx::new(16)));
                assert_eq!(bv.background, Some(t.color(ColorToken::Danger)));
            }
            _ => panic!("badge"),
        },
        _ => panic!(),
    }
}

#[test]
fn full_width_items_share_the_bar_and_a_rail_stacks() {
    let t = BaseTokens;
    let node = TabBar::new()
        .floating(false)
        .tabs(alloc::vec![TabItem::new("A").id("a"), TabItem::new("Bbbbbbbbbbbb").id("b")])
        .build(&t);
    let host = LayoutNode::Stack(
        Stack { direction: Direction::Column, align: Align::Stretch, ..Stack::default() },
        VisualStyle::default(),
        alloc::vec![node],
    );
    let all =
        LayoutEngine::new().layout(&host, FxPx::new(400), &BakedTextMeasure).expect("layout").boxes;
    let (a, b) = (by_id(&all, "a"), by_id(&all, "b"));
    assert_eq!(a.rect.width, b.rect.width, "equal shares");
    let rail = TabBar::new()
        .vertical(true)
        .show_labels(false)
        .surface(TabBarSurface::None)
        .tabs(alloc::vec![TabItem::new("A").icon(icon(&t)).id("a")])
        .build(&t);
    let all = boxes(&rail, 400);
    let a = by_id(&all, "a");
    assert_eq!((a.rect.width, a.rect.height), (FxPx::new(56), FxPx::new(44)));
    let (_, v, _) = slot_parts(&rail, 0);
    assert_eq!(v.corner_radius.top_left, FxPx::new(20));
}
