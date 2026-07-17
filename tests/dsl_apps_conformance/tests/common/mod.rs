// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Shared per-app conformance helpers: compile the REAL project dir, mount,
// lay out at the display size, dispatch by name, and walk scene texts — the
// layout_viewport discipline, hoisted so each app test stays a page of intent.

#![allow(dead_code)]

use nexus_dsl_runtime::{IdentityLocale, Value, View};

pub fn app_root(app: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../userspace/apps").join(app)
}

pub fn compile(app: &str) -> Vec<u8> {
    nexus_dsl_core::compile_project_dir(&app_root(app))
        .unwrap_or_else(|e| panic!("{app} compiles: {e}"))
}

pub fn layout_boxes(view: &View) -> Vec<nexus_layout::LayoutBox> {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
}

/// Dispatches `Event::Case(payload)` by NAME (symbol-table lookup).
pub fn dispatch(
    view: &mut View,
    device: &nexus_dsl_runtime::FixtureEnv,
    host: &mut dyn nexus_dsl_runtime::EffectHost,
    symbols: &[String],
    event: &str,
    case: &str,
    payload: Vec<Value>,
) {
    let (e, c) = view
        .runtime()
        .event_case(event, case)
        .unwrap_or_else(|| panic!("unknown case {event}::{case}"));
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols, keys: &keys };
    view.dispatch(&nexus_theme_tokens::BaseTokens, device, &locale, host, e, c, payload)
        .unwrap_or_else(|err| panic!("dispatch {event}::{case}: {err:?}"));
}

/// Every Text content in the current scene (pre-order).
pub fn scene_texts(view: &View) -> Vec<String> {
    fn walk(node: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        match node {
            nexus_layout_types::LayoutNode::Stack(_, _, children)
            | nexus_layout_types::LayoutNode::Grid(_, _, children) => {
                for c in children {
                    walk(c, out);
                }
            }
            nexus_layout_types::LayoutNode::Text(t, _) => {
                out.push(String::from(t.content.as_str()))
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(view.scene(), &mut out);
    out
}

pub fn program_symbols(nxir: &[u8]) -> Vec<String> {
    nexus_dsl_runtime::Runtime::mount(nxir).expect("mounts runtime").symbols().to_vec()
}
