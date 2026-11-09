// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `GlassTextField` (split from glass_text_field.rs): control heights,
//! surfaces, validation states, the focus outline, the parts in the box and the disabled dim.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_layout_types::{EdgeBorder, Fraction, Material, Position, ShapeKind, TextInputNode};
use nexus_theme_tokens::{BaseTokens, DarkTokens};

fn column(node: LayoutNode) -> (VisualStyle, Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(_, v, col) => (v, col),
        _ => panic!("GlassTextField must build a column"),
    }
}

/// The field box (the column child at `index`).
fn field_box(col: &[LayoutNode], index: usize) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match &col[index] {
        LayoutNode::Stack(s, v, kids) => (s, v, kids),
        _ => panic!("expected the field box"),
    }
}

fn input(kids: &[LayoutNode]) -> &TextInputNode {
    kids.iter()
        .find_map(|k| match k {
            LayoutNode::TextInput(i, _) => Some(i),
            _ => None,
        })
        .expect("the box holds the input")
}

#[test]
fn every_size_sits_on_its_control_height_with_its_field_metrics() {
    let t = BaseTokens;
    for (size, h, radius, text_px) in [
        (ControlSize::Xs, 24, 8, 12),
        (ControlSize::Sm, 30, 8, 13),
        (ControlSize::Md, 36, 12, 14),
        (ControlSize::Lg, 44, 12, 15),
        (ControlSize::Xl, 52, 16, 17),
    ] {
        let (_, col) = column(GlassTextField::new().size(size).build(&t));
        let (s, v, kids) = field_box(&col, 0);
        assert_eq!((s.min_height, s.max_height), (Some(FxPx::new(h)), Some(FxPx::new(h))));
        assert_eq!(s.padding.top, FxPx::ZERO, "no vertical padding: the height is the control's");
        assert_eq!(v.corner_radius.top_left, FxPx::new(radius), "value field radius by size");
        let i = input(kids);
        assert_eq!(i.style.font_size, FxPx::new(text_px), "field text one step above the label");
        assert_eq!(i.style.font_weight, FontWeight::Medium);
        assert_eq!(i.style.color, t.color(ColorToken::Label));
    }
}

#[test]
fn the_md_box_pads_12_plus_the_border_and_gaps_8() {
    let (_, col) = column(GlassTextField::new().build(&BaseTokens));
    let (s, _, _) = field_box(&col, 0);
    assert_eq!(s.padding.left, FxPx::new(13));
    assert_eq!(s.gap, FxPx::new(8));
    assert_eq!(s.min_width, Some(FxPx::new(MIN_FIELD_WIDTH)), "the 180 px floor stays");
}

#[test]
fn a_filled_field_rests_on_the_fill_with_no_visible_edge() {
    let t = BaseTokens;
    let f = GlassTextField::new();
    assert_eq!(f.border_color(&t), None, "1 px transparent at rest");
    let (_, col) = column(f.build(&t));
    let (_, v, _) = field_box(&col, 0);
    assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(v.border, EdgeBorder::none());
}

#[test]
fn error_turns_the_border_danger_and_adds_the_message() {
    let t = BaseTokens;
    let f = GlassTextField::new().label("E-Mail").error("Zu kurz");
    assert_eq!(f.field_state(), FieldState::Error);
    assert_eq!(f.border_color(&t), Some(t.color(ColorToken::Danger)));
    let (_, col) = column(f.build(&t));
    assert_eq!(col.len(), 3, "label + box + error");
    match (&col[0], &col[2]) {
        (LayoutNode::Text(label, _), LayoutNode::Text(msg, _)) => {
            assert_eq!(label.style.font_weight, FontWeight::Semibold);
            assert_eq!(label.style.color, t.color(ColorToken::LabelSecondary));
            assert_eq!(label.style.font_size, FxPx::new(12), "md label = footnote");
            assert_eq!(label.item.margin.left, FxPx::new(2));
            assert_eq!(msg.style.color, t.color(ColorToken::Danger));
            assert_eq!(msg.style.font_size, FxPx::new(12), "message = footnote");
        }
        _ => panic!("label and message are text lines"),
    }
}

#[test]
fn success_draws_the_green_border_the_check_and_a_green_helper() {
    let t = BaseTokens;
    let f = GlassTextField::new().validation(FieldState::Success).helper("Verfügbar");
    assert_eq!(f.border_color(&t), Some(t.color(ColorToken::Success)));
    let (_, col) = column(f.build(&t));
    let (_, _, kids) = field_box(&col, 0);
    let check = kids.last().expect("the check trails the input");
    match check {
        LayoutNode::Stack(_, v, _) => {
            assert!(matches!(v.shape, ShapeKind::Stroke { .. }));
            assert_eq!(v.background, Some(t.color(ColorToken::Success)));
        }
        _ => panic!("check glyph"),
    }
    match &col[1] {
        LayoutNode::Text(msg, _) => assert_eq!(msg.style.color, t.color(ColorToken::Success)),
        _ => panic!("helper line"),
    }
}

#[test]
fn an_error_state_without_a_message_is_the_border_only() {
    let t = BaseTokens;
    let f = GlassTextField::new().validation(FieldState::Error);
    assert_eq!(f.border_color(&t), Some(t.color(ColorToken::Danger)));
    assert_eq!(column(f.build(&t)).1.len(), 1, "no message line");
}

