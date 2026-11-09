// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for the list row and group — geometry on the real layout engine with the
//! baked text measure (row heights, paddings, where the separator starts, content below the
//! line), the roles of every part, and the selected / selectable / disabled states.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: ListItem, List
//! TEST_SCENARIOS: heights 48/60, text roles, inset separator, trailing parts, states,
//! selection circle/check, destructive ink, content below the line, appearances, dark theme
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use super::*;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::{Material, Rect, ShapeKind};
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::{BaseTokens, DarkTokens, RadiusToken};
use nexus_widget_icon::{Icon, LucideSymbol};

const W: i32 = 320;

fn boxes(node: &LayoutNode) -> Vec<LayoutBox> {
    LayoutEngine::new().layout(node, FxPx::new(W), &BakedTextMeasure).expect("layout").boxes
}

fn rect(b: &LayoutBox) -> (i32, i32, i32, i32) {
    let Rect { x, y, width, height } = b.rect;
    (x.0, y.0, width.0, height.0)
}

fn icon18(t: &dyn Tokens) -> LayoutNode {
    Icon::lucide(LucideSymbol::Wifi).size(18).build(t)
}

/// The row's own children (selection, leading, text column).
fn row_children(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Stack(_, _, c) => c,
        _ => panic!("a row is a Stack"),
    }
}

/// The parts of the line (text column → line → parts).
fn line_parts(node: &LayoutNode) -> &[LayoutNode] {
    let text_col = row_children(node).last().expect("text column");
    match text_col {
        LayoutNode::Stack(_, _, c) => match &c[0] {
            LayoutNode::Stack(_, _, parts) => parts,
            _ => panic!("the line is a Stack"),
        },
        _ => panic!("the text column is a Stack"),
    }
}

fn texts(node: &LayoutNode, out: &mut Vec<(String, nexus_layout_types::TextStyle)>) {
    match node {
        LayoutNode::Text(t, _) => out.push((String::from(t.content.as_str()), t.style.clone())),
        LayoutNode::Stack(_, _, c) | LayoutNode::Grid(_, _, c) => {
            c.iter().for_each(|n| texts(n, out));
        }
        _ => {}
    }
}

#[test]
fn a_title_row_is_48_and_a_two_line_row_60() {
    let t = BaseTokens;
    let one = ListItem::new("WLAN").build(&t);
    assert_eq!(rect(&boxes(&one)[0]), (0, 0, W, 48));
    let two = ListItem::new("WLAN").subtitle("Verbunden").build(&t);
    assert_eq!(rect(&boxes(&two)[0]).3, 12 + 20 + 16 + 12);
    // Compact and sidebar rows.
    let compact = ListItem::new("WLAN")
        .metrics(RowMetrics::of(ListAppearance::InsetGrouped, ListDensity::Compact))
        .build(&t);
    assert_eq!(rect(&boxes(&compact)[0]).3, 40);
    let side = ListItem::new("Alle")
        .metrics(RowMetrics::of(ListAppearance::Sidebar, ListDensity::Regular))
        .build(&t);
    assert_eq!(rect(&boxes(&side)[0]).3, 36);
}

#[test]
fn title_is_body_medium_label_and_subtitle_footnote_secondary() {
    let t = BaseTokens;
    let mut out = Vec::new();
    texts(&ListItem::new("WLAN").subtitle("Verbunden").trailing_text("Ein").build(&t), &mut out);
    let (title, sub, value) = (&out[0].1, &out[1].1, &out[2].1);
    assert_eq!((title.font_size.0, title.font_weight), (14, FontWeight::Medium));
    assert_eq!(title.color, t.color(ColorToken::Label));
    assert_eq!((sub.font_size.0, sub.font_weight), (12, FontWeight::Normal));
    assert_eq!(sub.color, t.color(ColorToken::LabelSecondary));
    assert_eq!((value.font_size.0, value.color), (14, t.color(ColorToken::LabelSecondary)));
}

