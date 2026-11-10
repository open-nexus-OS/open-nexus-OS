// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! GlassCard: the six levels' materials, radii, shadows and the clip.

use super::*;
use nexus_layout_types::{FlexItem, Spacer};
use nexus_style::box_shadow;
use nexus_theme_tokens::{BackdropToken, BaseTokens, DarkTokens};

#[test]
fn levels_map_to_materials() {
    assert_eq!(CardLevel::Panel.material(), Material::Panel);
    assert_eq!(CardLevel::Dock.material(), Material::Dock);
    assert_eq!(CardLevel::Card.material(), Material::Card);
    assert_eq!(CardLevel::Inner.material(), Material::Card);
    assert_eq!(CardLevel::Subtle.material(), Material::Card);
    assert_eq!(CardLevel::Overlay.material(), Material::Overlay);
}

#[test]
fn surfaces_are_xl_and_layers_lg() {
    let t = BaseTokens;
    for (level, radius) in [
        (CardLevel::Panel, 24),
        (CardLevel::Dock, 24),
        (CardLevel::Overlay, 24),
        (CardLevel::Card, 16),
        (CardLevel::Inner, 16),
        (CardLevel::Subtle, 16),
    ] {
        let v = GlassCard::new().level(level).style(&t).visual();
        assert_eq!(v.corner_radius.top_left, FxPx::new(radius), "{level:?}");
    }
    let nested = GlassCard::new().level(CardLevel::Inner).concentric(FxPx::new(24), FxPx::new(16));
    assert_eq!(nested.style(&t).visual().corner_radius.top_left, FxPx::new(8));
}

#[test]
fn panel_dock_and_overlay_blur_and_the_dock_is_thick() {
    let t = DarkTokens;
    let panel = GlassCard::new().level(CardLevel::Panel).style(&t);
    let dock = GlassCard::new().level(CardLevel::Dock).style(&t);
    // The floating overlay is frosted as a root (RFC-0116, 2026-11-10): the regular blur.
    let overlay = GlassCard::new().level(CardLevel::Overlay).style(&t);
    let regular = t.backdrop(BackdropToken::Regular);
    let thick = t.backdrop(BackdropToken::Thick);
    assert_eq!(panel.backdrop_blur().map(|b| b.radius), Some(u32::from(regular.blur)));
    assert_eq!(dock.backdrop_blur().map(|b| b.radius), Some(u32::from(thick.blur)));
    assert_eq!(overlay.backdrop_blur().map(|b| b.radius), Some(u32::from(regular.blur)));
    for level in [CardLevel::Card, CardLevel::Inner, CardLevel::Subtle] {
        assert!(GlassCard::new().level(level).style(&t).backdrop_blur().is_none(), "{level:?}");
    }
}

#[test]
fn each_level_carries_its_edge_and_shadow() {
    let t = BaseTokens;
    let shadow = |s: ShadowToken| Some(box_shadow(t.shadow(s)));
    let edge = Some(t.color(ColorToken::GlassEdge));
    let lit = Some(t.shadow(ShadowToken::EdgeHighlight).color);
    let v = |level| GlassCard::new().level(level).style(&t).visual();

    let panel = v(CardLevel::Panel);
    assert_eq!((panel.shadow, panel.inset_highlight), (shadow(ShadowToken::Shadow4), lit));
    assert_eq!(panel.background, Some(t.color(ColorToken::GlassPanel)));
    let dock = v(CardLevel::Dock);
    assert_eq!((dock.shadow, dock.inset_highlight), (shadow(ShadowToken::Shadow4), lit));
    let card = v(CardLevel::Card);
    assert_eq!(card.background, Some(t.color(ColorToken::GlassCard)));
    assert_eq!((card.shadow, card.inset_highlight), (shadow(ShadowToken::Shadow2), None));
    assert_eq!(card.border.top.map(|b| b.color), edge);
    let inner = v(CardLevel::Inner);
    assert_eq!((inner.shadow, inner.border.top.map(|b| b.color)), (None, edge));
    let subtle = v(CardLevel::Subtle);
    assert_eq!((subtle.shadow, subtle.border.top), (None, None));
    assert_eq!(subtle.background, Some(t.color(ColorToken::GlassCard)));
    let overlay = v(CardLevel::Overlay);
    assert_eq!(overlay.background, Some(t.color(ColorToken::GlassOverlay)));
    assert_eq!((overlay.shadow, overlay.inset_highlight), (shadow(ShadowToken::Shadow3), lit));
}

#[test]
fn material_false_keeps_the_panel_but_drops_the_blur() {
    let t = BaseTokens;
    let s = GlassCard::new().level(CardLevel::Panel).material(false).style(&t);
    assert!(s.backdrop_blur().is_none());
    let v = s.visual();
    assert_eq!(v.material, None, "no region for the compositor");
    assert_eq!(v.background, Some(t.color(ColorToken::GlassPanel)));
    assert_eq!(v.shadow, Some(box_shadow(t.shadow(ShadowToken::Shadow4))));
    // No effect on a level that never blurs.
    assert_eq!(
        GlassCard::new().level(CardLevel::Card).material(false).style(&t),
        GlassCard::new().level(CardLevel::Card).style(&t)
    );
}

#[test]
fn builds_a_clipping_panel_with_id_and_children() {
    let t = BaseTokens;
    let node = GlassCard::new()
        .id("control-center")
        .level(CardLevel::Card)
        .child(LayoutNode::Spacer(Spacer {
            id: None,
            flex_grow: 0,
            min_size: Some(FxPx::new(10)),
            item: FlexItem::default(),
        }))
        .build(&t);
    match node {
        LayoutNode::Stack(stack, visual, children) => {
            assert_eq!(stack.id, Some("control-center"));
            assert_eq!(stack.overflow, Overflow::Hidden, "every level clips");
            assert!(visual.background.is_some());
            assert_eq!(children.len(), 1);
        }
        _ => panic!("GlassCard must build a Stack (via Panel)"),
    }
}
