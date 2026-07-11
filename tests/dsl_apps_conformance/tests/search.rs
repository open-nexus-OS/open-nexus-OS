// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct Registry {
    id_sym: u32,
    label_sym: u32,
    queries: Vec<String>,
    launched: Vec<String>,
}
impl nexus_dsl_runtime::EffectHost for Registry {
    fn call(
        &mut self,
        svc: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        match (svc, method) {
            ("bundlemgr", "enumerate") => {
                if let Some(Value::Str(q)) = args.first() {
                    self.queries.push(q.clone());
                }
                let mut fields = vec![
                    (self.id_sym, Value::Str("counter".into())),
                    (self.label_sym, Value::Str("Counter".into())),
                ];
                fields.sort_by_key(|(sym, _)| *sym);
                Ok(Value::List(vec![Value::Record(fields)]))
            }
            ("ability", "launch") => {
                if let Some(Value::Str(id)) = args.first() {
                    self.launched.push(id.clone());
                }
                Ok(Value::Bool(true))
            }
            _ => Err(0),
        }
    }
}

/// A query dispatch runs the SERVICE-side search (`enumerate(query)`); the
/// result row's tap launches through the launch authority.
#[test]
fn search_query_and_result_launch() {
    let nxir = common::compile("search");
    let symbols = common::program_symbols(&nxir);
    let sym = |name: &str| symbols.iter().position(|s| s == name).expect(name) as u32;
    let mut host = Registry {
        id_sym: sym("id"),
        label_sym: sym("label"),
        queries: Vec::new(),
        launched: Vec::new(),
    };
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let empty: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &empty, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    common::dispatch(&mut view, &device, &mut host, &symbols, "SearchEvent", "QueryChanged", vec![]);
    assert_eq!(host.queries.len(), 1, "enumerate ran service-side");

    // The result row (a ListItem below the search bar) launches on tap.
    let boxes = common::layout_boxes(&view);
    let ids: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
    for id in ids {
        let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
        if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
            continue;
        }
        let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
        let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
        let locale = IdentityLocale { symbols: &empty, keys: &keys };
        let _ = view.pointer(&tokens, &device, &locale, &mut host, &boxes, "Tap", cx, cy);
        if !host.launched.is_empty() {
            break;
        }
    }
    assert_eq!(host.launched, vec![String::from("counter")]);
}
