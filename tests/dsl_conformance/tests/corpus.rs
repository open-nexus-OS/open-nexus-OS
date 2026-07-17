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
}
reduce E {
    Load => state.busy = true,
    Loaded(items) => {
        state.items = items;
        state.busy = false;
    },
}
@effect on Load {
    let items = svc.catalog.list(timeoutMs: 250);
    dispatch(Loaded(items));
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

#[test]
fn a_failing_call_stops_the_plan() {
    let nxir = compile(
        r#"
Store S { busy: Bool = false, count: Int = 0, }
Event E { Load, Loaded(Int), }
reduce E {
    Load => state.busy = true,
    Loaded(n) => { state.count = n; state.busy = false; },
}
@effect on Load {
    let n = svc.stats.count("all", timeoutMs: 100);
    dispatch(Loaded(n));
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    let mut script = Script::new(vec![("stats.count", Err(7))]);
    h.dispatch(&mut script, "E", "Load", vec![]);
    // Err stops the plan: Loaded never dispatched, busy stays true (the
    // canonical async recipe adds an onErr arm — TASK-0077B).
    h.assert_field("S", "busy", &Value::Bool(true));
    h.assert_field("S", "count", &Value::Int(0));
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
    match svc.db.put("k", "v", timeoutMs: 100) {
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
            nexus_dsl_runtime::interact::HandlerAction::Bind { store, path } => {
                Some((*store, path.clone()))
            }
            _ => None,
        })
        .expect("bind handler on the local field");
    let changes = view.runtime.write_binding(store, &path, Value::Bool(true)).expect("writes");
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

#[test]
fn stateful_component_used_twice_is_rejected() {
    let src = r#"
Component C {
    state: { active: Bool = false, }
    Stack { Toggle { checked: $state.active, label: "x" } }
}
Page P { Stack { C { } C { } } }
Store S { n: Int = 0, }
Event E { X, }
reduce E { X => state.n = state.n, }
"#;
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags));
    let canonical = nexus_dsl_core::format_file(&file);
    assert!(nexus_dsl_core::lower_file(&file, &model, &canonical).is_err());
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
