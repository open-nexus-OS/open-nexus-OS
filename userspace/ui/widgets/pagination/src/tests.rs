// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for the page navigators — the truncation rule, the 36 px glass buttons
//! and the accent current page on the real layout engine, the arrows' end dims and ids, and the
//! page dots' sizes, stretch and alphas.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: Pagination, PageControl
//! TEST_SCENARIOS: truncation, geometry, current page, arrows, ellipsis, dots
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use super::*;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::{Fraction, Rgba8};
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::{BaseTokens, ControlSize};

fn boxes(node: &LayoutNode) -> Vec<LayoutBox> {
    let host = LayoutNode::Stack(
        Stack { direction: Direction::Row, ..Stack::default() },
        VisualStyle::default(),
        alloc::vec![node.clone()],
    );
    LayoutEngine::new().layout(&host, FxPx::new(800), &BakedTextMeasure).expect("layout").boxes
}

#[test]
fn the_trail_always_truncates() {
    assert_eq!(
        Pagination::new(7).page(1).visible_pages(),
        alloc::vec![Some(1), Some(2), None, Some(7)]
    );
    assert_eq!(
        Pagination::new(20).page(10).visible_pages(),
        alloc::vec![Some(1), None, Some(9), Some(10), Some(11), None, Some(20)]
    );
    assert_eq!(
        Pagination::new(5).page(3).visible_pages(),
        alloc::vec![Some(1), Some(2), Some(3), Some(4), Some(5)]
    );
    assert_eq!(Pagination::new(1).visible_pages(), alloc::vec![Some(1)]);
    assert_eq!(Pagination::new(2).visible_pages(), alloc::vec![Some(1), Some(2)]);
}

#[test]
fn buttons_are_36_px_glass_and_the_current_page_is_accent() {
    let t = BaseTokens;
    let node = Pagination::new(5)
        .page(2)
        .page_ids(alloc::vec!["p1", "p2", "p3", "p4", "p5"])
        .arrow_ids("prev", "next")
        .id("pg")
        .build(&t);
    let all = boxes(&node);
    for id in ["prev", "next", "p1", "p2"] {
        let b = all.iter().find(|b| b.id == Some(id)).expect("button");
        assert_eq!(b.rect.height, FxPx::new(36), "{id}");
        assert!(b.rect.width >= FxPx::new(36), "{id}");
    }
    let prev = all.iter().find(|b| b.id == Some("prev")).expect("prev");
    assert_eq!(prev.rect.width, FxPx::new(36), "a circle");
    let LayoutNode::Stack(stack, _, children) = &node else { panic!() };
    assert_eq!((stack.id, stack.gap), (Some("pg"), FxPx::new(4)));
    assert_eq!(children.len(), 7, "prev + 5 pages + next");
    match (&children[1], &children[2]) {
        (LayoutNode::Stack(_, rest, rk), LayoutNode::Stack(_, cur, ck)) => {
            assert_eq!(rest.background, Some(t.color(ColorToken::GlassCard)));
            assert_eq!(rest.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
            assert!(rest.inset_highlight.is_some());
            assert_eq!(cur.background, Some(t.color(ColorToken::Accent)));
            assert_eq!(cur.border.top, None);
            assert_eq!(cur.shadow.map(|s| s.color), Some(t.color(ColorToken::AccentGlow)));
            match (&rk[0], &ck[0]) {
                (LayoutNode::Text(r, _), LayoutNode::Text(c, _)) => {
                    assert_eq!(
                        (r.style.font_size.0, r.style.font_weight),
                        (14, FontWeight::Medium)
                    );
                    assert_eq!(c.style.font_weight, FontWeight::Semibold);
                    assert_eq!(c.style.color, t.color(ColorToken::OnAccent));
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn the_arrows_dim_at_the_ends_and_gaps_are_28_wide() {
    let t = BaseTokens;
    let node = Pagination::new(20).page(1).build(&t);
    let LayoutNode::Stack(_, _, children) = &node else { panic!() };
    match (&children[0], children.last()) {
        (LayoutNode::Stack(_, prev, pk), Some(LayoutNode::Stack(_, next, _))) => {
            assert_eq!(prev.opacity, Some(Fraction::new(102)), "first page: no previous");
            assert_eq!(next.opacity, None);
            match &pk[0] {
                LayoutNode::Stack(gs, _, _) => assert_eq!(gs.min_width, Some(FxPx::new(15))),
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
    let gap = children
        .iter()
        .find(|n| matches!(n, LayoutNode::Stack(s, _, _) if s.max_width == Some(FxPx::new(28))));
    assert!(gap.is_some(), "an ellipsis slot");
}

#[test]
fn the_current_dot_stretches_and_the_others_fade() {
    let t = BaseTokens;
    let pc = PageControl::new(3).page(1).dot_ids(alloc::vec!["d0", "d1", "d2"]);
    assert_eq!(pc.dot_metrics(), (8, 8));
    let node = pc.build(&t);
    let all = boxes(&node);
    let d1 = all.iter().find(|b| b.id == Some("d1")).expect("dot cell");
    assert_eq!(d1.rect.height, FxPx::new(20), "hit height max(20, d + 12)");
    assert_eq!(d1.rect.width, FxPx::new(8 + 12 + 8), "d + 1.5 d + spacing");
    let label = t.color(ColorToken::Label);
    let dot_bg = |i: usize| match &node {
        LayoutNode::Stack(_, _, cells) => match &cells[i] {
            LayoutNode::Stack(_, _, dot) => match &dot[0] {
                LayoutNode::Stack(_, v, _) => v.background,
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    assert_eq!(dot_bg(1), Some(Rgba8 { a: (u32::from(label.a) * 92 / 100) as u8, ..label }));
    assert_eq!(dot_bg(0), Some(Rgba8 { a: (u32::from(label.a) * 30 / 100) as u8, ..label }));
    match PageControl::new(2).size(ControlSize::Xl).prominent(true).disabled(true).build(&t) {
        LayoutNode::Stack(s, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
            assert!(v.border.top.is_some());
            assert_eq!(s.padding.left, FxPx::new(12));
            assert_eq!(v.opacity, Some(Fraction::new(115)));
        }
        _ => panic!(),
    }
}
