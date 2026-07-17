// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Transcript-host conformance (TASK-0078): `svc.*` effects replay
//! deterministically from checked-in transcript text — success, stable
//! error codes, and the replay-miss contract (a divergence is a hard
//! failure, never a silent default).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! TEST_COVERAGE: 3 tests

use dsl_conformance::{compile, Harness};
use nexus_dsl_runtime::svc::{TranscriptHost, ERR_TRANSCRIPT_MISS};
use nexus_dsl_runtime::Value;

const LOADER: &str = r#"
Store S {
    items: List<Str> = [],
    err: Int = 0,
    busy: Bool = false,
}

Event E {
    LoadRequested,
    Loaded(List<Str>),
    LoadFailed(Int),
}

reduce E {
    LoadRequested => state.busy = true,
    Loaded(rows) => {
        state.items = rows;
        state.busy = false;
    },
    LoadFailed(code) => {
        state.err = code;
        state.busy = false;
    },
}

@effect on LoadRequested {
    match svc.library.list(timeoutMs: 250) {
        Ok(rows) => dispatch(Loaded(rows)),
        Err(e) => dispatch(LoadFailed(e)),
    }
}
"#;

#[test]
fn transcript_replay_drives_the_loaded_state_update() {
    let nxir = compile(LOADER);
    let mut h = Harness::mount(&nxir);
    let mut host = TranscriptHost::parse(
        "# nx-transcript v1\n\
         call library.list() -> Ok(List[Str(\"Alpha\"),Str(\"Beta\")])\n",
    )
    .expect("fixture parses");

    h.dispatch(&mut host, "E", "LoadRequested", vec![]);
    h.assert_field(
        "S",
        "items",
        &Value::List(vec![Value::Str("Alpha".into()), Value::Str("Beta".into())]),
    );
    h.assert_field("S", "busy", &Value::Bool(false));
    assert!(host.is_clean(), "every transcript entry consumed, no misses");
}

#[test]
fn transcripted_error_takes_the_err_arm_with_the_stable_code() {
    let nxir = compile(LOADER);
    let mut h = Harness::mount(&nxir);
    let mut host = TranscriptHost::parse("call library.list() -> Err(7)\n").expect("parses");

    h.dispatch(&mut host, "E", "LoadRequested", vec![]);
    h.assert_field("S", "err", &Value::Int(7));
    h.assert_field("S", "busy", &Value::Bool(false));
    h.assert_field("S", "items", &Value::List(vec![]));
    assert!(host.is_clean());
}

#[test]
fn a_replay_miss_surfaces_as_the_miss_code_never_a_default() {
    // The transcript expects a DIFFERENT call — the divergence must be
    // visible in state (the miss code) and on the host (misses recorded).
    let nxir = compile(LOADER);
    let mut h = Harness::mount(&nxir);
    let mut host = TranscriptHost::parse("call other.method() -> Ok(Unit)\n").expect("parses");

    h.dispatch(&mut host, "E", "LoadRequested", vec![]);
    h.assert_field("S", "err", &Value::Int(i64::from(ERR_TRANSCRIPT_MISS)));
    assert!(!host.is_clean());
    assert_eq!(host.misses.len(), 1);
}
