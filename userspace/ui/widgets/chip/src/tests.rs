// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for the chip — the five heights and label styles on the real layout
//! engine, the four looks and the selected glow, the remove button, the avatar inset and the
//! disabled dim.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: Chip
//! TEST_SCENARIOS: sizes, looks, selected, remove, avatar, disabled, dark theme
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

use super::*;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::Fraction;
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::{BaseTokens, DarkTokens};

fn boxes(node: &LayoutNode) -> Vec<LayoutBox> {
    let host = LayoutNode::Stack(
        Stack { direction: Direction::Row, ..Stack::default() },
        VisualStyle::default(),
        alloc::vec![node.clone()],
    );
    LayoutEngine::new().layout(&host, FxPx::new(600), &BakedTextMeasure).expect("layout").boxes
}

fn label(node: &LayoutNode) -> &nexus_layout_types::TextNode {
    match node {
        LayoutNode::Stack(_, _, c) => c
            .iter()
            .find_map(|n| match n {
                LayoutNode::Text(t, _) => Some(t),
                _ => None,
            })
            .expect("label"),
        _ => panic!(),
    }
}

#[test]
fn heights_and_label_styles_follow_the_size() {
    let t = BaseTokens;
    for (size, h, px) in [
        (ControlSize::Xs, 24, 10),
        (ControlSize::Sm, 24, 11),
        (ControlSize::Md, 30, 13),
        (ControlSize::Lg, 36, 14),
        (ControlSize::Xl, 44, 15),
    ] {
        let node = Chip::new("Design").size(size).id("c").build(&t);
        let all = boxes(&node);
        let b = all.iter().find(|b| b.id == Some("c")).expect("chip");
        assert_eq!(b.rect.height, FxPx::new(h), "{size:?}");
        let l = label(&node);
        assert_eq!(l.style.font_size, FxPx::new(px), "{size:?}");
        assert_eq!(l.style.font_weight, FontWeight::Medium);
    }
    // md: 1 px edge + 12 px padding on each side of the label.
    let node = Chip::new("Design").id("c").build(&t);
    let all = boxes(&node);
    let chip = all.iter().find(|b| b.id == Some("c")).expect("chip");
    let text = all.iter().find(|b| b.rect.height == FxPx::new(18)).expect("label");
    assert_eq!(text.rect.x - chip.rect.x, FxPx::new(13));
}

#[test]
fn the_looks() {
    let t = BaseTokens;
    match Chip::new("Ungelesen").build(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::Fill)));
            assert_eq!(v.border.top, None, "no visible edge");
            assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px());
        }
        _ => panic!(),
    }
    let tinted = Chip::new("Neu").appearance(ChipAppearance::Tinted).tint(ColorToken::Success);
    assert_eq!(tinted.foreground(), ColorToken::Success);
    match tinted.build(&t) {
        LayoutNode::Stack(_, v, _) => {
            let s = t.color(ColorToken::Success);
            assert_eq!(v.background, Some(wash(s, 16)));
        }
        _ => panic!(),
    }
    match Chip::new("Alle").appearance(ChipAppearance::Outline).build(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, None);
            assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::Separator)));
        }
        _ => panic!(),
    }
    let filled = Chip::new("Arbeit").appearance(ChipAppearance::Filled).tint(ColorToken::Danger);
    assert_eq!(filled.foreground(), ColorToken::OnAccent);
    match filled.build(&t) {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::Danger)));
            assert_eq!(v.shadow, None, "only a selected chip glows");
        }
        _ => panic!(),
    }
}

#[test]
fn selected_is_the_solid_tint_with_its_glow() {
    let t = BaseTokens;
    let chip = Chip::new("Design").selected(true);
    assert_eq!(chip.foreground(), ColorToken::OnAccent);
    let node = chip.build(&t);
    match &node {
        LayoutNode::Stack(_, v, _) => {
            assert_eq!(v.background, Some(t.color(ColorToken::Accent)));
            let glow = v.shadow.expect("glow");
            assert_eq!((glow.offset_y, glow.blur_radius), (FxPx::new(2), FxPx::new(8)));
            assert_eq!(glow.color, t.color(ColorToken::AccentGlow));
        }
        _ => panic!(),
    }
    assert_eq!(label(&node).style.color, t.color(ColorToken::OnAccent));
}

#[test]
fn the_remove_button_is_built_in() {
    let t = BaseTokens;
    let node = Chip::new("anna@firma.de").removable("rm").id("c").build(&t);
    match &node {
        LayoutNode::Stack(s, _, kids) => {
            assert_eq!(s.padding.right, FxPx::new(5), "4 px + the edge before the button");
            match kids.last() {
                Some(LayoutNode::Stack(bs, bv, glyph)) => {
                    assert_eq!(bs.id, Some("rm"));
                    assert_eq!(bs.min_width, Some(FxPx::new(18)));
                    assert_eq!(bv.background, Some(t.color(ColorToken::Fill)));
                    match &glyph[0] {
                        LayoutNode::Stack(gs, gv, _) => {
                            assert_eq!(gs.min_width, Some(FxPx::new(9)));
                            assert_eq!(gv.background, Some(t.color(ColorToken::Label)));
                        }
                        _ => panic!(),
                    }
                }
                _ => panic!("remove button"),
            }
        }
        _ => panic!(),
    }
    // On a solid chip the circle is the on-ink at 30 %.
    match Chip::new("x").selected(true).removable("rm").build(&t) {
        LayoutNode::Stack(_, _, kids) => match kids.last() {
            Some(LayoutNode::Stack(_, bv, _)) => {
                assert_eq!(bv.background, Some(wash(t.color(ColorToken::OnAccent), 30)))
            }
            _ => panic!(),
        },
        _ => panic!(),
    }
}

#[test]
fn an_avatar_sits_2_px_from_the_edge_and_disabled_dims() {
    let t = BaseTokens;
    let chip = Chip::new("Anna");
    assert_eq!(chip.avatar_size(), 24);
    let avatar = LayoutNode::Stack(Stack::default(), VisualStyle::default(), alloc::vec![]);
    match chip.avatar(avatar).build(&t) {
        LayoutNode::Stack(s, _, _) => assert_eq!(s.padding.left, FxPx::new(3)),
        _ => panic!(),
    }
    match Chip::new("x").state(InteractionState::Disabled).build(&t) {
        LayoutNode::Stack(_, v, _) => assert_eq!(v.opacity, Some(Fraction::new(115))),
        _ => panic!(),
    }
    let d = DarkTokens;
    match Chip::new("x").build(&d) {
        LayoutNode::Stack(_, v, _) => assert_eq!(v.background, Some(d.color(ColorToken::Fill))),
        _ => panic!(),
    }
}
