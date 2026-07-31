// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The calculator driven under its REAL German locale pack — the path the
// device takes (`apphost: locale de-DE applied`), which the other tests miss
// because they run against the baked English default.

mod common;

use nexus_dsl_runtime::i18n::{parse_payload_container, Catalog};
use nexus_dsl_runtime::{CatalogOverBaked, FixtureEnv, Value, View};

fn alloc_digit(d: i64) -> String {
    d.to_string()
}

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

#[test]
fn typing_under_the_german_pack_shows_the_digits() {
    // The BUNDLE payload (NXIR + locale packs), exactly what execd ships.
    let payload = nexus_dsl_core::compile_project_bundle(&common::app_root("calculator"))
        .expect("calculator bundles");
    let (nxir, packs) = parse_payload_container(&payload).expect("payload is a container");
    let de = packs
        .iter()
        .find(|p| p.tag.starts_with("de"))
        .map(|p| Catalog::from_indexed_pack(p.pack).expect("de pack parses"))
        .expect("a de pack is shipped");

    let symbols = common::program_symbols(nxir);
    let keys = common::program_i18n_keys(nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let locale = CatalogOverBaked { catalog: Some(&de), symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");

    let texts = |v: &View<'_>| {
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
    };

    let (e, c) = view.runtime().event_case("CalcEvent", "Digit").expect("Digit");
    for d in [1i64, 2, 3] {
        view.dispatch(
            &tokens,
            &device,
            &locale,
            &mut host,
            e,
            c,
            vec![Value::Fx(d << 32), Value::Str(alloc_digit(d))],
        )
        .expect("digit dispatches");
    }
    let t = texts(&view);
    assert!(t.iter().any(|s| s == "123"), "typing 123 under de must show 123, got {t:?}");

    // …and a German-formatted result.
    let (oe, oc) = view.runtime().event_case("CalcEvent", "Op").expect("Op");
    view.dispatch(&tokens, &device, &locale, &mut host, oe, oc, vec![Value::Str("*".into())])
        .expect("op");
    for d in [1i64, 0] {
        view.dispatch(
            &tokens,
            &device,
            &locale,
            &mut host,
            e,
            c,
            vec![Value::Fx(d << 32), Value::Str(alloc_digit(d))],
        )
        .expect("digit");
    }
    let (qe, qc) = view.runtime().event_case("CalcEvent", "Eq").expect("Eq");
    view.dispatch(&tokens, &device, &locale, &mut host, qe, qc, vec![]).expect("eq");
    let t = texts(&view);
    assert!(t.iter().any(|s| s == "1.230"), "123 * 10 = 1.230 (de grouping), got {t:?}");
}
