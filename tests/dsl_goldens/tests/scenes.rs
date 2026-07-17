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
        let symbols =
            nexus_dsl_runtime::Runtime::mount(nxir).expect("pre-mount").symbols().to_vec();
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

    fn run_initial_effects(&mut self, host: &mut dyn nexus_dsl_runtime::EffectHost) -> Damage {
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        self.view
            .run_initial_effects(&BaseTokens, &FixtureEnv::default(), &locale, host)
            .expect("run_initial_effects runs")
    }

    fn field(&self, store: &str, field: &str) -> Option<Value> {
        self.view.runtime.field(store, field).cloned()
    }
}

/// A stub `EffectHost` that records calls and returns a fixed list — proves
/// an `on Mount` effect actually reached the service seam.
struct CountingHost {
    calls: u32,
    reply: Vec<Value>,
}

impl nexus_dsl_runtime::EffectHost for CountingHost {
    fn call(
        &mut self,
        _service: &str,
        _method: &str,
        _args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        self.calls += 1;
        Ok(Value::List(self.reply.clone()))
    }
}

/// `Load` carries an `@effect` but is dispatched by NOTHING — no handler, no
/// reducer, no other effect. It is therefore a ROOT: the runtime runs it once
/// at mount (principles.md §5 — the dataflow IS the trigger, there is no
/// `on Mount` hook). `Submit` is ALSO an effect trigger, but the button's
/// `on Tap` dispatches it → it is NOT a root and must NOT fire at mount.
const ROOT_LOADER: &str = r#"
Store S {
    items: List<Str> = [],
    loaded: Bool = false,
    submitted: Bool = false,
}

Event E {
    Load,
    Loaded(List<Str>),
    Failed(Int),
    Submit,
    Submitted,
}

reduce E {
    Load => state.loaded = false,
    Loaded(rows) => {
        state.items = rows;
        state.loaded = true;
    },
    Failed(code) => state.loaded = false,
    Submit => state.submitted = false,
    Submitted => state.submitted = true,
}

@effect on Load {
    match svc.library.list(timeoutMs: 250) {
        Ok(rows) => dispatch(Loaded(rows)),
        Err(e) => dispatch(Failed(e)),
    }
}

@effect on Submit {
    match svc.library.list(timeoutMs: 250) {
        Ok(rows) => dispatch(Submitted),
        Err(e) => dispatch(Failed(e)),
    }
}

Page P {
    Stack {
        Button { label: @t("go") }
            .bg(surfaceVariant)
        on Tap -> dispatch(Submit)
    }
}
"#;

#[test]
fn root_effect_loads_at_mount_without_a_lifecycle_hook() {
    let nxir = compile(ROOT_LOADER);
    let mut mounted = Mounted::new(&nxir);
    // Nothing has run yet: empty store, service untouched.
    assert_eq!(mounted.field("S", "loaded"), Some(Value::Bool(false)));

    let mut host = CountingHost {
        calls: 0,
        reply: vec![Value::Str(String::from("Alpha")), Value::Str(String::from("Beta"))],
    };
    // The runtime fires the ROOT effect (`Load`) once at mount — the source has
    // no `on Mount`, only `@effect on Load` that nobody dispatches.
    let _ = mounted.run_initial_effects(&mut host);
    assert_eq!(host.calls, 1, "the root effect reached the service once");
    assert_eq!(mounted.field("S", "loaded"), Some(Value::Bool(true)));
    assert_eq!(
        mounted.field("S", "items"),
        Some(Value::List(vec![
            Value::Str(String::from("Alpha")),
            Value::Str(String::from("Beta")),
        ]))
    );
    // `Submit` is dispatched by the button (a handler) → NOT a root → it never
    // fired at mount (no auto-submit).
    assert_eq!(mounted.field("S", "submitted"), Some(Value::Bool(false)));

    // Runs exactly once — a later frame does not re-load.
    let damage = mounted.run_initial_effects(&mut host);
    assert_eq!(host.calls, 1, "initial effects run once");
    assert_eq!(damage, Damage::None);
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
    let plus_rect =
        result.boxes.iter().find(|b| b.node_id == plus_box_id).expect("plus button box").rect;
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
    let reader =
        dsl_goldens::nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&nxir).expect("reads");
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
    let damage =
        mounted.view.navigate_back(&BaseTokens, &FixtureEnv::default(), &locale).expect("back");
    assert_eq!(damage, Damage::Layout);
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("home screen")));
}

