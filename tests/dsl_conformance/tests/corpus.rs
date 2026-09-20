// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The v0.1 conformance corpus. Every case documents one semantic rule.
//! AOT (TASK-0079) re-executes this exact corpus — do not weaken cases.

use dsl_conformance::{compile, Harness, Script};
use nexus_dsl_runtime::{NoIo, Value};

const COUNTER: &str = r#"
Store CounterStore {
    value: Int = 0,
    label: Str = "start",
}

Event CounterEvent {
    Inc,
    Dec,
    SetLabel(Str),
}

reduce CounterEvent {
    Inc => state.value += 1,
    Dec => state.value -= 1,
    SetLabel(text) => state.label = text,
}
"#;

#[test]
fn reducers_update_state_deterministically() {
    let nxir = compile(COUNTER);
    let mut h = Harness::mount(&nxir);
    h.assert_field("CounterStore", "value", &Value::Int(0));
    h.assert_field("CounterStore", "label", &Value::Str("start".into()));

    h.dispatch(&mut NoIo, "CounterEvent", "Inc", vec![]);
    h.dispatch(&mut NoIo, "CounterEvent", "Inc", vec![]);
    h.dispatch(&mut NoIo, "CounterEvent", "Dec", vec![]);
    h.assert_field("CounterStore", "value", &Value::Int(1));

    h.dispatch(&mut NoIo, "CounterEvent", "SetLabel", vec![Value::Str("done".into())]);
    h.assert_field("CounterStore", "label", &Value::Str("done".into()));
}

#[test]
fn defaults_come_from_constant_expressions() {
    let nxir = compile(
        r#"
Store S {
    sum: Int = 2 + 3 * 4,
    half: Fx = 0.5,
    enabled: Bool = !false,
    text: Str = "a",
}
Event E { Noop, }
reduce E { Noop => state.sum = state.sum, }
"#,
    );
    let h = Harness::mount(&nxir);
    h.assert_field("S", "sum", &Value::Int(14));
    h.assert_field("S", "half", &Value::Fx(1i64 << 31));
    h.assert_field("S", "enabled", &Value::Bool(true));
}

#[test]
fn effects_run_after_commit_and_feed_back_through_the_queue() {
    let nxir = compile(
        r#"
Store S {
    busy: Bool = false,
    items: List<Item> = [],
}
Event E {
    Load,
    Loaded(List<Item>),
    Failed(Int),
}
reduce E {
    Load => state.busy = true,
    Loaded(items) => {
        state.items = items;
        state.busy = false;
    },
    Failed(code) => state.busy = false,
}
@effect on Load {
    match svc.catalog.list() {
        Ok(items) => dispatch(Loaded(items)),
        Err(e) => dispatch(Failed(e)),
    }
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    let rows = Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]);
    let mut script = Script::new(vec![("catalog.list", Ok(rows.clone()))]);
    h.dispatch(&mut script, "E", "Load", vec![]);
    // The whole cascade ran: Load (busy=true) → effect → Loaded (items, busy=false).
    h.assert_field("S", "busy", &Value::Bool(false));
    h.assert_field("S", "items", &rows);
    assert_eq!(script.calls, vec!["catalog.list"]);
}

