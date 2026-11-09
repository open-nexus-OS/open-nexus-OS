// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests of `SearchBar` (split from lib.rs): the capsule on the control height,
//! the widget's own magnifier and clear disc, tokens, the cancel button, scopes and states.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use super::*;
use nexus_layout_types::{EdgeBorder, Fraction, ShapeKind, Spacer};
use nexus_theme_tokens::{BaseTokens, DarkTokens};

fn icon() -> LayoutNode {
    LayoutNode::Spacer(Spacer {
        id: None,
        flex_grow: 0,
        min_size: Some(FxPx::new(16)),
        item: FlexItem::default(),
    })
}

fn stack(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
    match node {
        LayoutNode::Stack(s, v, k) => (s, v, k),
        _ => panic!("expected a stack"),
    }
}

#[test]
fn the_field_is_a_capsule_on_the_control_height_with_its_own_magnifier() {
    let t = BaseTokens;
    let node = SearchBar::new().id("q").placeholder("Apps suchen").build(&t);
    let (s, v, kids) = stack(&node);
    assert_eq!(s.id, Some("q"));
    assert_eq!((s.min_height, s.max_height), (Some(FxPx::new(36)), Some(FxPx::new(36))));
    assert_eq!(s.padding.left, FxPx::new(13), "pad 12 + the 1 px border");
    assert_eq!(s.gap, FxPx::new(8));
    assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
    assert_eq!(v.border, EdgeBorder::none(), "1 px transparent at rest");
    assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
    assert_eq!(kids.len(), 2, "magnifier + input (no clear while empty)");
    let (m, mv, _) = stack(&kids[0]);
    assert_eq!(m.min_width, Some(FxPx::new(16)));
    assert!(matches!(mv.shape, ShapeKind::Stroke { .. }));
    assert_eq!(mv.background, Some(t.color(ColorToken::LabelSecondary)));
    match &kids[1] {
        LayoutNode::TextInput(i, _) => {
            assert_eq!(i.style.font_weight, FontWeight::Medium);
            assert_eq!(i.style.font_size, FxPx::new(14));
            assert_eq!(i.min_width, Some(FxPx::new(48)));
        }
        _ => panic!("input"),
    }
}

#[test]
fn sizes_set_height_text_and_icon() {
    let t = BaseTokens;
    for (size, h, icon, text) in [
        (ControlSize::Sm, 30, 14, 13),
        (ControlSize::Lg, 44, 18, 15),
        (ControlSize::Xl, 52, 20, 17),
    ] {
        let node = SearchBar::new().size(size).build(&t);
        let (s, _, kids) = stack(&node);
        assert_eq!(s.max_height, Some(FxPx::new(h)));
        assert_eq!(stack(&kids[0]).0.min_width, Some(FxPx::new(icon)));
        assert!(
            matches!(&kids[1], LayoutNode::TextInput(i, _) if i.style.font_size == FxPx::new(text))
        );
    }
}

#[test]
fn typing_adds_the_clear_disc_and_a_caller_trailing_replaces_it() {
    let t = BaseTokens;
    let node = SearchBar::new().value("x").clear_id("clr").build(&t);
    let (_, _, kids) = stack(&node);
    assert_eq!(kids.len(), 3);
    let (disc, dv, _) = stack(&kids[2]);
    assert_eq!(disc.id, Some("clr"));
    assert_eq!(disc.min_width, Some(FxPx::new(18)));
    assert_eq!(dv.background, Some(t.color(ColorToken::LabelSecondary)));
    let node = SearchBar::new().value("x").leading(icon()).trailing(icon()).build(&t);
    assert_eq!(stack(&node).2.len(), 3, "leading + input + trailing");
}

#[test]
fn focus_draws_the_tint_outline_outside_the_capsule() {
    let t = BaseTokens;
    let node = SearchBar::new().state(InteractionState::Focused).build(&t);
    let (_, v, kids) = stack(&node);
    assert_eq!(v.border, EdgeBorder::none(), "focus never recolours the border");
    let ring = kids.last().expect("ring");
    assert_eq!(ring.item().position, Position::Absolute);
    let (_, _, inner) = stack(ring);
    assert_eq!(stack(&inner[0]).1.border.top.map(|b| b.color), Some(t.color(ColorToken::Accent)));
}

