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

#[test]
fn live_pointer_tap_dispatches_through_hit_testing() {
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::FxPx;

    let nxir = compile(COUNTER);
    let mut mounted = Mounted::new(&nxir);
    assert!(!mounted.view.handlers().is_empty(), "counter has Tap handlers");

    // Lay out the scene, then tap inside the "+" button's box.
    let engine = LayoutEngine::new();
    let result = engine
        .layout(mounted.view.scene(), FxPx::new(160), &ui_v10_goldens::NoText)
        .expect("lays out");
    let plus_box_id = mounted.view.handlers().last().expect("has entries").0;
    let plus_rect = result
        .boxes
        .iter()
        .find(|b| b.node_id == plus_box_id)
        .expect("plus button box")
        .rect;
    let (cx, cy) = (
        FxPx::new(plus_rect.x.0 + plus_rect.width.0 / 2),
        FxPx::new(plus_rect.y.0 + plus_rect.height.0 / 2),
    );

    let locale = IdentityLocale { symbols: &mounted.symbols, keys: &mounted.keys };
    let damage = mounted
        .view
        .pointer(
            &BaseTokens,
            &FixtureEnv::default(),
            &locale,
            &mut NoIo,
            &result.boxes,
            "Tap",
            cx,
            cy,
        )
        .expect("pointer routes");
    assert_eq!(damage, Some(Damage::Layout), "Inc changes the counter text");
    assert_eq!(
        mounted.view.runtime.field("CounterStore", "value"),
        Some(&Value::Int(1)),
        "the + button dispatched Inc"
    );

    // A tap outside every handler hits nothing and changes nothing.
    let missed = mounted
        .view
        .pointer(
            &BaseTokens,
            &FixtureEnv::default(),
            &locale,
            &mut NoIo,
            &result.boxes,
            "Tap",
            FxPx::new(159),
            FxPx::new(9999),
        )
        .expect("pointer routes");
    assert_eq!(missed, None);
    assert_eq!(mounted.view.runtime.field("CounterStore", "value"), Some(&Value::Int(1)));
}

#[test]
fn disabled_nodes_take_no_input() {
    let nxir = compile(COUNTER);
    let mut mounted = Mounted::new(&nxir);
    let handlers_enabled = mounted.view.handlers().len();
    // busy=true disables the "+" button → its handler disappears.
    mounted.dispatch("CounterEvent", "SetBusy", vec![Value::Bool(true)]);
    assert_eq!(
        mounted.view.handlers().len(),
        handlers_enabled - 1,
        "the disabled button's Tap handler is not registered"
    );
}

const RESPONSIVE: &str = r#"
Store S { n: Int = 1, }
Event E { Noop, }
reduce E { Noop => state.n = state.n, }
Page P {
    Stack {
        if device.profile == desktop {
            Stack {
                Text("wide layout").textSize(lg)
                Text("sidebar")
            }
            .direction(row)
            .gap(4)
        } else if device.profile == tablet {
            Text("regular layout").textSize(base)
        } else {
            Text("compact").textSize(sm)
        }
        if device.orientation == landscape {
            Text("landscape hint")
        } else {
            Spacer
        }
    }
    .padding(3)
    .gap(2)
}
"#;

/// One page, five devices: the default-UI-plus-overrides story renders a
/// stable, DISTINCT golden per profile fixture (TASK-0077 DoD matrix).
#[test]
fn profile_matrix_goldens_are_stable_and_distinct() {
    let nxir = compile(RESPONSIVE);
    let variants: [(&str, FixtureEnv); 5] = [
        ("dsl_env_desktop", FixtureEnv::desktop()),
        ("dsl_env_phone_portrait", FixtureEnv::phone("portrait")),
        ("dsl_env_phone_landscape", FixtureEnv::phone("landscape")),
        ("dsl_env_tablet_portrait", FixtureEnv::tablet("portrait")),
        ("dsl_env_convertible_desktop", FixtureEnv::convertible("desktop")),
    ];
    let mut scenes = Vec::new();
    for (name, env) in variants {
        let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
        let keys = i18n_keys(&nxir);
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let view = View::mount(&nxir, &BaseTokens, &env, &locale).expect("mounts");
        ui_v10_goldens::check_golden(name, view.scene()).unwrap();
        scenes.push((name, dsl_goldens::texts(view.scene())));
    }
    // Different devices must take different branches (structural, since the
    // golden painter draws no glyphs — text sets carry the distinction).
    assert_ne!(scenes[0].1, scenes[1].1, "desktop vs phone");
    assert_ne!(scenes[1].1, scenes[3].1, "phone vs tablet");
    assert_ne!(scenes[1].1, scenes[2].1, "portrait vs landscape");
    assert!(scenes[0].1.contains(&String::from("wide layout")));
    assert!(scenes[1].1.contains(&String::from("compact")));
}

