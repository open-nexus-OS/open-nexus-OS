// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The async recipes, proven (TASK-0077B P4 — `docs/dev/dsl/state.md`
//! §"Async recipes").
//!
//! One acceptance file for one subject, the shape `dsl_v0_1a_host/slots.rs`
//! established. It lives here because this crate already owns language +
//! runtime semantics AND has the `Script` host that can make a service fail on
//! demand — which is the whole point of a recipe about failure.
//!
//! What each test pins is the OBSERVABLE state a user would be left in, not the
//! internal path taken: an app that survives a failed load has cleared its
//! spinner and can say why, and one that cannot is indistinguishable from a
//! hung app no matter how the runtime got there.

use dsl_conformance::{compile, Harness, Script};
use nexus_dsl_runtime::Value;

/// Loading / Loaded / Failed / Empty — the four states a list can be in, and
/// the store shape that keeps them apart.
///
/// `busy` and `failed` are separate fields on purpose: an empty result is not a
/// failure, and collapsing them (an empty list meaning "something went wrong")
/// is how an app ends up showing an error for a folder that is simply empty.
const LIST_APP: &str = r#"
Store S {
    items: List<Str> = [],
    busy: Bool = false,
    failed: Int = 0,
}
Event E {
    Load,
    Loaded(List<Str>),
    Failed(Int),
}
reduce E {
    Load => {
        state.busy = true;
        state.failed = 0;
    },
    Loaded(items) => {
        state.items = items;
        state.busy = false;
        state.failed = 0;
    },
    Failed(code) => {
        state.failed = code;
        state.busy = false;
    },
}
@effect on Load {
    match svc.catalog.list() {
        Ok(items) => dispatch(Loaded(items)),
        Err(e) => dispatch(Failed(e)),
    }
}
Page P { Stack { Text("x") } }
"#;

#[test]
fn a_successful_load_leaves_items_and_no_failure() {
    let nxir = compile(LIST_APP);
    let mut h = Harness::mount(&nxir);
    let rows = Value::List(vec![Value::Str("a".into()), Value::Str("b".into())]);
    let mut script = Script::new(vec![("catalog.list", Ok(rows.clone()))]);
    h.dispatch(&mut script, "E", "Load", vec![]);
    h.assert_field("S", "busy", &Value::Bool(false));
    h.assert_field("S", "items", &rows);
    h.assert_field("S", "failed", &Value::Int(0));
}

/// The recipe's reason for existing: a failed load must CLEAR the spinner and
/// leave a stable code behind. Before TASK-0077B P3 the language let you write
/// a load with no `Err` arm, and the effect plan then stopped silently — `busy`
/// stayed true for ever with nothing to tell the user.
#[test]
fn a_failed_load_clears_the_spinner_and_keeps_a_stable_code() {
    let nxir = compile(LIST_APP);
    let mut h = Harness::mount(&nxir);
    let mut script = Script::new(vec![("catalog.list", Err(7))]);
    h.dispatch(&mut script, "E", "Load", vec![]);
    h.assert_field("S", "busy", &Value::Bool(false));
    h.assert_field("S", "failed", &Value::Int(7));
    h.assert_field("S", "items", &Value::List(vec![]));
}

/// EMPTY is not FAILED. A successful call that returns nothing leaves `failed`
/// at zero, so the page can say "nothing here" instead of "something broke".
#[test]
fn an_empty_result_is_not_a_failure() {
    let nxir = compile(LIST_APP);
    let mut h = Harness::mount(&nxir);
    let mut script = Script::new(vec![("catalog.list", Ok(Value::List(vec![])))]);
    h.dispatch(&mut script, "E", "Load", vec![]);
    h.assert_field("S", "busy", &Value::Bool(false));
    h.assert_field("S", "failed", &Value::Int(0));
    h.assert_field("S", "items", &Value::List(vec![]));
}

/// Retrying is re-dispatching the trigger, and a retry after a failure must
/// leave no trace of the failure behind — `Load` clears `failed` on the way in,
/// so a page cannot show a stale error over fresh results.
#[test]
fn a_retry_after_a_failure_clears_the_error() {
    let nxir = compile(LIST_APP);
    let mut h = Harness::mount(&nxir);
    let mut failing = Script::new(vec![("catalog.list", Err(7))]);
    h.dispatch(&mut failing, "E", "Load", vec![]);
    h.assert_field("S", "failed", &Value::Int(7));

    let rows = Value::List(vec![Value::Str("a".into())]);
    let mut ok = Script::new(vec![("catalog.list", Ok(rows.clone()))]);
    h.dispatch(&mut ok, "E", "Load", vec![]);
    h.assert_field("S", "failed", &Value::Int(0));
    h.assert_field("S", "items", &rows);
    h.assert_field("S", "busy", &Value::Bool(false));
}

/// An AUTOMATIC retry cannot be written, and the docs say so rather than
/// pretending otherwise: an effect body lowers to a bounded LINEAR plan, so
/// `if` inside an effect is `NX0501`. There is no way to consult an attempt
/// counter before re-dispatching, and an unconditional retry runs until
/// `MAX_DISPATCH_CASCADE` trips the budget — a failure bound, not a policy.
///
/// This test is the guard on that statement: if a conditional effect step ever
/// lands, this fails, and the chapter in `state.md` must be rewritten with it.
#[test]
fn test_reject_a_conditional_effect_step() {
    let src = r#"
Store S { n: Int = 0, }
Event E { Go, Done, }
reduce E { Go => state.n = 1, Done => state.n = 2, }
@effect on Go {
    if state.n == 0 {
        dispatch(Done);
    }
}
Page P { Stack { Text("x") } }
"#;
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "the check phase allows it: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    let err = nexus_dsl_core::lower_file(&file, &model, &canonical)
        .err()
        .expect("an effect cannot branch — lowering must reject it");
    assert_eq!(
        err.code,
        nexus_dsl_core::diag::DiagCode::LoweringUnsupported,
        "expected NX0501, got {err:?}"
    );
}
