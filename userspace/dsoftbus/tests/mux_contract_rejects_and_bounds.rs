// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host contract tests for deterministic mux rejects and bounded behavior
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 11 tests
//!
//! TEST_SCOPE:
//!   - deterministic reject labels
//!   - bounded flow-control and backpressure behavior
//!   - keepalive timeout determinism and scheduler starvation bounds
//!
//! TEST_SCENARIOS:
//!   - test_reject_mux_frame_oversize(): reject oversize outbound frames
//!   - test_reject_invalid_stream_state_transition(): reject illegal state transitions
//!   - test_reject_window_credit_overflow_or_underflow(): reject credit overflow and underflow
//!   - test_reject_unknown_stream_frame(): reject data for unknown stream id
//!   - test_reject_unauthenticated_session(): fail-closed when session is unauthenticated
//!   - backpressure_returns_would_block_when_credit_exhausted(): enforce bounded send budget
//!   - keepalive_timeout_is_deterministic(): emit deterministic keepalive verdicts
//!   - scheduler_enforces_bounded_starvation(): release lower priority stream within budget
//!   - scheduler_prevents_low_priority_starvation_with_mixed_classes(): avoid starvation across mixed lower classes
//!   - stream_name_rejects_empty_and_oversized_values(): reject invalid stream naming inputs
//!   - duplicate_stream_name_rejects_endpoint_open(): reject duplicate local stream name registration
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg(nexus_env = "host")]

use dsoftbus::mux_v2::{
    apply_stream_transition, apply_window_delta, KeepaliveVerdict, MuxHostEndpoint,
    MuxSessionState, PriorityClass, PriorityScheduler, SendBudgetOutcome, StreamId, StreamName,
    StreamState, StreamTransition, WindowCredit, HIGH_PRIORITY_BURST_LIMIT,
    MAX_FRAME_PAYLOAD_BYTES, REJECT_DUPLICATE_STREAM_NAME, REJECT_FRAME_OVERSIZE,
    REJECT_INVALID_STREAM_NAME, REJECT_INVALID_STREAM_STATE_TRANSITION,
    REJECT_UNAUTHENTICATED_SESSION, REJECT_UNKNOWN_STREAM_FRAME,
    REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW,
};

fn stream_id(raw: u32) -> StreamId {
    StreamId::new(raw).expect("stream id")
}

fn priority(raw: u8) -> PriorityClass {
    PriorityClass::new(raw).expect("priority class")
}

fn stream_name(raw: &str) -> StreamName {
    StreamName::new(raw).expect("stream name")
}

#[test]
fn test_reject_mux_frame_oversize() {
    let mut session = MuxSessionState::new_authenticated(0);
    session
        .open_stream(stream_id(1), priority(0), WindowCredit::new(u32::MAX))
        .expect("open stream");

    let err = session
        .send_data(stream_id(1), MAX_FRAME_PAYLOAD_BYTES + 1)
        .expect_err("oversize must reject");
    assert_eq!(err.label(), REJECT_FRAME_OVERSIZE);
}

#[test]
fn test_reject_invalid_stream_state_transition() {
    let err = apply_stream_transition(StreamState::Closed, StreamTransition::SendClose)
        .expect_err("invalid transition must reject");
    assert_eq!(err.label(), REJECT_INVALID_STREAM_STATE_TRANSITION);
}

#[test]
fn test_reject_window_credit_overflow_or_underflow() {
    let underflow =
        apply_window_delta(WindowCredit::new(0), -1).expect_err("underflow must reject");
    assert_eq!(
        underflow.label(),
        REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW
    );

    let overflow =
        apply_window_delta(WindowCredit::new(u32::MAX), 1).expect_err("overflow must reject");
    assert_eq!(overflow.label(), REJECT_WINDOW_CREDIT_OVERFLOW_OR_UNDERFLOW);
}

#[test]
fn test_reject_unknown_stream_frame() {
    let mut session = MuxSessionState::new_authenticated(0);
    session
        .open_stream(stream_id(1), priority(0), WindowCredit::new(64))
        .expect("open stream");

    let err = session
        .send_data(stream_id(42), 4)
        .expect_err("unknown stream must reject");
    assert_eq!(err.label(), REJECT_UNKNOWN_STREAM_FRAME);
}

