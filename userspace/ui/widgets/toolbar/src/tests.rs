// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for the panel/sheet toolbar — 52 px bar with 16 px sides on the real
//! layout engine, the title and subtitle roles, the panel's bottom hairline, the clusters and the
//! centred title.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: Toolbar
//! TEST_SCENARIOS: geometry, roles, panel vs transparent, clusters, centred title
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use super::*;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::FontWeight;
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::BaseTokens;

fn btn(id: &'static str) -> LayoutNode {
    LayoutNode::Stack(
        Stack {
            id: Some(id),
            min_width: Some(FxPx::new(36)),
            max_width: Some(FxPx::new(36)),
            min_height: Some(FxPx::new(36)),
            max_height: Some(FxPx::new(36)),
            ..Stack::default()
        },
        VisualStyle::default(),
        alloc::vec![],
    )
}

fn boxes(node: &LayoutNode) -> Vec<LayoutBox> {
    LayoutEngine::new().layout(node, FxPx::new(600), &BakedTextMeasure).expect("layout").boxes
}

fn by_id<'a>(all: &'a [LayoutBox], id: &str) -> &'a LayoutBox {
    all.iter().find(|b| b.id == Some(id)).expect("box with id")
}

#[test]
fn the_bar_is_52_with_16_px_sides_and_8_px_clusters() {
    let t = BaseTokens;
    let node = Toolbar::new()
        .title("Einstellungen")
        .leading(btn("back"))
        .trailing(btn("a"))
        .trailing(btn("b"))
        .id("bar")
        .build(&t);
    let all = boxes(&node);
    assert_eq!(by_id(&all, "bar").rect.height, FxPx::new(52));
    let (back, a, b) = (by_id(&all, "back"), by_id(&all, "a"), by_id(&all, "b"));
    assert_eq!(back.rect.x, FxPx::new(16));
    assert_eq!(back.rect.y, FxPx::new(8), "centred in 52");
    assert_eq!(b.rect.x + b.rect.width, FxPx::new(600 - 16));
    assert_eq!(b.rect.x - (a.rect.x + a.rect.width), FxPx::new(8));
}

#[test]
fn title_is_headline_label_subtitle_caption_secondary() {
    let t = BaseTokens;
    let node = Toolbar::new().title("Dateien").subtitle("3 Objekte").build(&t);
    let LayoutNode::Stack(_, _, parts) = &node else { panic!() };
    let LayoutNode::Stack(s, _, col) = &parts[1] else { panic!("title column") };
    assert_eq!(s.item.flex_grow, 1);
    match (&col[0], &col[1]) {
        (LayoutNode::Text(title, _), LayoutNode::Text(sub, _)) => {
            assert_eq!(
                (title.style.font_size.0, title.style.font_weight),
                (15, FontWeight::Semibold)
            );
            assert_eq!(title.style.color, t.color(ColorToken::Label));
            assert_eq!(
                (sub.style.font_size.0, sub.style.line_height),
                (11, nexus_layout_types::LineHeight::Absolute(FxPx::new(14)))
            );
            assert_eq!(sub.style.color, t.color(ColorToken::LabelSecondary));
        }
        _ => panic!(),
    }
}

#[test]
fn panel_has_glass_card_and_a_bottom_hairline_only() {
    let t = BaseTokens;
    match Toolbar::new().title("x").build(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
            assert_eq!(v.border.bottom.map(|b| b.color), Some(t.color(ColorToken::Separator)));
            assert!(v.border.top.is_none() && v.border.left.is_none() && v.border.right.is_none());
        }
        _ => panic!(),
    }
    match Toolbar::new().variant(ToolbarVariant::Transparent).title("x").build(&t) {
        LayoutNode::Stack(_, v, _) => assert_eq!(v, VisualStyle::default()),
        _ => panic!(),
    }
}

#[test]
fn a_centred_title_keeps_the_middle() {
    let t = BaseTokens;
    let node = Toolbar::new().title("Titel").center_title(true).leading(btn("back")).build(&t);
    let LayoutNode::Stack(_, _, parts) = &node else { panic!() };
    assert_eq!(parts.len(), 3, "leading, title, trailing");
    for i in [0, 2] {
        match &parts[i] {
            LayoutNode::Stack(s, _, _) => assert_eq!(s.min_width, Some(FxPx::new(60))),
            _ => panic!(),
        }
    }
    // Both clusters are 60 wide, so the title column (16 + 60 + 12 … 600 − 16 − 60 − 12) is
    // centred on the bar and the title centred in it.
    let all = boxes(&node);
    let title = all.iter().find(|b| b.rect.height == FxPx::new(20)).expect("title text");
    let centre = title.rect.x.0 + title.rect.width.0 / 2;
    assert!((centre - 300).abs() <= 1, "title centre {centre}");
}