/// Locale switch: same program, catalog chain vs pseudo-locale — text changes,
/// scene stays deterministic per locale (re-emit on switch = Layout damage).
#[test]
fn locale_switch_changes_bound_text_deterministically() {
    use nexus_dsl_runtime::{i18n, Catalog, LocaleChain};
    let nxir = compile(TODO);
    let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
    let reader = dsl_goldens::nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&nxir)
        .expect("reads");
    let names = i18n::key_names(reader.root().expect("root"), &symbols);
    let name_strs: Vec<&str> = names.iter().map(String::as_str).collect();

    let de = Catalog::from_entries(
        &name_strs,
        &[("todo.title", "Aufgaben"), ("todo.refresh", "Neu laden"), ("common.loading", "Lädt…")],
    );
    let de_cats = [&de];
    let de_chain = LocaleChain::new(&de_cats, &names);
    let view_de =
        View::mount(&nxir, &BaseTokens, &FixtureEnv::default(), &de_chain).expect("mounts de");
    ui_v10_goldens::check_golden("dsl_todo_locale_de", view_de.scene()).unwrap();

    // Pseudo-locale (untranslated) renders the keys — visibly different.
    let keys = i18n_keys(&nxir);
    let pseudo = IdentityLocale { symbols: &symbols, keys: &keys };
    let view_pseudo =
        View::mount(&nxir, &BaseTokens, &FixtureEnv::default(), &pseudo).expect("mounts pseudo");
    let de_texts = dsl_goldens::texts(view_de.scene());
    let pseudo_texts = dsl_goldens::texts(view_pseudo.scene());
    assert_ne!(de_texts, pseudo_texts, "translated vs pseudo-locale must differ");
    assert!(de_texts.contains(&String::from("Aufgaben")), "de catalog applied: {de_texts:?}");
    assert!(
        pseudo_texts.contains(&String::from("todo.title")),
        "pseudo-locale shows the key: {pseudo_texts:?}"
    );
}

/// `navigate("/detail/…")` as a DSL handler action: a live tap switches the
/// page through the route table; back returns (schema v1.1 Handler.navigate).
#[test]
fn navigate_handler_switches_pages_on_tap() {
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::FxPx;

    let nxir = compile(
        r#"
Store S { n: Int = 0, }
Event E { Noop, }
reduce E { Noop => state.n = state.n, }
Page Home {
    Stack {
        Text("home screen")
        Button { label: "open" }
        on Tap -> navigate("/detail")
    }
    .padding(2)
    .gap(2)
}
Page Detail {
    Stack {
        Text("detail screen")
    }
}
Routes {
    "/" -> Home;
    "/detail" -> Detail;
}
"#,
    );
    let mut mounted = Mounted::new(&nxir);
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("home screen")));

    // Tap the button through real hit-testing.
    let engine = LayoutEngine::new();
    let result = engine
        .layout(mounted.view.scene(), FxPx::new(160), &ui_v10_goldens::NoText)
        .expect("lays out");
    let button_box = mounted.view.handlers()[0].0;
    let rect = result.boxes.iter().find(|b| b.node_id == button_box).expect("box").rect;
    let locale = IdentityLocale { symbols: &mounted.symbols, keys: &mounted.keys };
    let damage = mounted
        .view
        .pointer(
            &BaseTokens,
            &FixtureEnv::default(),
            &locale,
            &mut NoIo,
            &result.boxes,
            "Tap",
            FxPx::new(rect.x.0 + rect.width.0 / 2),
            FxPx::new(rect.y.0 + rect.height.0 / 2),
        )
        .expect("routes");
    assert_eq!(damage, Some(Damage::Layout));
    assert!(
        dsl_goldens::texts(mounted.view.scene()).contains(&String::from("detail screen")),
        "tap navigated to the detail page"
    );

    // And back.
    let damage = mounted
        .view
        .navigate_back(&BaseTokens, &FixtureEnv::default(), &locale)
        .expect("back");
    assert_eq!(damage, Damage::Layout);
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("home screen")));
}