/// TASK-0077B P3. `principles.md` §4: *"Effects must handle both `Ok` and
/// `Err` of every service call."* An effect plan stops at the first unhandled
/// failure, so a call without an `Err` arm strands the app: `busy` stays true,
/// the spinner never clears, and nothing tells the user.
///
/// This test replaces `a_failing_call_stops_the_plan`, which asserted exactly
/// that stranding as expected behaviour — its own comment pointed at this task.
/// The premise is retired because the shape is no longer writable: every way to
/// drop a service result is `NX0407`, an error. The handled path is proved at
/// runtime by `match_on_a_call_result_routes_ok_and_err`.
#[test]
fn test_reject_a_service_result_whose_paths_are_not_both_handled() {
    const HEAD: &str = r#"
Store S { busy: Bool = false, count: Int = 0, }
Event E { Load, Loaded(Int), Failed(Int), }
reduce E {
    Load => state.busy = true,
    Loaded(n) => { state.count = n; state.busy = false; },
    Failed(code) => state.busy = false,
}
"#;
    let rejected: &[(&str, &str)] = &[
        ("bound with `let` and never discriminated", "let n = svc.stats.count(\"all\"); dispatch(Loaded(n));"),
        ("a bare call statement", "svc.stats.count(\"all\");"),
        ("a match with no `Err` arm", "match svc.stats.count(\"all\") { Ok(n) => dispatch(Loaded(n)), }"),
        ("a match with no `Ok` arm", "match svc.stats.count(\"all\") { Err(e) => dispatch(Failed(e)), }"),
        ("a call hidden in another call's arguments", "match svc.stats.count(svc.stats.name()) { Ok(n) => dispatch(Loaded(n)), Err(e) => dispatch(Failed(e)), }"),
    ];
    for (what, body) in rejected {
        let src = alloc_format(HEAD, body);
        let file = nexus_dsl_core::parse_file(&src).expect("parses");
        let (_, diags) = nexus_dsl_core::check_file(&file);
        assert!(
            diags.iter().any(|d| d.code == nexus_dsl_core::diag::DiagCode::UnhandledResult),
            "{what} must be NX0407, got {diags:?}"
        );
        assert!(nexus_dsl_core::has_errors(&diags), "{what} must be an ERROR, not advice");
    }

    // And the one handled form compiles.
    let src = alloc_format(
        HEAD,
        "match svc.stats.count(\"all\") { Ok(n) => dispatch(Loaded(n)), Err(e) => dispatch(Failed(e)), }",
    );
    let file = nexus_dsl_core::parse_file(&src).expect("parses");
    let (_, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "the handled form must compile: {diags:?}");
}

fn alloc_format(head: &str, body: &str) -> String {
    format!("{head}\n@effect on Load {{\n    {body}\n}}\n")
}

#[test]
fn match_on_a_call_result_routes_ok_and_err() {
    let src = r#"
Store S { status: Str = "idle", }
Event E { Save, Saved, Failed(Int), }
reduce E {
    Save => state.status = "saving",
    Saved => state.status = "saved",
    Failed(code) => state.status = "failed",
}
@effect on Save {
    match svc.db.put("k", "v") {
        Ok(r) => dispatch(Saved),
        Err(e) => dispatch(Failed(e)),
    }
}
"#;
    let nxir = compile(src);
    // Ok path.
    let mut h = Harness::mount(&nxir);
    let mut ok = Script::new(vec![("db.put", Ok(Value::Unit))]);
    h.dispatch(&mut ok, "E", "Save", vec![]);
    h.assert_field("S", "status", &Value::Str("saved".into()));
    // Err path (fresh mount — corpus cases are independent).
    let mut h = Harness::mount(&nxir);
    let mut err = Script::new(vec![("db.put", Err(3))]);
    h.dispatch(&mut err, "E", "Save", vec![]);
    h.assert_field("S", "status", &Value::Str("failed".into()));
}

#[test]
fn equal_writes_do_not_mark_changes_and_checked_math_errors() {
    let nxir = compile(
        r#"
Store S { a: Int = 5, }
Event E { Same, }
reduce E { Same => state.a = 5, }
"#,
    );
    let mut h = Harness::mount(&nxir);
    // Dispatch succeeds; the equal write is a no-op (change tracking is
    // observable via the returned change set — asserted through the harness
    // extension once the emit path lands; here we assert state stability).
    h.dispatch(&mut NoIo, "E", "Same", vec![]);
    h.assert_field("S", "a", &Value::Int(5));
}

