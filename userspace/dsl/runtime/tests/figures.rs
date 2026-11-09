// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Tabular figures (RFC-0116 amendment, M-3): `.figures(tabular)` gives a text's digits one
//! width, the `clock` style is tabular by its theme definition, `.figures(proportional)` undoes
//! it, and a name outside the closed vocabulary does not compile.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::LayoutNode;
use nexus_theme_tokens::BaseTokens;

fn compile(src: &str) -> Result<Vec<u8>, String> {
    let file = nexus_dsl_core::parse_file(src).map_err(|e| format!("{e:?}"))?;
    let (model, diags) = nexus_dsl_core::check_file(&file);
    if nexus_dsl_core::has_errors(&diags) {
        return Err(format!("{diags:?}"));
    }
    Ok(nexus_dsl_core::lower_file(&file, &model, src).map_err(|e| format!("{e:?}"))?.nxir)
}

fn tabular_flags(src: &str) -> Vec<bool> {
    let nxir: &'static [u8] = Box::leak(compile(src).expect("compiles").into_boxed_slice());
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(nxir, &BaseTokens, &device, &locale).expect("mounts");
    let LayoutNode::Stack(_, _, children) = view.scene() else { panic!("root") };
    children
        .iter()
        .map(|c| match c {
            LayoutNode::Text(t, _) => t.style.tabular,
            other => panic!("expected a text, got {other:?}"),
        })
        .collect()
}

#[test]
fn figures_reach_the_text_style_and_clock_is_tabular() {
    let flags = tabular_flags(
        "Page P { Stack { Text(\"12\") Text(\"12\").figures(tabular) Text(\"12\").textStyle(clock) \
         Text(\"12\").textStyle(clock).figures(proportional) } }",
    );
    assert_eq!(flags, vec![false, true, true, false]);
}

#[test]
fn test_reject_an_unknown_figures_name() {
    let err = compile("Page P { Stack { Text(\"1\").figures(oldstyle) } }")
        .expect_err("a name outside the vocabulary must not compile");
    assert!(err.contains("figures") || err.contains("oldstyle"), "names the problem: {err}");
}
