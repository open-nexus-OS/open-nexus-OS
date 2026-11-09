// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `TextArea` (split from lib.rs): per-size box geometry, surfaces,
//! states, the placeholder role, the footer and the focus outline.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_layout_types::{EdgeBorder, Fraction, Position};
use nexus_theme_tokens::BaseTokens;

fn column(node: LayoutNode) -> (VisualStyle, Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(_, v, col) => (v, col),
        _ => panic!("TextArea must build a column"),
    }
}

fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(s, v, k) => (s, v, k),
        _ => panic!("expected a stack"),
    }
}

#[test]
fn the_box_fits_rows_of_the_size_line_plus_padding_and_border() {
    // md: 4 rows × 20 + 2 × 8 + 2 = 98 (the design's table).
    assert_eq!(TextArea::new().min_box_height(), 98);
    for (size, h) in [
        (ControlSize::Xs, 74),
        (ControlSize::Sm, 82),
        (ControlSize::Lg, 106),
        (ControlSize::Xl, 114),
    ] {
        assert_eq!(TextArea::new().size(size).min_box_height(), h, "{size:?}");
    }
    let (_, col) = column(TextArea::new().rows(5).build(&BaseTokens));
    let (s, _, _) = stack(&col[0]);
    assert_eq!(s.min_height, Some(FxPx::new(5 * 20 + 16 + 2)));
    assert_eq!(s.padding, EdgeInsets::symmetric(FxPx::new(9), FxPx::new(13)));
}

#[test]
fn rests_on_the_fill_with_the_size_radius_or_the_capsule() {
    let t = BaseTokens;
    let (_, col) = column(TextArea::new().build(&t));
    let (_, v, _) = stack(&col[0]);
    assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(v.border, EdgeBorder::none());
    assert_eq!(v.corner_radius.top_left, FxPx::new(12));
    assert_eq!(TextArea::new().capsule(true).radius(), FxPx::new((20 + 16 + 2) / 2));
}

#[test]
fn the_body_is_field_text_and_the_placeholder_label_tertiary() {
    let t = BaseTokens;
    let (_, col) = column(TextArea::new().placeholder("Notiz …").build(&t));
    let (_, _, kids) = stack(&col[0]);
    match &kids[0] {
        LayoutNode::Text(body, _) => {
            assert_eq!(body.style.color, t.color(ColorToken::LabelTertiary));
            assert_eq!(body.style.font_weight, FontWeight::Medium);
            assert_eq!(body.max_lines, Some(4), "default rows 4");
        }
        _ => panic!("body"),
    }
    let (_, col) = column(TextArea::new().value("x").auto_grow(true).build(&t));
    match &stack(&col[0]).2[0] {
        LayoutNode::Text(body, _) => {
            assert_eq!(body.style.color, t.color(ColorToken::Label));
            assert_eq!(body.max_lines, None, "auto-grow never clips");
        }
        _ => panic!("body"),
    }
}

#[test]
fn error_adds_the_footer_in_danger_and_the_label_on_top() {
    let t = BaseTokens;
    let ta = TextArea::new().label("Notiz").error("Pflichtfeld");
    assert_eq!(ta.field_state(), FieldState::Error);
    let (_, col) = column(ta.build(&t));
    assert_eq!(col.len(), 3, "label + box + footer");
    assert_eq!(stack(&col[1]).1.border.top.map(|b| b.color), Some(t.color(ColorToken::Danger)));
    let (footer, _, kids) = stack(&col[2]);
    assert_eq!(footer.item.margin.left, FxPx::new(2));
    assert!(
        matches!(&kids[0], LayoutNode::Text(m, _) if m.style.color == t.color(ColorToken::Danger))
    );
}

#[test]
fn the_counter_needs_show_count_and_max_length() {
    let t = BaseTokens;
    assert_eq!(column(TextArea::new().value("hi").max_length(280).build(&t)).1.len(), 1);
    let (_, col) = column(TextArea::new().value("hi").max_length(280).show_count(true).build(&t));
    assert_eq!(col.len(), 2, "box + footer");
    let (footer, _, kids) = stack(&col[1]);
    assert_eq!(footer.justify, Justify::End, "a lone counter sits at the end");
    assert!(matches!(&kids[0], LayoutNode::Text(c, _) if c.content.as_str() == "2/280"));
}

#[test]
fn focus_outline_success_state_and_disabled_dim() {
    let t = BaseTokens;
    let (_, col) = column(TextArea::new().state(InteractionState::Focused).build(&t));
    let (_, _, kids) = stack(&col[0]);
    assert_eq!(kids.last().map(|k| k.item().position), Some(Position::Absolute));
    let (_, col) = column(TextArea::new().validation(FieldState::Success).build(&t));
    assert_eq!(stack(&col[0]).1.border.top.map(|b| b.color), Some(t.color(ColorToken::Success)));
    let (v, _) = column(TextArea::new().state(InteractionState::Disabled).build(&t));
    assert_eq!(v.opacity, Some(Fraction::new(128)));
}
