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
///
/// Uses an EMPTY i18n key table, so every `@t(...)` in the re-emitted scene
/// reads back as the empty string. That is fine for a test that only looks at
/// geometry or at service traffic — and a trap for one that reads labels, so
/// use `dispatch_with_keys` and `program_i18n_keys` there.
pub fn dispatch(
    view: &mut View,
    device: &nexus_dsl_runtime::FixtureEnv,
    host: &mut dyn nexus_dsl_runtime::EffectHost,
    symbols: &[String],
    event: &str,
    case: &str,
    payload: Vec<Value>,
) {
    let keys: Vec<u32> = Vec::new();
    dispatch_with_keys(view, device, host, symbols, &keys, event, case, payload);
}

/// `dispatch` with the program's real i18n key table, so the re-emitted scene
/// still carries its labels (`program_i18n_keys`). Resolution is the BAKED
/// default catalog — `i18n/en.json` — not the app's authoring language.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_with_keys(
    view: &mut View,
    device: &nexus_dsl_runtime::FixtureEnv,
    host: &mut dyn nexus_dsl_runtime::EffectHost,
    symbols: &[String],
    keys: &[u32],
    event: &str,
    case: &str,
    payload: Vec<Value>,
) {
    let (e, c) = view
        .runtime()
        .event_case(event, case)
        .unwrap_or_else(|| panic!("unknown case {event}::{case}"));
    let locale = IdentityLocale { symbols, keys };
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

/// The program's i18n key table (key index -> symbol id). Without it an
/// `IdentityLocale` resolves every `@t(...)` to the empty string, so a scene
/// full of real labels reads back as a list of blanks.
pub fn program_i18n_keys(nxir: &[u8]) -> Vec<u32> {
    nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir)
        .expect("reads")
        .root()
        .expect("root")
        .get_i18n_keys()
        .expect("keys")
        .iter()
        .map(|k| k.get_key())
        .collect()
}

/// The injector's display point `name` (TASK-0333 6f-3, ADR-0089) — ONE source,
/// `proof-manifest/scenarios/targets.toml`: the visible lanes' scenarios press these points and
/// the shell tests assert that the layout puts each widget under its point.
pub fn inject_point(name: &str) -> (i32, i32) {
    *inject_targets()
        .points
        .get(name)
        .unwrap_or_else(|| panic!("targets.toml names no point `{name}`"))
}

/// The word `name` the injector types (the same file).
pub fn inject_word(name: &str) -> String {
    inject_targets()
        .words
        .get(name)
        .cloned()
        .unwrap_or_else(|| panic!("targets.toml names no word `{name}`"))
}

fn inject_targets() -> &'static nexus_proof_manifest::scenario::Targets {
    static TARGETS: std::sync::OnceLock<nexus_proof_manifest::scenario::Targets> =
        std::sync::OnceLock::new();
    TARGETS.get_or_init(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../source/apps/selftest-client/proof-manifest/scenarios/targets.toml");
        nexus_proof_manifest::scenario::read_targets(&path).unwrap_or_else(|e| panic!("{e}"))
    })
}