/// Two-way bindings (IR v1.2 auto-bind): a tap on a bound Toggle flips the
/// field; text input on a bound TextField writes it — both through the one
/// store mutation path, both re-rendering via the dep set.
#[test]
fn two_way_bindings_flip_toggle_and_write_textfield() {
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::FxPx;

    let nxir = compile(
        r#"
Store S {
    dark: Bool = false,
    query: Str = "",
}
Event E { Noop, }
reduce E { Noop => state.dark = state.dark, }
Page P {
    Stack {
        Toggle { checked: $state.dark, label: "Dark" }
        TextField { label: "Search", value: $state.query }
        if $state.dark {
            Text("dark on")
        } else {
            Text("dark off")
        }
        Text($state.query)
    }
    .padding(2)
    .gap(2)
}
"#,
    );
    let mut mounted = Mounted::new(&nxir);
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("dark off")));

    let engine = LayoutEngine::new();
    let result = engine
        .layout(mounted.view.scene(), FxPx::new(160), &ui_v10_goldens::NoText)
        .expect("lays out");
    let locale = IdentityLocale { symbols: &mounted.symbols, keys: &mounted.keys };

    // Tap the toggle (its bind handler is the Tap-triggered one).
    let toggle_box = mounted
        .view
        .handlers()
        .iter()
        .find(|(_, h)| matches!(h.action, nexus_dsl_runtime::interact::HandlerAction::Bind { .. }))
        .expect("bind handler")
        .0;
    let rect = result.boxes.iter().find(|b| b.node_id == toggle_box).expect("box").rect;
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
    assert_eq!(damage, Some(Damage::Layout), "the branch flips");
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("dark on")));
    assert_eq!(
        mounted.view.runtime.field("S", "dark"),
        Some(&nexus_dsl_runtime::Value::Bool(true))
    );

    // Text input into the bound field (re-layout first: the scene changed).
    let result = engine
        .layout(mounted.view.scene(), FxPx::new(160), &ui_v10_goldens::NoText)
        .expect("relays");
    let field_box = mounted
        .view
        .handlers()
        .iter()
        .filter(|(_, h)| {
            matches!(h.action, nexus_dsl_runtime::interact::HandlerAction::Bind { .. })
        })
        .nth(1)
        .expect("textfield bind")
        .0;
    let rect = result.boxes.iter().find(|b| b.node_id == field_box).expect("box").rect;
    let damage = mounted
        .view
        .text_input(
            &BaseTokens,
            &FixtureEnv::default(),
            &locale,
            &result.boxes,
            FxPx::new(rect.x.0 + rect.width.0 / 2),
            FxPx::new(rect.y.0 + rect.height.0 / 2),
            "glass",
        )
        .expect("writes");
    assert_eq!(damage, Some(Damage::Layout));
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("glass")));
    assert_eq!(
        mounted.view.runtime.field("S", "query"),
        Some(&nexus_dsl_runtime::Value::Str("glass".into()))
    );
}

/// The master–detail example app builds from a PROJECT DIRECTORY (multi-file
/// merge + phone override) and drives list-tap → detail → back through the
/// interpreter — the Phase-6 launch-demo payload, proven host-side.
#[test]
fn masterdetail_project_navigates_and_respects_phone_override() {
    use nexus_dsl_core::{canonical_source_set, merge_project, SourceFile};
    use nexus_layout::LayoutEngine;
    use nexus_layout_types::FxPx;

    // Load the example app the same way the CLI project mode does.
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/dsl/masterdetail");
    let mut files = Vec::new();
    let mut stack = vec![root.join("ui")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable").flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("nx") {
                files.push(SourceFile {
                    path: p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"),
                    source: std::fs::read_to_string(&p).expect("readable"),
                });
            }
        }
    }
    let merged = merge_project(&files).expect("merges");
    let (model, diags) = nexus_dsl_core::check_file(&merged);
    assert!(!nexus_dsl_core::has_errors(&diags), "{diags:?}");
    let canonical = canonical_source_set(&files);
    let nxir = nexus_dsl_core::lower_file(&merged, &model, &canonical).expect("lowers").nxir;

    // Desktop: list page → tap the first card → detail page → back.
    let mut mounted = Mounted::new(&nxir);
    assert!(dsl_goldens::texts(mounted.view.scene()).contains(&String::from("library.title")));
    let engine = LayoutEngine::new();
    let result = engine
        .layout(mounted.view.scene(), FxPx::new(160), &ui_v10_goldens::NoText)
        .expect("lays out");
    let nav_box = mounted
        .view
        .handlers()
        .iter()
        .find(|(_, h)| {
            matches!(h.action, nexus_dsl_runtime::interact::HandlerAction::Navigate { .. })
        })
        .expect("card navigate handler")
        .0;
    let rect = result.boxes.iter().find(|b| b.node_id == nav_box).expect("box").rect;
    let locale = IdentityLocale { symbols: &mounted.symbols, keys: &mounted.keys };
    mounted
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
    let texts = dsl_goldens::texts(mounted.view.scene());
    assert!(texts.contains(&String::from("library.detail")), "desktop detail: {texts:?}");

    // Phone: the platform override drops the heading row.
    let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
    let keys = i18n_keys(&nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut phone =
        View::mount(&nxir, &BaseTokens, &FixtureEnv::phone("portrait"), &locale).expect("mounts");
    phone
        .navigate(&BaseTokens, &FixtureEnv::phone("portrait"), &locale, "/detail")
        .expect("navigates");
    let texts = dsl_goldens::texts(phone.scene());
    assert!(
        !texts.contains(&String::from("library.detail")),
        "phone override has no heading: {texts:?}"
    );
    assert!(texts.contains(&String::from("common.back")));
}
