// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! `len(text)` counts characters (the calculator picks its display style by the length of the
//! formatted number); `len(list)` keeps counting items.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::LayoutNode;

fn texts(src: &str) -> Vec<String> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir;
    let nxir: &'static [u8] = Box::leak(nxir.into_boxed_slice());
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view =
        View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts");
    fn walk(node: &LayoutNode, out: &mut Vec<String>) {
        match node {
            LayoutNode::Stack(_, _, c) | LayoutNode::Grid(_, _, c) => {
                c.iter().for_each(|n| walk(n, out))
            }
            LayoutNode::Text(t, _) => out.push(String::from(t.content.as_str())),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(view.scene(), &mut out);
    out
}

#[test]
fn len_counts_the_characters_of_a_text() {
    let page = |value: &str| {
        format!(
            "Store S {{ shown: Str = \"{value}\", }}\nPage P {{ Stack {{ if len($state.shown) > 8 {{ Text(\"long\") }} else {{ Text(\"short\") }} }} }}"
        )
    };
    assert_eq!(texts(&page("12.345")), ["short"]);
    assert_eq!(texts(&page("1.234.567,89")), ["long"]);
    // Characters, not bytes: "−1.234,5ä" is 9 characters (more bytes).
    assert_eq!(texts(&page("−1.234,5ä")), ["long"]);
    assert_eq!(texts(&page("−1.234,5")), ["short"]);
}