#[test]
fn a_group_separates_rows_at_the_text_column() {
    let t = BaseTokens;
    let list = List::new(alloc::vec![
        ListItem::new("WLAN").leading(icon18(&t)).id("a"),
        ListItem::new("Bluetooth").leading(icon18(&t)).id("b"),
        ListItem::new("Info").id("c"),
    ])
    .build(&t);
    let all = boxes(&list);
    let seps: Vec<&LayoutBox> = all.iter().filter(|b| b.visual.border.top.is_some()).collect();
    // The group's ring is a uniform border; the separators are top-only edges.
    let tops: Vec<(i32, i32, i32)> = seps
        .iter()
        .filter(|b| b.visual.border.left.is_none())
        .map(|b| (rect(b).0, rect(b).1, rect(b).2))
        .collect();
    // Row 2 (y 48): from the text x (16 + 18 + 12 = 46) to the trailing edge. Row 3 (y 96, no
    // leading): from the 16 px padding. The first row has none.
    assert_eq!(tops, alloc::vec![(46, 48, W - 46), (16, 96, W - 16)]);
    let sep = seps.iter().find(|b| b.visual.border.left.is_none()).expect("separator");
    assert_eq!(sep.visual.border.top.map(|b| b.color), Some(t.color(ColorToken::Separator)));
    assert_eq!(sep.visual.border.top.map(|b| b.width), Some(HAIRLINE));
}

#[test]
fn trailing_parts_follow_the_text() {
    let t = BaseTokens;
    let row = ListItem::new("Mitteilungen").trailing_text("3 neu").badge("3").show_chevron(true);
    assert!(row.wants_chevron());
    let node = row.build(&t);
    let parts = line_parts(&node);
    assert_eq!(parts.len(), 4, "text column, value, badge, chevron");
    match &parts[2] {
        LayoutNode::Stack(s, v, _) => {
            assert_eq!((s.min_height, s.max_height), (Some(FxPx::new(20)), Some(FxPx::new(20))));
            assert_eq!(v.background, Some(t.color(ColorToken::Danger)));
            assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
        }
        _ => panic!("badge"),
    }
    match &parts[3] {
        LayoutNode::Stack(s, v, _) => {
            assert_eq!(s.min_width, Some(FxPx::new(16)));
            assert_eq!(v.background, Some(t.color(ColorToken::LabelTertiary)));
            assert!(matches!(v.shape, ShapeKind::Stroke { .. }));
        }
        _ => panic!("chevron"),
    }
    // A reorder handle does not make the row taller.
    let handle = ListItem::new("Ordner").reorderable(true).build(&t);
    assert_eq!(rect(&boxes(&handle)[0]).3, 48);
}

#[test]
fn selected_is_the_fill_layer_and_disabled_dims() {
    let t = BaseTokens;
    match ListItem::new("Alle").selected(true).build(&t) {
        LayoutNode::Stack(_, v, _) => assert_eq!(v.background, Some(t.color(ColorToken::Fill))),
        _ => panic!(),
    }
    match ListItem::new("Alle").build(&t) {
        LayoutNode::Stack(_, v, _) => assert_eq!((v.background, v.opacity), (None, None)),
        _ => panic!(),
    }
    match ListItem::new("Alle").disabled(true).build(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.opacity, Some(nexus_layout_types::Fraction::new(115)))
        }
        _ => panic!(),
    }
}

#[test]
fn a_selectable_row_draws_its_circle_or_check() {
    let t = BaseTokens;
    let off = ListItem::new("Foto").selectable(true).build(&t);
    match &row_children(&off)[0] {
        LayoutNode::Stack(s, v, kids) => {
            assert_eq!(s.min_width, Some(FxPx::new(22)));
            assert!(v.border.top.is_some() && v.background.is_none() && kids.is_empty());
        }
        _ => panic!("circle"),
    }
    let on = ListItem::new("Foto").selectable(true).selected(true).build(&t);
    match (&on, &row_children(&on)[0]) {
        (LayoutNode::Stack(_, row_v, _), LayoutNode::Stack(_, v, kids)) => {
            assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
            assert_eq!(kids.len(), 1, "the check");
            assert_eq!(row_v.background, None, "selection is the circle, not the fill layer");
        }
        _ => panic!(),
    }
    let check = ListItem::new("Foto")
        .selectable(true)
        .selected(true)
        .selection_style(SelectionStyle::Checkmark)
        .build(&t);
    match line_parts(&check).last() {
        Some(LayoutNode::Stack(s, v, _)) => {
            assert_eq!(s.min_width, Some(FxPx::new(16)));
            assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
        }
        _ => panic!("check"),
    }
}

