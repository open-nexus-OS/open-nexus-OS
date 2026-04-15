// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Behavior-first reject proofs for TASK-0022 core no_std contract helpers
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 8 host tests (5 reject-path + Send/Sync contract + deterministic perf budget + zero-copy borrow-view)
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg(nexus_env = "host")]

use dsoftbus::{
    apply_stream_transition, validate_payload_identity_spoof_vs_sender_service_id,
    validate_record_bounds, CorrelationNonce, CorrelationWindow, PayloadIdentityClaim,
    PriorityClass, SendBudgetOutcome, SenderServiceId, StreamId, StreamState, StreamTransition,
    WindowCredit, REJECT_INVALID_STREAM_STATE_TRANSITION, REJECT_NONCE_MISMATCH_OR_STALE_REPLY,
    REJECT_OVERSIZE_FRAME_OR_RECORD, REJECT_PAYLOAD_IDENTITY_SPOOF_VS_SENDER_SERVICE_ID,
    REJECT_UNAUTHENTICATED_MESSAGE_PATH,
};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn test_reject_invalid_state_transition() {
    let mux_err = apply_stream_transition(StreamState::Closed, StreamTransition::SendClose)
        .expect_err("invalid stream transition must reject");
    assert_eq!(mux_err.label(), REJECT_INVALID_STREAM_STATE_TRANSITION);

    let core_err = dsoftbus::core_contract::reject_invalid_state_transition()
        .expect_err("core invalid transition helper must reject");
    assert_eq!(core_err.label(), dsoftbus::REJECT_INVALID_STATE_TRANSITION);
}

#[test]
fn test_reject_nonce_mismatch_or_stale_reply() {
    let mut correlation = CorrelationWindow::new_authenticated(11);

    let first_expected = correlation.reserve_outbound_nonce().expect("reserve outbound nonce");
    let mismatch = correlation
        .validate_inbound_reply(first_expected, CorrelationNonce::new(first_expected.get() + 1))
        .expect_err("nonce mismatch must reject");
    assert_eq!(mismatch.label(), REJECT_NONCE_MISMATCH_OR_STALE_REPLY);

    let second_expected = correlation.reserve_outbound_nonce().expect("reserve second nonce");
    correlation
        .validate_inbound_reply(second_expected, second_expected)
        .expect("first observation for nonce should pass");
    let stale = correlation
        .validate_inbound_reply(second_expected, second_expected)
        .expect_err("stale nonce replay must reject");
    assert_eq!(stale.label(), REJECT_NONCE_MISMATCH_OR_STALE_REPLY);
}

#[test]
fn test_reject_oversize_frame_or_record() {
    let err = validate_record_bounds(65, 64).expect_err("oversize control record must reject");
    assert_eq!(err.label(), REJECT_OVERSIZE_FRAME_OR_RECORD);
}

#[test]
fn test_reject_unauthenticated_message_path() {
    let mut correlation = CorrelationWindow::new_unauthenticated(7);
    let err =
        correlation.reserve_outbound_nonce().expect_err("unauthenticated message path must reject");
    assert_eq!(err.label(), REJECT_UNAUTHENTICATED_MESSAGE_PATH);
}

#[test]
fn test_reject_payload_identity_spoof_vs_sender_service_id() {
    let err = validate_payload_identity_spoof_vs_sender_service_id(
        SenderServiceId::new("samgrd"),
        PayloadIdentityClaim::new("bundlemgrd"),
    )
    .expect_err("payload identity mismatch must reject");
    assert_eq!(err.label(), REJECT_PAYLOAD_IDENTITY_SPOOF_VS_SENDER_SERVICE_ID);
}

#[test]
fn test_core_boundary_types_are_send_sync() {
    assert_send_sync::<CorrelationWindow>();
    assert_send_sync::<dsoftbus::MuxSessionState>();
    assert_send_sync::<dsoftbus::MuxHostEndpoint>();
    assert_send_sync::<dsoftbus::OwnedRecord>();
}

#[test]
fn test_perf_backpressure_budget_is_deterministic() {
    let stream_id = StreamId::new(1).expect("stream id");
    let priority = PriorityClass::new(0).expect("priority");
    let mut session = dsoftbus::MuxSessionState::new_authenticated(0);
    session.open_stream(stream_id, priority, WindowCredit::new(4)).expect("open stream");

    let first = session.send_data(stream_id, 8).expect("deterministic backpressure outcome");
    let second = session.send_data(stream_id, 8).expect("deterministic backpressure outcome");
    assert_eq!(first, SendBudgetOutcome::WouldBlock { remaining_credit: WindowCredit::new(4) });
    assert_eq!(second, first);
}

#[test]
fn test_zero_copy_borrow_view_preserves_payload_reference() {
    let record = dsoftbus::OwnedRecord::new(7, vec![1, 2, 3, 4]);
    let borrowed = record.borrow();
    assert_eq!(borrowed.channel(), 7);
    assert_eq!(borrowed.bytes(), &[1, 2, 3, 4]);
    assert!(
        core::ptr::eq(record.bytes().as_ptr(), borrowed.bytes().as_ptr()),
        "borrow-view must not reallocate/copy payload bytes"
    );
}