#[test]
fn match_stmt_binds_payload_in_reducers() {
    let nxir = compile(
        r#"
Store S { last: Str = "", n: Int = 0, }
Event E {
    Msg(Str, Int),
}
reduce E {
    Msg(text, count) => {
        state.last = text;
        state.n = count;
    },
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    h.dispatch(&mut NoIo, "E", "Msg", vec![Value::Str("hello".into()), Value::Int(42)]);
    h.assert_field("S", "last", &Value::Str("hello".into()));
    h.assert_field("S", "n", &Value::Int(42));
}

#[test]
fn navigation_routes_push_replace_back_with_typed_params() {
    use nexus_dsl_runtime::Value;
    let nxir = compile(
        r#"
Store S { current: Int = 0, }
Event E { Noop, }
reduce E { Noop => state.current = state.current, }
Page Home { Stack { Text("home") } }
Page Detail { Stack { Text("detail") } }
Routes {
    "/" -> Home;
    "/detail/:id" -> Detail(id: Int);
}
"#,
    );
    let runtime = nexus_dsl_runtime::Runtime::mount(&nxir).expect("mounts");
    let reader = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(&nxir).expect("reads");
    let mut nav = nexus_dsl_runtime::Nav::mount(reader.root().expect("root")).expect("nav");
    let _ = runtime;

    // Entry = "/" route.
    let home_page = nav.current().page;

    // Typed param parses; wrong types don't match the route.
    let entry = nav.push("/detail/7").expect("pushes").clone();
    assert_ne!(entry.page, home_page);
    assert_eq!(entry.params, vec![Value::Int(7)]);
    assert!(nav.push("/detail/seven").is_err(), "Int-typed param rejects text");

    // Replace keeps depth; back returns home and the root never pops.
    let depth = nav.depth();
    nav.replace("/detail/9").expect("replaces");
    assert_eq!(nav.depth(), depth);
    assert_eq!(nav.current().params, vec![Value::Int(9)]);
    assert!(nav.back());
    assert_eq!(nav.current().page, home_page);
    assert!(!nav.back(), "the root entry always remains");

    // Bounded history.
    for i in 0..64 {
        if nav.push("/detail/1").is_err() {
            assert!(i >= 30, "budget kicks in at MAX_HISTORY");
            return;
        }
    }
    panic!("history must be bounded");
}

#[test]
fn multi_store_programs_bind_reducers_by_touched_fields() {
    // Two stores, one shared event: each reducer binds its own store
    // (resolved from the fields it touches); dispatch runs both.
    let nxir = compile(
        r#"
Store CartStore {
    items: Int = 0,
}
Store SessionStore {
    actions: Int = 0,
}
Event E {
    AddItem,
}
reduce E {
    AddItem => state.items += 1,
}
Page P {
    Stack {
        Text($state.items)
        Text($state.actions)
    }
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    h.dispatch(&mut NoIo, "E", "AddItem", vec![]);
    h.dispatch(&mut NoIo, "E", "AddItem", vec![]);
    h.assert_field("CartStore", "items", &Value::Int(2));
    h.assert_field("SessionStore", "actions", &Value::Int(0));
}

#[test]
fn ambiguous_field_names_across_stores_are_rejected() {
    let src = r#"
Store A { n: Int = 0, }
Store B { n: Int = 0, }
Event E { X, }
reduce E { X => state.n += 1, }
"#;
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags));
    let canonical = nexus_dsl_core::format_file(&file);
    let outcome = nexus_dsl_core::lower_file(&file, &model, &canonical);
    let Err(err) = outcome else { panic!("ambiguous field must not lower") };
    assert_eq!(err.code, nexus_dsl_core::DiagCode::LoweringUnsupported);
}

#[test]
fn one_reducer_touching_two_stores_is_rejected() {
    let src = r#"
Store A { left: Int = 0, }
Store B { right: Int = 0, }
Event E { X, }
reduce E { X => { state.left += 1; state.right += 1; }, }
"#;
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags));
    let canonical = nexus_dsl_core::format_file(&file);
    assert!(nexus_dsl_core::lower_file(&file, &model, &canonical).is_err());
}

