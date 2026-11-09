// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! A card inside an opaque surface is `fill` at half its alpha (RFC-0116: the design artifacts
//! re-tint `glassCard` inside a pane or a floating surface); on glass it stays `glassCard`.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::{LayoutNode, Rgba8};
use nexus_theme_tokens::{BaseTokens, ColorToken, Tokens};

fn mount(src: &str) -> View<'static> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir;
    let nxir: &'static [u8] = Box::leak(nxir.into_boxed_slice());
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, &BaseTokens, &device, &locale).expect("mounts")
}

/// The background of the first child of the root's first child.
fn inner_bg(view: &View) -> Option<Rgba8> {
    let LayoutNode::Stack(_, _, outer) = view.scene() else { panic!("root") };
    let LayoutNode::Stack(_, _, inner) = &outer[0] else { panic!("surface") };
    match &inner[0] {
        LayoutNode::Stack(_, v, _) => v.background,
        _ => panic!("card"),
    }
}

#[test]
fn a_card_on_a_pane_takes_half_the_fill_and_on_glass_keeps_the_card_role() {
    let t = BaseTokens;
    let fill = t.color(ColorToken::Fill);
    let half = Rgba8 { a: ((u16::from(fill.a) + 1) / 2) as u8, ..fill };
    for surface in ["pane", "overlay"] {
        let view = mount(&format!(
            "Page P {{ Stack {{ Stack {{ Stack {{ }}.material(card) }}.material({surface}) }} }}"
        ));
        assert_eq!(inner_bg(&view), Some(half), "a card on a {surface}");
        let view = mount(&format!(
            "Page P {{ Stack {{ Stack {{ Stack {{ }}.bg(glassCard) }}.material({surface}) }} }}"
        ));
        assert_eq!(inner_bg(&view), Some(half), "glassCard named on a {surface}");
    }
    let view = mount("Page P { Stack { Stack { Stack { }.material(card) }.material(panel) } }");
    assert_eq!(inner_bg(&view), Some(t.color(ColorToken::GlassCard)), "a card on glass");
    // The pane's own fill is untouched.
    let view = mount("Page P { Stack { Stack { Stack { } }.material(pane) } }");
    let LayoutNode::Stack(_, _, outer) = view.scene() else { panic!("root") };
    let LayoutNode::Stack(_, pane, _) = &outer[0] else { panic!("pane") };
    assert_eq!(pane.background, Some(t.color(ColorToken::SurfacePane)));
}
