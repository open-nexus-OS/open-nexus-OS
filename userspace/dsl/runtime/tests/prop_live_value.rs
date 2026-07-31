// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Does a component PROP bound to a store field track that field's changes?
//!
//! Every live-value component in the repo reads `$state.x` directly
//! (`Text($state.clock)`, `Text($state.preedit)`); none threads a changing
//! value through a prop. This pins whether that is a house style or a
//! REQUIREMENT — if props go stale, an app author has no way to know.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

fn texts(v: &View<'_>) -> Vec<String> {
    fn walk(n: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        match n {
            nexus_layout_types::LayoutNode::Stack(_, _, c)
            | nexus_layout_types::LayoutNode::Grid(_, _, c) => {
                for ch in c {
                    walk(ch, out);
                }
            }
            nexus_layout_types::LayoutNode::Text(t, _) => {
                out.push(String::from(t.content.as_str()))
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v.scene(), &mut out);
    out
}

const SRC: &str = r#"Store S {
    v: Str = "zero",
}
Event E {
    Set,
}
reduce E {
    Set => state.v = "one",
}
Component Show {
    props: {
        shown: Str,
    }
    Stack { Text($props.shown) }
}
Page Main {
    Stack {
        Show { shown: $state.v }
        Text($state.v)
    }
}
"#;

#[test]
fn a_prop_bound_to_a_store_field_tracks_it() {
    let file = nexus_dsl_core::parse_file(SRC).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, SRC).expect("lowers").nxir;
    let symbols: Vec<String> =
        nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts").symbols().to_vec();
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    assert_eq!(texts(&view), vec!["zero", "zero"], "both render the initial value");

    let (e, c) = view.runtime().event_case("E", "Set").expect("Set");
    view.dispatch(&tokens, &device, &locale, &mut host, e, c, vec![]).expect("dispatch");

    let after = texts(&view);
    assert_eq!(
        after,
        vec!["one", "one"],
        "the PROP-bound text went stale while the direct binding updated: {after:?}"
    );
}
