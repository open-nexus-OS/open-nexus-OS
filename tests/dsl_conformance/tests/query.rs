// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: QuerySpec v1 conformance (TASK-0078B): the `query` effect step
//! against the REAL engine (EngineHost = nexus-query over MemKv), keyset
//! paging through DSL state, error paths, the pure-build/effect-execute
//! gate, and the v1 shape lints. AOT re-executes this corpus.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! TEST_COVERAGE: 6 tests

use dsl_conformance::{compile, EngineHost, Harness};
use nexus_dsl_runtime::Value;

const PAGED: &str = r#"
Store S {
    items: List<Str> = [],
    token: Str = "",
    err: Int = 0,
}

Event E {
    Load,
    Loaded(List<Str>, Str),
    Failed(Int),
}

reduce E {
    Load => state.err = 0,
    Loaded(rows, next) => {
        state.items = rows;
        state.token = next;
    },
    Failed(code) => state.err = code,
}

Query Recent on library {
    orderBy name,
    limit 2,
}

@effect on Load {
    match Recent(token: state.token) {
        Ok(rows, next) => dispatch(Loaded(rows, next)),
        Err(e) => dispatch(Failed(e)),
    }
}
"#;

fn strs(items: &[&str]) -> Value {
    Value::List(items.iter().map(|s| Value::Str(String::from(*s))).collect())
}

#[test]
fn query_step_pages_through_the_real_engine() {
    let nxir = compile(PAGED);
    let mut h = Harness::mount(&nxir);
    let mut host = EngineHost::new(&[("library", &["Delta", "Alpha", "Echo", "Bravo", "Charlie"])]);

    // Page 1: engine order (by name), limit 2, token stored in state.
    h.dispatch(&mut host, "E", "Load", vec![]);
    h.assert_field("S", "items", &strs(&["Alpha", "Bravo"]));
    let token1 = match h.runtime.field("S", "token") {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("token not a Str: {other:?}"),
    };
    assert!(!token1.is_empty(), "page 1 must continue");

    // Page 2 resumes FROM STATE — the keyset contract through DSL state.
    h.dispatch(&mut host, "E", "Load", vec![]);
    h.assert_field("S", "items", &strs(&["Charlie", "Delta"]));

    // Page 3 exhausts: next = "".
    h.dispatch(&mut host, "E", "Load", vec![]);
    h.assert_field("S", "items", &strs(&["Echo"]));
    h.assert_field("S", "token", &Value::Str(String::new()));

    assert_eq!(host.queries.len(), 3);
    assert_eq!(host.queries[0].limit, 2);
    assert_eq!(host.queries[0].order_col, "name");
}

#[test]
fn query_error_takes_the_err_arm_with_a_stable_code() {
    let nxir = compile(PAGED);
    let mut h = Harness::mount(&nxir);
    // No such source: the host answers with its stable code.
    let mut host = EngineHost::new(&[("other", &[])]);
    h.dispatch(&mut host, "E", "Load", vec![]);
    h.assert_field("S", "err", &Value::Int(i64::from(dsl_conformance::ERR_UNKNOWN_SOURCE)));
    h.assert_field("S", "items", &strs(&[])); // untouched
}

#[test]
fn query_params_flow_into_range_predicates() {
    let nxir = compile(
        r#"
Store S { items: List<Str> = [], }
Event E { Load(Str), Loaded(List<Str>, Str), Failed(Int), }
reduce E {
    Load(from) => state.items = state.items,
    Loaded(rows, next) => state.items = rows,
    Failed(code) => state.items = state.items,
}
Query FromName on library {
    params: { from: Str, },
    where name >= from,
    orderBy name,
    limit 10,
}
@effect on Load(from) {
    match FromName(from: from) {
        Ok(rows, next) => dispatch(Loaded(rows, next)),
        Err(e) => dispatch(Failed(e)),
    }
}
"#,
    );
    let mut h = Harness::mount(&nxir);
    let mut host = EngineHost::new(&[("library", &["Alpha", "Bravo", "Charlie", "Delta"])]);
    h.dispatch(&mut host, "E", "Load", vec![Value::Str(String::from("Charlie"))]);
    h.assert_field("S", "items", &strs(&["Charlie", "Delta"]));
    assert_eq!(host.queries[0].low, Some(Value::Str(String::from("Charlie"))));
}

#[test]
fn query_execution_in_a_reducer_is_impure() {
    let source = r#"
Store S { n: Int = 0, }
Event E { Tick, }
reduce E {
    Tick => {
        match Recent(token: "") {
            Ok(rows, next) => state.n = 1,
            Err(e) => state.n = 2,
        }
    },
}
Query Recent on library {
    orderBy name,
    limit 2,
}
"#;
    let file = nexus_dsl_core::parse_file(source).expect("parses");
    let (_, diags) = nexus_dsl_core::check_file(&file);
    assert!(
        diags.iter().any(|d| d.code.code() == "NX0405"),
        "expected NX0405 (reducer impure), got {diags:?}"
    );
}

#[test]
fn v1_shape_violations_are_nx0410() {
    // Range predicate off the order column.
    let source = r#"
Store S { n: Int = 0, }
Event E { Tick, }
reduce E { Tick => state.n = 0, }
Query Bad on library {
    where other >= "x",
    orderBy name,
    limit 5,
}
"#;
    let file = nexus_dsl_core::parse_file(source).expect("parses");
    let (_, diags) = nexus_dsl_core::check_file(&file);
    assert!(
        diags.iter().any(|d| d.code.code() == "NX0410"),
        "expected NX0410 (query shape), got {diags:?}"
    );

    // Zero limit.
    let source = source.replace("limit 5", "limit 0").replace("where other >= \"x\",\n", "");
    let file = nexus_dsl_core::parse_file(&source).expect("parses");
    let (_, diags) = nexus_dsl_core::check_file(&file);
    assert!(diags.iter().any(|d| d.code.code() == "NX0410"), "limit 0: {diags:?}");
}

#[test]
fn query_call_arity_is_checked() {
    // Missing declared param + unknown param name.
    let source = r#"
Store S { n: Int = 0, }
Event E { Tick, Ok2(List<Str>, Str), Bad(Int), }
reduce E { Tick => state.n = 0, Ok2(rows, next) => state.n = 1, Bad(code) => state.n = 2, }
Query Q on library {
    params: { from: Str, },
    where name >= from,
    orderBy name,
    limit 5,
}
@effect on Tick {
    match Q(wrong: "x") {
        Ok(rows, next) => dispatch(Ok2(rows, next)),
        Err(e) => dispatch(Bad(e)),
    }
}
"#;
    let file = nexus_dsl_core::parse_file(source).expect("parses");
    let (_, diags) = nexus_dsl_core::check_file(&file);
    let codes: Vec<&str> = diags.iter().map(|d| d.code.code()).collect();
    assert!(codes.contains(&"NX0303"), "unknown param name: {diags:?}");
    assert!(codes.contains(&"NX0302"), "missing param: {diags:?}");
}
