// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// What `.textStyle(<style>)` ACTUALLY renders at (RFC-0116).
//
// A text node is one of the thirteen styles: size, line box, weight and tracking together, and
// `.fontWeight` the one override. The atlas bakes a face for every style and for every override
// the screens use, so a style is a PROMISE, not a request: `FontSize::exact` must find its face.
// Before the token set, a `.textSize` px was a request `FontSize::nearest` rounded onto a
// handful of rungs (a 12 px label drew at 13, a 14 px one at 13, a medium weight as regular).

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::{FontWeight, LayoutNode, LineHeight, TextNode};
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_text_baked::FontSize;
use nexus_theme_tokens::TextStyleToken;

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
    View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts")
}

fn texts(node: &LayoutNode, out: &mut Vec<TextNode>) {
    match node {
        LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
            children.iter().for_each(|c| texts(c, out))
        }
        LayoutNode::Text(t, _) => out.push(t.clone()),
        _ => {}
    }
}

/// Every style, through the DSL, carries its own size, line box, weight and tracking — and the
/// atlas has the exact face for it.
#[test]
fn every_text_style_renders_on_its_exact_face() {
    let body: String = TextStyleToken::ALL
        .iter()
        .map(|s| format!("Text(\"Ag\").textStyle({})\n", s.name()))
        .collect();
    let view = mount(&format!("Page P {{ Stack {{ {body} }} }}"));
    let mut nodes = Vec::new();
    texts(view.scene(), &mut nodes);
    assert_eq!(nodes.len(), TextStyleToken::ALL.len());
    for (style, node) in TextStyleToken::ALL.iter().zip(&nodes) {
        let spec = style.spec();
        assert_eq!(node.style.font_size.0, i32::from(spec.size), "{}", style.name());
        assert_eq!(
            node.style.line_height,
            LineHeight::Absolute(nexus_layout_types::FxPx::new(i32::from(spec.line)))
        );
        assert_eq!(node.style.font_weight, spec.weight, "{}", style.name());
        assert_eq!(node.style.tracking_milli_em, spec.tracking_milli_em, "{}", style.name());
        let weight = BakedTextMeasure::weight(spec.weight);
        assert!(
            FontSize::exact(i32::from(spec.size), weight).is_some(),
            "`{}` ({} px, {:?}) has no baked face",
            style.name(),
            spec.size,
            spec.weight
        );
    }
}

/// `.fontWeight` is the one override, and the overline sets its text in upper case; a text with
/// no style is `body`.
#[test]
fn weight_override_upper_case_and_the_default_style() {
    let view = mount(
        "Page P { Stack { Text(\"Zeit\").textStyle(footnote).fontWeight(semibold)\n\
         Text(\"Allgemein\").textStyle(overline)\n Text(\"plain\") } }",
    );
    let mut nodes = Vec::new();
    texts(view.scene(), &mut nodes);
    assert_eq!(nodes[0].style.font_weight, FontWeight::Semibold);
    assert_eq!(nodes[0].style.font_size.0, 12);
    assert_eq!(nodes[1].content.as_str(), "ALLGEMEIN");
    let body = TextStyleToken::Body.spec();
    assert_eq!(nodes[2].style.font_size.0, i32::from(body.size));
    assert_eq!(nodes[2].style.font_weight, body.weight);
}

/// `.truncate(n)` keeps a text to `n` lines (the painter cuts the last with an ellipsis); without
/// it a text wraps freely.
#[test]
fn truncate_limits_the_lines() {
    let view = mount("Page P { Stack { Text(\"one\").truncate(1)\n Text(\"two\").truncate(2)\n Text(\"free\") } }");
    let mut nodes = Vec::new();
    texts(view.scene(), &mut nodes);
    let limits: Vec<Option<u32>> = nodes.iter().map(|n| n.max_lines).collect();
    assert_eq!(limits, [Some(1), Some(2), None]);
}