#[test]
fn test_reject_unauthenticated_session() {
    let mut session = MuxSessionState::new_unauthenticated(0);
    let err = session
        .open_stream(stream_id(1), priority(0), WindowCredit::new(64))
        .expect_err("unauthenticated session must reject stream open");
    assert_eq!(err.label(), REJECT_UNAUTHENTICATED_SESSION);
}

#[test]
fn backpressure_returns_would_block_when_credit_exhausted() {
    let mut session = MuxSessionState::new_authenticated(0);
    session
        .open_stream(stream_id(1), priority(0), WindowCredit::new(4))
        .expect("open stream");

    let outcome = session
        .send_data(stream_id(1), 8)
        .expect("backpressure should not reject");
    assert_eq!(
        outcome,
        SendBudgetOutcome::WouldBlock {
            remaining_credit: WindowCredit::new(4)
        }
    );
}

#[test]
fn keepalive_timeout_is_deterministic() {
    let mut session = MuxSessionState::new_authenticated(0);
    assert_eq!(session.keepalive_tick(1), KeepaliveVerdict::Healthy);
    assert_eq!(session.keepalive_tick(3), KeepaliveVerdict::SendPing);
    assert_eq!(session.keepalive_tick(9), KeepaliveVerdict::TimedOut);
}

#[test]
fn scheduler_enforces_bounded_starvation() {
    let mut scheduler = PriorityScheduler::new();
    let high = stream_id(1);
    let low = stream_id(2);

    scheduler.enqueue(priority(0), high);
    scheduler.enqueue(priority(1), low);

    // Keep high-priority work continuously available to prove the starvation bound.
    for step in 0..=HIGH_PRIORITY_BURST_LIMIT {
        let next = scheduler.dequeue_next().expect("dequeue");
        if next == low {
            return;
        }
        assert_eq!(
            next, high,
            "only high-priority stream expected before low stream release"
        );
        scheduler.enqueue(priority(0), high);
        assert!(
            step < HIGH_PRIORITY_BURST_LIMIT,
            "low-priority stream must be released by starvation budget"
        );
    }

    panic!("low-priority stream was starved past the configured burst budget");
}

#[test]
fn scheduler_prevents_low_priority_starvation_with_mixed_classes() {
    let mut scheduler = PriorityScheduler::new();
    let high = stream_id(11);
    let medium = stream_id(12);
    let low = stream_id(13);

    scheduler.enqueue(priority(0), high);
    scheduler.enqueue(priority(2), medium);
    scheduler.enqueue(priority(7), low);

    let mut low_seen = false;
    let bound = ((HIGH_PRIORITY_BURST_LIMIT as usize) + 1) * 2;
    for _step in 0..=bound {
        let next = scheduler.dequeue_next().expect("dequeue");
        if next == high {
            scheduler.enqueue(priority(0), high);
        } else if next == medium {
            scheduler.enqueue(priority(2), medium);
        } else if next == low {
            low_seen = true;
            break;
        }
    }

    assert!(
        low_seen,
        "low-priority stream must be released under sustained mixed-priority pressure"
    );
}

#[test]
fn stream_name_rejects_empty_and_oversized_values() {
    let empty = StreamName::new("").expect_err("empty stream name must reject");
    assert_eq!(empty.label(), REJECT_INVALID_STREAM_NAME);

    let oversized_raw = "a".repeat(StreamName::MAX_LEN + 1);
    let oversized = StreamName::new(oversized_raw).expect_err("oversized stream name must reject");
    assert_eq!(oversized.label(), REJECT_INVALID_STREAM_NAME);
}

#[test]
fn duplicate_stream_name_rejects_endpoint_open() {
    let mut endpoint = MuxHostEndpoint::new_authenticated(0);
    endpoint
        .open_stream(
            stream_id(20),
            priority(1),
            stream_name("rpc"),
            WindowCredit::new(64),
        )
        .expect("first stream open");

    let err = endpoint
        .open_stream(
            stream_id(21),
            priority(1),
            stream_name("rpc"),
            WindowCredit::new(64),
        )
        .expect_err("duplicate stream name must reject");
    assert_eq!(err.label(), REJECT_DUPLICATE_STREAM_NAME);
}
