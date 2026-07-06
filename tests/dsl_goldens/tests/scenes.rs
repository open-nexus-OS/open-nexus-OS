// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Scene goldens + damage-class proofs for the interpreter (TASK-0076).
//! Regenerate goldens with `UPDATE_GOLDENS=1`.

use dsl_goldens::{compile, i18n_keys, COUNTER, TODO};
use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, NoIo, Value, View};
use nexus_theme_tokens::BaseTokens;

struct Mounted<'p> {
    view: View<'p>,
    symbols: Vec<String>,
    keys: Vec<u32>,
}

impl<'p> Mounted<'p> {
    fn new(nxir: &'p [u8]) -> Self {
        let symbols = nexus_dsl_runtime::Runtime::mount(nxir)
            .expect("pre-mount")
            .symbols()
            .to_vec();
        let keys = i18n_keys(nxir);
        let view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, &BaseTokens, &FixtureEnv::default(), &locale).expect("mounts")
        };
        Self { view, symbols, keys }
    }

    fn dispatch(&mut self, event: &str, case: &str, payload: Vec<Value>) -> Damage {
        let (e, c) = self.view.runtime.event_case(event, case).expect("event exists");
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        self.view
            .dispatch(&BaseTokens, &FixtureEnv::default(), &locale, &mut NoIo, e, c, payload)
            .expect("dispatch runs")
    }
}

#[test]
fn counter_scene_matches_golden_and_updates() {
    let nxir = compile(COUNTER);
    let mut mounted = Mounted::new(&nxir);
    ui_v10_goldens::check_golden("dsl_counter_initial", mounted.view.scene()).unwrap();

    let damage = mounted.dispatch("CounterEvent", "Inc", vec![]);
    assert_eq!(damage, Damage::Layout, "text content change re-measures");
    ui_v10_goldens::check_golden("dsl_counter_after_inc", mounted.view.scene()).unwrap();
}

#[test]
fn paint_only_dispatch_reports_paint_damage() {
    let nxir = compile(COUNTER);
    let mut mounted = Mounted::new(&nxir);
    // `busy` feeds ONLY `.disabled($state.busy)` — a paint-class dependency:
    // geometry stays valid, the host repaints with existing layout boxes.
    let damage = mounted.dispatch("CounterEvent", "SetBusy", vec![Value::Bool(true)]);
    assert_eq!(damage, Damage::Paint, "disabled() is paint-only");
    ui_v10_goldens::check_golden("dsl_counter_busy", mounted.view.scene()).unwrap();
}

#[test]
fn equal_write_reports_no_damage() {
    let nxir = compile(COUNTER);
    let mut mounted = Mounted::new(&nxir);
    // busy is already false — the equal write must not dirty anything.
    let damage = mounted.dispatch("CounterEvent", "SetBusy", vec![Value::Bool(false)]);
    assert_eq!(damage, Damage::None);
}

#[test]
fn todo_renders_keyed_collection_and_loading_branch() {
    let nxir = compile(TODO);
    let mut mounted = Mounted::new(&nxir);
    ui_v10_goldens::check_golden("dsl_todo_initial", mounted.view.scene()).unwrap();

    // Refresh flips the loading branch (structure change ⇒ layout damage);
    // the effect fails under NoIo, so the loading state stays visible.
    let damage = mounted.dispatch("TodoEvent", "Refresh", vec![]);
    assert_eq!(damage, Damage::Layout);
    ui_v10_goldens::check_golden("dsl_todo_loading", mounted.view.scene()).unwrap();

    // Loaded replaces the items and leaves the branch.
    let damage = mounted.dispatch(
        "TodoEvent",
        "Loaded",
        vec![Value::List(vec![Value::Str("alpha".into()), Value::Str("beta".into())])],
    );
    assert_eq!(damage, Damage::Layout);
    ui_v10_goldens::check_golden("dsl_todo_loaded", mounted.view.scene()).unwrap();
}

#[test]
fn scene_emission_is_deterministic() {
    let nxir = compile(TODO);
    let a = Mounted::new(&nxir);
    let b = Mounted::new(&nxir);
    assert_eq!(
        ui_v10_goldens::render_to_bgra(a.view.scene()).unwrap(),
        ui_v10_goldens::render_to_bgra(b.view.scene()).unwrap(),
        "two mounts render identical bytes"
    );
}
