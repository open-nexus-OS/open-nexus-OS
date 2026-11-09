// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for the navigation rail — item and header geometry on the real layout
//! engine (36 px items, 30 px overline headers, 248 px rail), the active pill's roles and glow,
//! the badge, the panel's trailing hairline, the slots, and the split view's full-height panes.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: Sidebar, SplitView
//! TEST_SCENARIOS: geometry, active roles, badge, panel vs plain, slots, split view
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use super::*;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::Spacer;
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::BaseTokens;
use nexus_widget_icon::{Icon, LucideSymbol};

/// Lays `node` out as the child of a row (the engine's root fills the viewport; a placed rail
/// keeps its own width).
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

fn rail(t: &dyn Tokens) -> LayoutNode {
    Sidebar::new(alloc::vec![
        SidebarItem::header("Bibliothek"),
        SidebarItem::item("all", "Alle Dateien").icon(Icon::lucide(LucideSymbol::Folder).build(t)),
        SidebarItem::item("shared", "Geteilt").badge(2),
    ])
    .active("all")
    .id("rail")
    .build(t)
}

#[test]
fn items_are_36_headers_30_and_the_rail_248() {
    let t = BaseTokens;
    let all = boxes(&rail(&t), 1000);
    let rail_box = by_id(&all, "rail");
    assert_eq!(rail_box.rect.width, FxPx::new(SIDEBAR_WIDTH));
    let (a, s) = (by_id(&all, "all"), by_id(&all, "shared"));
    assert_eq!((a.rect.height, s.rect.height), (FxPx::new(36), FxPx::new(36)));
    // List padding 4 8: header (12 + 14 + 4 = 30) at y 4, the first item 2 px under it.
    assert_eq!(a.rect.y, FxPx::new(4 + 30 + 2));
    assert_eq!((a.rect.x, a.rect.width), (FxPx::new(8), FxPx::new(SIDEBAR_WIDTH - 16)));
    assert_eq!(s.rect.y, a.rect.y + FxPx::new(36 + 2));
}

#[test]
fn the_active_item_is_the_accent_pill() {
    let t = BaseTokens;
    let node = rail(&t);
    let LayoutNode::Stack(_, _, col) = &node else { panic!() };
    let LayoutNode::Stack(_, _, rows) = &col[0] else { panic!("item list") };
    // Header: an upper-case overline in the secondary label.
    match &rows[0] {
        LayoutNode::Stack(_, _, kids) => match &kids[0] {
            LayoutNode::Text(n, _) => {
                assert_eq!(n.content.as_str(), "BIBLIOTHEK");
                assert_eq!(n.style.color, t.color(ColorToken::LabelSecondary));
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
    match &rows[1] {
        LayoutNode::Stack(s, v, kids) => {
            assert_eq!(s.id, Some("all"));
            assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
            assert_eq!(v.corner_radius.top_left, FxPx::new(12));
            assert_eq!(v.shadow.map(|s| s.color), Some(t.color(ColorToken::AccentGlow)));
            // Icon inked onAccent, label onAccent semibold.
            match (&kids[0], &kids[1]) {
                (LayoutNode::Stack(_, iv, _), LayoutNode::Text(n, _)) => {
                    assert_eq!(iv.background, Some(t.color(ColorToken::OnAccent)));
                    assert_eq!(n.style.color, t.color(ColorToken::OnAccent));
                    assert_eq!(n.style.font_weight, FontWeight::Semibold);
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
    match &rows[2] {
        LayoutNode::Stack(_, v, kids) => {
            assert_eq!((v.background, v.shadow), (None, None));
            match (&kids[0], &kids[1]) {
                (LayoutNode::Text(n, _), LayoutNode::Stack(bs, bv, _)) => {
                    assert_eq!(n.style.font_weight, FontWeight::Medium);
                    assert_eq!(n.style.color, t.color(ColorToken::Label));
                    assert_eq!(bs.max_height, Some(FxPx::new(18)));
                    assert_eq!(bv.background, Some(t.color(ColorToken::Danger)));
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn an_inactive_icon_is_the_label_at_85_percent() {
    let t = BaseTokens;
    let node = Sidebar::new(alloc::vec![
        SidebarItem::item("a", "A").icon(Icon::lucide(LucideSymbol::Folder).build(&t))
    ])
    .build(&t);
    let LayoutNode::Stack(_, _, col) = &node else { panic!() };
    let LayoutNode::Stack(_, _, rows) = &col[0] else { panic!() };
    let LayoutNode::Stack(_, _, kids) = &rows[0] else { panic!() };
    let LayoutNode::Stack(_, iv, _) = &kids[0] else { panic!() };
    let label = t.color(ColorToken::Label);
    assert_eq!(iv.background, Some(fade(label, 85)));
}

#[test]
fn panel_is_glass_card_with_a_trailing_hairline_plain_is_bare() {
    let t = BaseTokens;
    match rail(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
            assert!(v.border.right.is_some());
            assert!(v.border.top.is_none() && v.border.left.is_none() && v.border.bottom.is_none());
        }
        _ => panic!(),
    }
    match Sidebar::new(alloc::vec![SidebarItem::item("a", "A")]).plain(true).build(&t) {
        LayoutNode::Stack(_, v, _) => assert_eq!(v, VisualStyle::default()),
        _ => panic!(),
    }
}

#[test]
fn slots_pad_and_the_footer_sits_under_a_hairline() {
    let t = BaseTokens;
    let account = Text::new("Jenning").build(&t);
    let title = Text::new("Navigation").build(&t);
    let node = Sidebar::new(alloc::vec![SidebarItem::item("a", "A")])
        .header(title)
        .footer(account)
        .build(&t);
    let LayoutNode::Stack(_, _, col) = &node else { panic!() };
    assert_eq!(col.len(), 3, "header, items, footer");
    match (&col[0], &col[2]) {
        (LayoutNode::Stack(h, _, _), LayoutNode::Stack(f, fv, _)) => {
            assert_eq!(
                (h.padding.top, h.padding.left, h.padding.bottom),
                (FxPx::new(12), FxPx::new(16), FxPx::new(4))
            );
            assert_eq!(f.padding, EdgeInsets::all(FxPx::new(12)));
            assert!(fv.border.top.is_some());
        }
        _ => panic!(),
    }
}

#[test]
fn split_view_keeps_the_rail_full_height_and_the_content_flexible() {
    let t = BaseTokens;
    let content = LayoutNode::Spacer(Spacer {
        id: Some("content"),
        flex_grow: 1,
        min_size: None,
        item: FlexItem::default(),
    });
    let node = SplitView::new(rail(&t), content).id("split").build();
    let all = LayoutEngine::new()
        .layout_with_viewport(&node, FxPx::new(900), Some(FxPx::new(600)), &BakedTextMeasure)
        .expect("layout")
        .boxes;
    let rail_box = by_id(&all, "rail");
    assert_eq!(rail_box.rect.height, FxPx::new(600), "the rail spans the full height");
    let LayoutNode::Stack(s, _, panes) = &node else { panic!() };
    assert_eq!(s.align, Align::Stretch);
    match &panes[1] {
        LayoutNode::Stack(cs, _, _) => {
            assert_eq!(cs.item.flex_grow, 1);
            assert_eq!(cs.overflow, Overflow::Scroll(ScrollAxis::Vertical));
        }
        _ => panic!(),
    }
}
