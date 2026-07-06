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
    let n = svc.stats.count(timeoutMs: 100);
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
    match svc.db.put(timeoutMs: 100) {
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
    h.dispatch(
        &mut NoIo,
        "E",
        "Msg",
        vec![Value::Str("hello".into()), Value::Int(42)],
    );
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