#[test]
fn the_glass_field_is_the_card_layer_never_a_blur() {
    let t = DarkTokens;
    let (_, col) = column(GlassTextField::new().variant(FieldVariant::Glass).build(&t));
    let (s, v, _) = field_box(&col, 0);
    assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
    assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
    assert!(v.inset_highlight.is_some() && v.shadow.is_some(), "edge highlight + shadow1");
    assert_ne!(v.material, Some(Material::Panel), "a field never blurs");
    assert_eq!(s.max_height, Some(FxPx::new(36)), "the glass field keeps its size's height");
}

#[test]
fn the_pill_is_the_glass_capsule_on_its_size() {
    let t = BaseTokens;
    let (_, col) = column(GlassTextField::new().size(ControlSize::Lg).pill().build(&t));
    let (s, v, _) = field_box(&col, 0);
    assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
    assert_eq!(s.max_height, Some(FxPx::new(44)), "lg = the lock screen's 44");
    assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
}

#[test]
fn read_only_drops_the_fill_and_the_clear_disc() {
    let t = BaseTokens;
    let f = GlassTextField::new().read_only(true).clearable(true).value("x");
    assert_eq!(f.border_color(&t), Some(t.color(ColorToken::Separator)));
    let (_, col) = column(f.build(&t));
    let (_, v, kids) = field_box(&col, 0);
    assert_eq!(v.background, None);
    assert_eq!(kids.len(), 1, "just the input");
}

#[test]
fn bare_draws_no_box_but_keeps_the_control_height() {
    // Composing a field INTO a surrounding pill: a second box inside the first would double
    // the border; the run still lines up on the control height.
    let t = BaseTokens;
    let (_, col) = column(GlassTextField::new().variant(FieldVariant::Bare).build(&t));
    let (s, v, _) = field_box(&col, 0);
    assert_eq!(v.background, None);
    assert_eq!(v.border, EdgeBorder::none());
    assert_eq!(v.material, None);
    assert_eq!(s.max_height, Some(FxPx::new(36)));
    assert_eq!(s.padding.left, FxPx::new(12), "no border to reserve");
}

#[test]
fn focus_draws_the_outline_outside_the_box_not_a_border() {
    let t = BaseTokens;
    let f = GlassTextField::new().state(InteractionState::Focused);
    assert_eq!(f.ring_color(&t), t.color(ColorToken::Accent));
    assert_eq!(f.border_color(&t), None, "focus never recolours the border");
    let (_, col) = column(f.build(&t));
    let (_, _, kids) = field_box(&col, 0);
    let ring = kids.last().expect("the ring is the last child");
    assert_eq!(ring.item().position, Position::Absolute, "no layout impact");
    // Invalid: the ring takes the state colour.
    let err = GlassTextField::new().state(InteractionState::Focused).error("x");
    assert_eq!(err.ring_color(&t), t.color(ColorToken::Danger));
    // A tint recolours it.
    let tinted = GlassTextField::new().tint(ColorToken::Success);
    assert_eq!(tinted.ring_color(&t), t.color(ColorToken::Success));
}

#[test]
fn disabled_dims_the_whole_field_to_half() {
    let (v, _) = column(GlassTextField::new().state(InteractionState::Disabled).build(&BaseTokens));
    assert_eq!(v.opacity, Some(Fraction::new(128)));
}

#[test]
fn affixes_clear_disc_and_eye_line_up_in_the_design_order() {
    let t = BaseTokens;
    let f = GlassTextField::new()
        .prefix("€")
        .suffix("EUR")
        .value("12")
        .clearable(true)
        .clear_id("clear")
        .secure(true)
        .revealable(true)
        .reveal_id("eye");
    let (_, col) = column(f.build(&t));
    let (_, _, kids) = field_box(&col, 0);
    // prefix · input · suffix · clear · eye
    assert_eq!(kids.len(), 5);
    assert!(matches!(&kids[0], LayoutNode::Text(p, _) if p.content.as_str() == "€"));
    assert!(
        matches!(&kids[1], LayoutNode::TextInput(i, _) if i.secure && i.content.as_str() == "••")
    );
    match &kids[3] {
        LayoutNode::Stack(s, v, _) => {
            assert_eq!(s.id, Some("clear"));
            assert_eq!(s.min_width, Some(FxPx::new(18)), "round(16 × 1.1)");
            assert_eq!(v.background, Some(t.color(ColorToken::LabelSecondary)));
        }
        _ => panic!("clear disc"),
    }
    match &kids[4] {
        LayoutNode::Stack(s, _, _) => {
            assert_eq!(s.id, Some("eye"));
            assert_eq!(s.min_width, Some(FxPx::new(24)), "icon + 8");
            assert_eq!(s.item.margin.left, FxPx::new(-4));
        }
        _ => panic!("eye button"),
    }
}

#[test]
fn a_revealed_password_shows_its_text_and_stays_secure() {
    let (_, col) = column(
        GlassTextField::new().secure(true).revealed(true).value("geheim").build(&BaseTokens),
    );
    let (_, _, kids) = field_box(&col, 0);
    let i = input(kids);
    assert!(i.secure, "the IME still treats it as a password");
    assert_eq!(i.content.as_str(), "geheim");
}

#[test]
fn an_empty_field_shows_no_clear_disc() {
    let (_, col) = column(GlassTextField::new().clearable(true).build(&BaseTokens));
    assert_eq!(field_box(&col, 0).2.len(), 1);
}