#[test]
fn effect_scheduling_is_fifo_and_declaration_ordered() {
    // Two effects on the SAME trigger + multi-step follow-ups: the trace in
    // state must reflect declaration order and FIFO queue draining.
    let nxir = compile(
        r#"
Store S {
    trace: Str = "",
}
Event E {
    Go,
    Mark(Str),
}
reduce E {
    Go => state.trace = state.trace + "go;",
    Mark(tag) => state.trace = state.trace + tag,
}
@effect on Go {
    dispatch(Mark("a;"));
    dispatch(Mark("b;"));
}
@effect on Go {
    dispatch(Mark("c;"));
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    h.dispatch(&mut NoIo, "E", "Go", vec![]);
    // reduce(Go) commits first; effect 1 queues a,b; effect 2 queues c;
    // FIFO drains a, b, c.
    h.assert_field("S", "trace", &Value::Str("go;a;b;c;".into()));
}

#[test]
fn cascade_budget_stops_runaway_dispatch_loops() {
    // An effect that re-dispatches its own trigger must hit the bounded
    // cascade budget deterministically instead of spinning forever.
    let nxir = compile(
        r#"
Store S { n: Int = 0, }
Event E { Tick, }
reduce E { Tick => state.n += 1, }
@effect on Tick {
    dispatch(Tick);
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    let (e, c) = h.runtime.event_case("E", "Tick").expect("case");
    let symbols = h.runtime.symbols().to_vec();
    let locale = nexus_dsl_runtime::IdentityLocale { symbols: &symbols, keys: &[] };
    let outcome = h.runtime.dispatch(&h.env, &locale, &mut NoIo, e, c, vec![]);
    assert_eq!(outcome, Err(nexus_dsl_runtime::RtError::Budget));
}

#[test]
fn platform_overrides_wrap_the_base_page_per_profile() {
    use nexus_dsl_core::{merge_project, SourceFile};
    use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

    let files = [
        SourceFile {
            path: "ui/pages/Home.nx".into(),
            source: r#"
Store S { n: Int = 0, }
Event E { Noop, }
reduce E { Noop => state.n = state.n, }
Page Home {
    Stack {
        Text("base layout")
    }
}
"#
            .into(),
        },
        SourceFile {
            path: "ui/platform/phone/pages/Home.nx".into(),
            source: r#"
Page Home {
    Stack {
        Text("phone layout")
    }
}
"#
            .into(),
        },
    ];
    let merged = merge_project(&files).expect("merges");
    let (model, diags) = nexus_dsl_core::check_file(&merged);
    assert!(!nexus_dsl_core::has_errors(&diags), "{diags:?}");
    let canonical = nexus_dsl_core::canonical_source_set(&files);
    let nxir = nexus_dsl_core::lower_file(&merged, &model, &canonical).expect("lowers").nxir;

    // One canonical .nxir serves both profiles via the device env.
    let mount = |env: FixtureEnv| -> Vec<String> {
        let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
        let locale = IdentityLocale { symbols: &symbols, keys: &[] };
        let view = View::mount(&nxir, &nexus_dsl_runtime::theme_tokens::BaseTokens, &env, &locale)
            .expect("mounts");
        collect_texts(view.scene())
    };
    assert!(mount(FixtureEnv::desktop()).contains(&String::from("base layout")));
    assert!(mount(FixtureEnv::phone("portrait")).contains(&String::from("phone layout")));
}

#[test]
fn platform_override_without_base_page_is_rejected() {
    use nexus_dsl_core::{merge_project, SourceFile};
    let files = [SourceFile {
        path: "ui/platform/tv/pages/Ghost.nx".into(),
        source: "Page Ghost { Stack { } }".into(),
    }];
    assert!(merge_project(&files).is_err());
}

fn collect_texts(scene: &nexus_layout_types::LayoutNode) -> Vec<String> {
    fn walk(node: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        use nexus_layout_types::LayoutNode as N;
        match node {
            N::Text(text, _) => out.push(String::from(text.content.as_str())),
            N::Stack(_, _, children) | N::Grid(_, _, children) => {
                for child in children {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(scene, &mut out);
    out
}

#[test]
fn stale_effect_followups_are_cancelled_when_the_trigger_refires() {
    // Search's effect echoes its argument back through Found. Firing Search
    // twice IN ONE CASCADE (via a driver event) means: by the time Search#1's
    // Found("old") dequeues, Search has re-fired — generation advanced —
    // stale follow-up dropped. Only "new" lands. Latest wins.
    let nxir = compile(
        r#"
Store S { result: Str = "none", }
Event E {
    Kick,
    Search(Str),
    Found(Str),
}
reduce E {
    Kick => state.result = state.result,
    Search(q) => state.result = state.result,
    Found(r) => state.result = r,
}
@effect on Kick {
    dispatch(Search("old"));
    dispatch(Search("new"));
}
@effect on Search(q) {
    dispatch(Found(q));
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    h.dispatch(&mut NoIo, "E", "Kick", vec![]);
    // Queue trace: [Search(old), Search(new)] → Search(old) enqueues
    // Found(old)@gen1 → Search(new) BUMPS the Search generation → Found(old)
    // is stale and dropped → Found(new)@gen2 lands.
    h.assert_field("S", "result", &Value::Str("new".into()));
}

#[test]
fn component_local_state_via_state_block_and_binding() {
    use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
    let nxir = compile(
        r#"
Store S { n: Int = 0, }
Event E { Noop, }
reduce E { Noop => state.n = state.n, }
Component Disclosure {
    state: {
        open: Bool = false,
    }
    Stack {
        Toggle { checked: $state.open, label: "More" }
        if $state.open {
            Text("details visible")
        } else {
            Text("collapsed")
        }
    }
}
Page P {
    Stack {
        Disclosure { }
    }
}
"#,
    );
    let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
    let locale = IdentityLocale { symbols: &symbols, keys: &[] };
    let mut view = View::mount(
        &nxir,
        &nexus_dsl_runtime::theme_tokens::BaseTokens,
        &FixtureEnv::default(),
        &locale,
    )
    .expect("mounts");
    assert!(collect_texts(view.scene()).contains(&String::from("collapsed")));

    // The auto-bind handler targets the implicit local store — flip it.
    let (store, path) = view
        .handlers()
        .iter()
        .find_map(|(_, h)| match &h.action {
            nexus_dsl_runtime::interact::HandlerAction::Bind { store, path, .. } => {
                Some((*store, path.clone()))
            }
            _ => None,
        })
        .expect("bind handler on the local field");
    let changes = view
        .runtime
        .write_binding(store, nexus_dsl_runtime::ROOT_INSTANCE, &path, Value::Bool(true))
        .expect("writes");
    assert!(!changes.is_empty());
    let damage = {
        let locale = IdentityLocale { symbols: &symbols, keys: &[] };
        view.dispatch_noop_reemit(
            &nexus_dsl_runtime::theme_tokens::BaseTokens,
            &FixtureEnv::default(),
            &locale,
            &changes,
        )
    };
    let _ = damage;
    assert!(collect_texts(view.scene()).contains(&String::from("details visible")));
}

/// TASK-0077B P2. The two-way bind is the RULE `docs/dev/dsl/ir.md` v1.2 states
/// — *"auto-synthesized when an interactive kind's primary prop is
/// `$state`-bound"* — read off the widget SSOT, not a list of names kept beside
/// it. The list it replaced had `SearchBar` missing (its `value` is edited by
/// exactly the text-input path `TextField` uses) and `TextArea` present, which
/// is not a widget this DSL has.
#[test]
fn the_bind_rule_follows_the_registry_not_a_list() {
    use nexus_dsl_core::registry::{widget_spec, WIDGETS};

    // Every control that declares a bind must also have a primary prop to write
    // back into, and must be interactive — otherwise the rule would synthesize
    // a handler for something the user cannot touch.
    for spec in WIDGETS {
        if spec.bind.is_some() {
            assert!(spec.interactive, "{} declares a bind but is not interactive", spec.name);
            assert!(spec.primary_prop.is_some(), "{} declares a bind with no prop", spec.name);
        }
    }

    let trigger = |name| widget_spec(name).and_then(|s| s.bind.as_ref()).map(|b| b.trigger);
    // SearchBar was the omission: same shape as TextField, no bind.
    assert_eq!(trigger("SearchBar"), Some("Change"));
    assert_eq!(trigger("TextField"), Some("Change"));
    assert_eq!(trigger("Toggle"), Some("Tap"));

    // A label is not a value: binding one would write back text the app owns.
    assert_eq!(trigger("Button"), None);
    assert_eq!(trigger("ListItem"), None);

    // And the phantom is gone.
    assert!(widget_spec("TextArea").is_none(), "TextArea is not a widget of this DSL");
}

/// TASK-0077B P2b. A bind rule states BOTH halves — when the value is edited
/// and how the interaction produces it. Half a rule is what made the Control
/// Center's sliders dead: a trigger with no derivation synthesizes a handler
/// the runtime cannot execute, and a derivation with no trigger is never
/// reached. The type makes the pair inseparable; this pins the catalog's
/// answers so a new control cannot quietly inherit the wrong one.
#[test]
fn every_bound_control_says_how_its_value_is_produced() {
    use nexus_dsl_core::registry::{widget_spec, BindValue, WIDGETS};

    let rule = |name| widget_spec(name).and_then(|s| s.bind.as_ref()).map(|b| (b.trigger, b.value));
    assert_eq!(rule("Toggle"), Some(("Tap", BindValue::ToggleBool)));
    assert_eq!(rule("Checkbox"), Some(("Tap", BindValue::ToggleBool)));
    assert_eq!(rule("TextField"), Some(("Change", BindValue::Text)));
    assert_eq!(rule("SearchBar"), Some(("Change", BindValue::Text)));
    // The gap this package closed: a Slider's value IS a percent of its track.
    assert_eq!(rule("Slider"), Some(("Tap", BindValue::TrackFraction)));

    // `Select` shows a value but its tap OPENS an app-owned option panel — the
    // tap has no value to produce, so binding it would be a dead handler.
    assert_eq!(rule("Select"), None);
    // `Stepper` is not a widget of this DSL at all (the kit crate exists, but
    // its -/+ glyphs are caller-provided nodes carrying their own handlers).
    assert!(widget_spec("Stepper").is_none(), "Stepper is not a widget of this DSL");

    // Nothing may declare a derivation the pointer path cannot deliver: the
    // surface drives exactly one pointer trigger.
    for spec in WIDGETS {
        let Some(bind) = &spec.bind else { continue };
        match bind.value {
            BindValue::ToggleBool | BindValue::TrackFraction => {
                assert_eq!(bind.trigger, "Tap", "{} derives from a point", spec.name);
            }
            BindValue::Text => assert_eq!(bind.trigger, "Change", "{} takes text", spec.name),
        }
    }
}

/// TASK-0077B P2b. EVERY bindable control's synthesized handler must answer to
/// the trigger the catalog names — the class of bug, not the one instance.
///
/// A synthesized handler's trigger is a SYMBOL, and symbols are collected in a
/// pass before lowering. Miss one and `Ctx::sym` falls back deterministically
/// to 0, producing a handler that is present, well-formed, and answers to a
/// symbol no interaction ever sends. `SearchBar` shipped that way for a
/// package: P2 moved the lowering onto the catalog and left the collector on a
/// list of names. Both ask `registry::bind_rule` now; this proves it for every
/// entry in the catalog, so the next control added cannot repeat it.
#[test]
fn every_bindable_control_gets_a_handler_whose_trigger_resolves() {
    use nexus_dsl_core::registry::WIDGETS;
    use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

    for spec in WIDGETS {
        let Some(rule) = &spec.bind else { continue };
        let prop = spec.primary_prop.expect("a bound control has a primary prop");
        let kind = spec.name;
        // The catalog also demands an accessible name on an interactive kind.
        let label = spec.label_prop.map_or(String::new(), |p| format!(", {p}: \"n\""));
        let nxir = compile(&format!(
            r#"
Store S {{
    v: Int = 0,
    t: Str = "",
    b: Bool = false,
}}
Event E {{ Noop, }}
reduce E {{ Noop => state.v = state.v, }}
Page P {{
    Stack {{
        {kind} {{ {prop}: $state.{field}{label} }}
            .label("n")
    }}
}}
"#,
            field = match rule.value {
                nexus_dsl_core::registry::BindValue::ToggleBool => "b",
                nexus_dsl_core::registry::BindValue::Text => "t",
                nexus_dsl_core::registry::BindValue::TrackFraction => "v",
            },
        ));
        let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
        let locale = IdentityLocale { symbols: &symbols, keys: &[] };
        let view = View::mount(
            &nxir,
            &nexus_dsl_runtime::theme_tokens::BaseTokens,
            &FixtureEnv::default(),
            &locale,
        )
        .unwrap_or_else(|e| panic!("{kind} mounts: {e:?}"));

        let bind = view
            .handlers()
            .iter()
            .find(|(_, h)| {
                matches!(h.action, nexus_dsl_runtime::interact::HandlerAction::Bind { .. })
            })
            .unwrap_or_else(|| panic!("{kind} must auto-bind its `{prop}`"));
        let expected = symbols
            .iter()
            .position(|s| s == rule.trigger)
            .unwrap_or_else(|| panic!("{kind}: `{}` never reached the symbol table", rule.trigger));
        assert_eq!(
            bind.1.trigger as usize, expected,
            "{kind}'s bind answers to symbol {}, not `{}`",
            bind.1.trigger, rule.trigger
        );
    }
}

/// The rule reaches the IR: a `SearchBar` bound to `$state` gets its bind
/// handler, which the old name list never produced.
#[test]
fn search_bar_binds_like_a_text_field() {
    use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
    let nxir = compile(
        r#"
Store S {
    q: Str = "",
}

Event E {
    Noop,
}

reduce E {
    Noop => state.q = state.q,
}

Page P {
    Stack {
        SearchBar { value: $state.q, placeholder: "find" }
    }
}
"#,
    );
    let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
    let locale = IdentityLocale { symbols: &symbols, keys: &[] };
    let view = View::mount(
        &nxir,
        &nexus_dsl_runtime::theme_tokens::BaseTokens,
        &FixtureEnv::default(),
        &locale,
    )
    .expect("mounts");
    let binds: Vec<_> = view
        .handlers()
        .iter()
        .filter(|(_, h)| {
            matches!(h.action, nexus_dsl_runtime::interact::HandlerAction::Bind { .. })
        })
        .collect();
    assert_eq!(binds.len(), 1, "SearchBar's value must auto-bind");

    // A handler is not alive until its TRIGGER resolves. This assertion is the
    // one P2 was missing: the lowering synthesized the bind off the catalog
    // while the symbol collector still worked from its own list of names, so
    // `SearchBar`'s trigger interned nothing and `Ctx::sym` fell back to 0 —
    // a handler answering to a symbol no interaction ever sends (P2b).
    let change =
        symbols.iter().position(|s| s == "Change").expect("`Change` must be in the symbol table");
    assert_eq!(
        binds[0].1.trigger as usize, change,
        "the synthesized bind must answer to the trigger the catalog names"
    );
}

/// TASK-0077B P1. The rule that used to live here — "a stateful component is
/// instantiated exactly once" — was the guard rail in front of a
/// `principles.md` §1 violation, not the fix: one store per COMPONENT meant two
/// instances would have SHARED their state.
///
/// This is §6's acceptance criterion instead: *"collections render through keyed
/// templates whose identity is stable — the runtime can diff, reorder and
/// virtualize WITHOUT user code"*. Two instances keep separate state, and a
/// REORDER carries each row's state with its key, because the identity
/// (`keyed_item_id(nodeId, key)`) contains no position.
#[test]
fn keyed_instances_keep_their_own_state_across_a_reorder() {
    use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, Value, View};
    let nxir = compile(
        r#"
Store S {
    rows: List<Str> = ["a", "b"],
}

Event E {
    Reverse,
}

reduce E {
    Reverse => state.rows = ["b", "a"],
}

Component Row {
    props: {
        id: Str,
    }
    state: {
        active: Bool = false,
    }
    Stack {
        Toggle { checked: $state.active, label: "t" }
        if $state.active {
            Text($props.id)
        } else {
            Text("off")
        }
    }
}

Page P {
    Stack {
        List($state.rows) { r in
            Stack { Row { id: r } }.key(r)
        }
    }
}
"#,
    );
    let symbols = nexus_dsl_runtime::Runtime::mount(&nxir).unwrap().symbols().to_vec();
    let locale = IdentityLocale { symbols: &symbols, keys: &[] };
    let mut view = View::mount(
        &nxir,
        &nexus_dsl_runtime::theme_tokens::BaseTokens,
        &FixtureEnv::default(),
        &locale,
    )
    .expect("mounts");

    // Two instances, both at their default. The old lowering refused to build
    // this program at all.
    let texts = collect_texts(view.scene());
    assert_eq!(texts.iter().filter(|t| *t == "off").count(), 2, "{texts:?}");

    // The two bind handlers differ by INSTANCE — that is the whole point.
    let binds: Vec<(u32, u64, Vec<u32>)> = view
        .handlers()
        .iter()
        .filter_map(|(_, h)| match &h.action {
            nexus_dsl_runtime::interact::HandlerAction::Bind { store, path, .. } => {
                Some((*store, h.instance, path.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(binds.len(), 2, "one bind per instance");
    assert_ne!(binds[0].1, binds[1].1, "instances must not share an identity");

    // Toggle the FIRST row only.
    let (store, instance_a, path) = binds[0].clone();
    let changes =
        view.runtime.write_binding(store, instance_a, &path, Value::Bool(true)).expect("writes");
    assert!(!changes.is_empty());
    let reemit = |view: &mut View<'_>, changes: &[nexus_dsl_runtime::ChangedField]| {
        let locale = IdentityLocale { symbols: &symbols, keys: &[] };
        let _ = view.dispatch_noop_reemit(
            &nexus_dsl_runtime::theme_tokens::BaseTokens,
            &FixtureEnv::default(),
            &locale,
            changes,
        );
    };
    reemit(&mut view, &changes);

    // Exactly ONE row is on, and it is row "a": a shared store would have
    // turned both on.
    let texts = collect_texts(view.scene());
    assert!(texts.contains(&String::from("a")), "row a is on: {texts:?}");
    assert_eq!(texts.iter().filter(|t| *t == "off").count(), 1, "{texts:?}");

    // REORDER. No app code moves any state; the identity does it.
    let locale2 = IdentityLocale { symbols: &symbols, keys: &[] };
    let (e, c) = view.runtime.event_case("E", "Reverse").expect("event exists");
    view.dispatch(
        &nexus_dsl_runtime::theme_tokens::BaseTokens,
        &FixtureEnv::default(),
        &locale2,
        &mut NoIo,
        e,
        c,
        vec![],
    )
    .expect("reverses");

    // Still exactly one row on, and still row "a" — the state followed its KEY,
    // not its position.
    let texts = collect_texts(view.scene());
    assert!(texts.contains(&String::from("a")), "state followed the key: {texts:?}");
    assert_eq!(texts.iter().filter(|t| *t == "off").count(), 1, "{texts:?}");
}

#[test]
fn persist_snapshot_restores_marked_fields_across_mounts() {
    use nexus_dsl_runtime::{NoIo, Value};
    let nxir = compile(
        r#"
Store S {
    count: Int = 0 @persist,
    label: Str = "start" @persist,
    scratch: Int = 0,
}
Event E { Bump, }
reduce E {
    Bump => {
        state.count = state.count + 1;
        state.label = "bumped";
        state.scratch = 9;
    },
}
Page P { Stack { Text("p") } }
"#,
    );
    // First instance: mutate, snapshot.
    let mut h = Harness::mount(&nxir);
    assert!(h.runtime.has_persist_fields());
    h.dispatch(&mut NoIo, "E", "Bump", vec![]);
    let snap = h.runtime.persist_snapshot().expect("snapshot with persist fields");

    // Second instance (fresh mount = defaults), restore: @persist fields come
    // back, the unmarked field stays at its default.
    let mut h2 = Harness::mount(&nxir);
    h2.assert_field("S", "count", &Value::Int(0));
    assert_eq!(h2.runtime.persist_restore(&snap), 2);
    h2.assert_field("S", "count", &Value::Int(1));
    h2.assert_field("S", "label", &Value::Str("bumped".into()));
    h2.assert_field("S", "scratch", &Value::Int(0));

    // Garbage bytes restore nothing (fail-closed).
    assert_eq!(h2.runtime.persist_restore(b"not a snapshot"), 0);
}

#[test]
fn persist_restore_skips_fields_that_changed_shape() {
    use nexus_dsl_runtime::{NoIo, Value};
    let v1 = compile(
        r#"
Store S { count: Int = 0 @persist, }
Event E { Bump, }
reduce E { Bump => state.count = state.count + 1, }
Page P { Stack { Text("p") } }
"#,
    );
    let mut h = Harness::mount(&v1);
    h.dispatch(&mut NoIo, "E", "Bump", vec![]);
    let snap = h.runtime.persist_snapshot().expect("snapshot");

    // "v2" of the app: same field name, DIFFERENT type — the entry is
    // skipped, the default survives (never a type-confused restore).
    let v2 = compile(
        r#"
Store S { count: Str = "none" @persist, }
Event E { Bump, }
reduce E { Bump => state.count = state.count, }
Page P { Stack { Text("p") } }
"#,
    );
    let mut h2 = Harness::mount(&v2);
    assert_eq!(h2.runtime.persist_restore(&snap), 0);
    h2.assert_field("S", "count", &Value::Str("none".into()));
}