#[test]
fn destructive_inks_title_and_leading_glyph() {
    let t = BaseTokens;
    let node = ListItem::new("Abmelden").leading(icon18(&t)).destructive(true).build(&t);
    let danger = t.color(ColorToken::Danger);
    match &row_children(&node)[0] {
        LayoutNode::Stack(_, v, _) => assert_eq!(v.background, Some(danger)),
        _ => panic!("leading"),
    }
    let mut out = Vec::new();
    texts(&node, &mut out);
    assert_eq!(out[0].1.color, danger);
}

#[test]
fn content_below_sits_under_the_line_at_the_text_column() {
    let t = BaseTokens;
    let slider = match parts::fixed(100, 20, VisualStyle::default(), alloc::vec![]) {
        LayoutNode::Stack(mut st, v, c) => {
            st.id = Some("slider");
            LayoutNode::Stack(st, v, c)
        }
        other => other,
    };
    let node =
        ListItem::new("Helligkeit").leading(icon18(&t)).children(alloc::vec![slider]).build(&t);
    let all = boxes(&node);
    // 12 + line 20 + gap 8 + content 20 + 12.
    assert_eq!(rect(&all[0]).3, 72);
    let below = all.iter().find(|b| b.id == Some("slider")).expect("content");
    assert_eq!((rect(below).0, rect(below).1), (46, 40));
    // The leading stays on the line: 12 + (20 − 18) / 2.
    assert_eq!(rect(&all[1]).1, 13);
}

#[test]
fn appearances_set_surface_metrics_and_corners() {
    let t = BaseTokens;
    let rows = || alloc::vec![ListItem::new("A"), ListItem::new("B")];
    match List::new(rows()).build(&t) {
        LayoutNode::Stack(_, v, kids) => {
            assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
            assert_eq!(v.material, Some(Material::Card));
            assert_eq!(v.border.left.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
            assert_eq!(v.corner_radius.top_left, FxPx::new(16));
            match (&kids[0], &kids[1]) {
                (LayoutNode::Stack(_, first, _), LayoutNode::Stack(_, last, _)) => {
                    assert_eq!(first.corner_radius.top_left, FxPx::new(16));
                    assert_eq!(first.corner_radius.bottom_left, FxPx::ZERO);
                    assert_eq!(last.corner_radius.bottom_right, FxPx::new(16));
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
    let side = List::new(rows()).appearance(ListAppearance::Sidebar).build(&t);
    match &side {
        LayoutNode::Stack(s, v, kids) => {
            assert_eq!((s.gap, v.background), (FxPx::new(2), None));
            match &kids[1] {
                LayoutNode::Stack(_, rv, _) => {
                    assert_eq!(rv.corner_radius.top_left, FxPx::new(12));
                    assert_eq!(rv.border.top, None, "no separators in a sidebar");
                }
                _ => panic!(),
            }
        }
        _ => panic!(),
    }
    let all = boxes(&side);
    assert_eq!(rect(&all[0]).3, 36 + 2 + 36);
    // Inside a pane the group re-tints to fill at 50 %.
    match List::new(rows()).on_pane(true).build(&t) {
        LayoutNode::Stack(_, v, _) => {
            let fill = t.color(ColorToken::Fill);
            assert_eq!(v.background.map(|c| (c.r, c.a)), Some((fill.r, fill.a / 2)));
        }
        _ => panic!(),
    }
}

#[test]
fn the_dark_theme_is_tokens_only() {
    let t = DarkTokens;
    let mut out = Vec::new();
    texts(&ListItem::new("WLAN").subtitle("Aus").build(&t), &mut out);
    assert_eq!(out[0].1.color, t.color(ColorToken::Label));
    assert_eq!(out[1].1.color, t.color(ColorToken::LabelSecondary));
}