#[test]
fn glass_appearance_and_disabled_dim() {
    let t = DarkTokens;
    let node = SearchBar::new()
        .appearance(FieldAppearance::Glass)
        .state(InteractionState::Disabled)
        .value("x")
        .build(&t);
    let (_, v, kids) = stack(&node);
    assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
    assert_eq!(v.opacity, Some(Fraction::new(128)));
    assert_eq!(kids.len(), 2, "no clear disc while disabled");
}

#[test]
fn tokens_are_tint_washes_and_hide_the_placeholder() {
    let t = BaseTokens;
    let node = SearchBar::new()
        .placeholder("Suchen")
        .tokens(alloc::vec![SearchToken::new("Bilder"), SearchToken::new("PDF").id("tok")])
        .token_selected(true)
        .build(&t);
    let (_, _, kids) = stack(&node);
    assert_eq!(kids.len(), 4, "magnifier + 2 tokens + input");
    let (tok, tv, label) = stack(&kids[1]);
    assert_eq!(tok.max_height, Some(FxPx::new(24)), "h − 12");
    assert_eq!(tv.background, Some(field::wash(t.color(ColorToken::Accent), 16)));
    assert!(
        matches!(&label[0], LayoutNode::Text(l, _) if l.style.color == t.color(ColorToken::Accent))
    );
    let (sel, sv, _) = stack(&kids[2]);
    assert_eq!(sel.id, Some("tok"));
    assert_eq!(sv.background, Some(t.color(ColorToken::Accent)), "selected = tint fill");
    assert!(matches!(&kids[3], LayoutNode::TextInput(i, _) if i.placeholder.is_none()));
}

#[test]
fn the_cancel_button_joins_only_while_active() {
    let t = BaseTokens;
    // Idle: just the field.
    let idle = SearchBar::new().cancel_button(DEFAULT_CANCEL).build(&t);
    assert_eq!(stack(&idle).0.max_height, Some(FxPx::new(36)), "the field itself");
    // Active: row [field (grows), cancel] with a 12 px gap.
    let node =
        SearchBar::new().value("a").cancel_button(DEFAULT_CANCEL).cancel_id("cancel").build(&t);
    let (row, _, kids) = stack(&node);
    assert_eq!(row.gap, FxPx::new(12));
    assert_eq!(stack(&kids[0]).0.item.flex_grow, 1);
    let (cancel, _, label) = stack(&kids[1]);
    assert_eq!(cancel.id, Some("cancel"));
    assert_eq!(cancel.max_height, Some(FxPx::new(36)));
    match &label[0] {
        LayoutNode::Text(l, _) => {
            assert_eq!(l.content.as_str(), "Abbrechen");
            assert_eq!(l.style.color, t.color(ColorToken::Accent));
            assert_eq!(l.style.font_weight, FontWeight::Medium);
        }
        _ => panic!("cancel label"),
    }
}

#[test]
fn scopes_sit_under_the_field_aligned_start() {
    let t = BaseTokens;
    let node = SearchBar::new().scopes(icon_stack()).size(ControlSize::Lg).build(&t);
    let (col, _, kids) = stack(&node);
    assert_eq!(col.direction, Direction::Column);
    assert_eq!(col.gap, FxPx::new(8));
    assert_eq!(kids[1].item().align_self, Some(Align::Start));
    assert_eq!(SearchBar::new().size(ControlSize::Lg).scope_size(), ControlSize::Md);
    assert_eq!(SearchBar::new().scope_size(), ControlSize::Sm);
}

fn icon_stack() -> LayoutNode {
    LayoutNode::Stack(Stack::default(), VisualStyle::default(), Vec::new())
}

#[test]
fn loading_swaps_the_magnifier_for_the_field_spinner() {
    let t = BaseTokens;
    let node = SearchBar::new().loading(true).build(&t);
    let (_, _, kids) = stack(&node);
    let (sp, _, spokes) = stack(&kids[0]);
    assert_eq!(sp.min_width, Some(FxPx::new(16)));
    assert_eq!(spokes.len(), 12);
    let alpha = |k: &LayoutNode| stack(k).1.background.map(|c| c.a);
    assert!(alpha(&spokes[0]) < alpha(&spokes[11]), "the fade rises to the last spoke");
}
