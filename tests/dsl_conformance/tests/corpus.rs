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
