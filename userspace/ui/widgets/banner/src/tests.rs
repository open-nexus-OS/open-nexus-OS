// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `Banner` (split from lib.rs): the wash surface, the status icon,
//! the text column, the action and the dismiss circle.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_theme_tokens::{BaseTokens, DarkTokens};

fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(s, v, k) => (s, v, k),
        _ => panic!("expected a stack"),
    }
}

#[test]
fn the_strip_is_the_status_wash_without_blur_or_border() {
    let t = DarkTokens;
    for variant in [
        BannerVariant::Info,
        BannerVariant::Success,
        BannerVariant::Warning,
        BannerVariant::Destructive,
    ] {
        let node = Banner::new().variant(variant).message("x").build(&t);
        let (s, v, _) = stack(&node);
        assert_eq!(v.background, Some(wash(t.color(variant.role()), 16)));
        assert_eq!(v.border.top, None);
        assert_eq!(v.material, None, "no blur: it sits on a surface");
        assert_eq!(v.corner_radius.top_left, FxPx::new(12));
        assert_eq!(s.padding, EdgeInsets::symmetric(FxPx::new(12), FxPx::new(16)));
        assert_eq!((s.gap, s.align), (FxPx::new(12), Align::Start));
    }
    assert_eq!(BannerVariant::Info.role(), ColorToken::Accent);
}

#[test]
fn full_banner_has_icon_text_column_and_dismiss() {
    let t = BaseTokens;
    let node = Banner::new()
        .variant(BannerVariant::Warning)
        .title("Speicher fast voll")
        .message("Noch 1,2 GB frei.")
        .action("Verwalten")
        .action_id("banner_action")
        .dismissible(true)
        .dismiss_id("banner_dismiss")
        .build(&t);
    let (_, _, kids) = stack(&node);
    assert_eq!(kids.len(), 3, "icon + text column + dismiss");
    let (icon, iv, _) = stack(&kids[0]);
    assert_eq!(icon.min_width, Some(FxPx::new(18)));
    assert_eq!(icon.item.margin.top, FxPx::new(1));
    assert_eq!(iv.background, Some(t.color(ColorToken::Warning)));
    assert!(matches!(iv.shape, ShapeKind::Stroke { width_milli: 83, .. }));
    let (col, _, lines) = stack(&kids[1]);
    assert_eq!((col.gap, col.item.flex_grow), (FxPx::new(2), 1));
    assert_eq!(lines.len(), 3, "title · message · action");
    match (&lines[0], &lines[1]) {
        (LayoutNode::Text(title, _), LayoutNode::Text(msg, _)) => {
            assert_eq!(title.style.font_weight, FontWeight::Semibold);
            assert_eq!(title.style.font_size, FxPx::new(14));
            assert_eq!(msg.style.font_size, FxPx::new(13));
            assert_eq!(msg.style.color, t.color(ColorToken::Label));
        }
        _ => panic!("title and message"),
    }
    let (action, _, label) = stack(&lines[2]);
    assert_eq!(action.id, Some("banner_action"));
    assert_eq!(action.item.align_self, Some(Align::Start));
    assert!(
        matches!(&label[0], LayoutNode::Text(a, _) if a.style.color == t.color(ColorToken::Warning) && a.style.font_weight == FontWeight::Semibold)
    );
    let (dismiss, dv, cross) = stack(&kids[2]);
    assert_eq!(dismiss.id, Some("banner_dismiss"));
    assert_eq!(dismiss.min_width, Some(FxPx::new(22)));
    assert_eq!(dv.background, None, "transparent circle");
    assert_eq!(stack(&cross[0]).0.min_width, Some(FxPx::new(12)));
}

#[test]
fn message_only_banner_is_minimal() {
    let node =
        Banner::new().variant(BannerVariant::Success).message("Synchronisiert").build(&BaseTokens);
    assert_eq!(stack(&node).2.len(), 2, "icon + text column");
}

#[test]
fn a_caller_icon_replaces_the_status_icon() {
    let custom = LayoutNode::Stack(
        Stack { id: Some("mine"), ..Stack::default() },
        VisualStyle::default(),
        Vec::new(),
    );
    let node = Banner::new().icon(custom).message("x").build(&BaseTokens);
    assert_eq!(stack(&stack(&node).2[0]).0.id, Some("mine"));
}

#[test]
fn every_icon_stays_inside_the_viewbox() {
    for paths in [INFO, SUCCESS, WARNING, DESTRUCTIVE, CROSS] {
        assert!(paths
            .iter()
            .all(|p| !p.is_empty() && p.iter().all(|&(x, y)| x <= 1000 && y <= 1000)));
    }
}
