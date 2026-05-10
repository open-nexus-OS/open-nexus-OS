// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host contract tests for mux frame/state transitions and keepalive behavior
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 7 tests
//!
//! TEST_SCOPE:
//!   - deterministic stream lifecycle transitions
//!   - frame boundedness and invalid transition rejection
//!   - keepalive and window update semantics
//!
//! TEST_SCENARIOS:
//!   - stream_lifecycle_open_ack_close_rst_is_deterministic(): canonical lifecycle sequence
//!   - data_frame_rejects_oversize_payload(): reject oversize DATA frames
//!   - open_ack_without_open_rejects(): fail-closed on invalid OPEN_ACK
//!   - keepalive_pong_resets_timeout_budget(): PONG updates keepalive activity baseline
//!   - window_update_frame_applies_credit_delta(): apply credit deltas deterministically
//!   - seeded_state_machine_sequence_preserves_credit_invariants(): deterministic sequence with stable accounting
//!   - rst_transition_is_idempotent_and_close_after_rst_rejects(): idempotent RST and fail-closed close-after-rst
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg(nexus_env = "host")]

use dsoftbus::{
    FrameApplyOutcome, InboundFrame, KeepaliveVerdict, MuxSessionState, PriorityClass,
    SendBudgetOutcome, StreamId, StreamState, StreamTransition, WindowCredit,
    REJECT_FRAME_OVERSIZE, REJECT_INVALID_STREAM_STATE_TRANSITION,
};

fn stream_id(raw: u32) -> StreamId {
    StreamId::new(raw).expect("stream id")
}

fn priority(raw: u8) -> PriorityClass {
    PriorityClass::new(raw).expect("priority class")
}

#[test]
fn stream_lifecycle_open_ack_close_rst_is_deterministic() {
    let mut session = MuxSessionState::new_authenticated(0);
    let sid = stream_id(7);

    let opened =
        session.apply_inbound_frame(sid, priority(1), InboundFrame::Open).expect("open frame");
    assert_eq!(opened, FrameApplyOutcome::StreamOpened { state: StreamState::Open });

    let acked = session
        .apply_inbound_frame(sid, priority(1), InboundFrame::OpenAck)
        .expect("open_ack frame");
    assert_eq!(acked, FrameApplyOutcome::OpenAcked { state: StreamState::Open });

    let closed_remote =
        session.apply_inbound_frame(sid, priority(1), InboundFrame::Close).expect("close frame");
    assert_eq!(
        closed_remote,
        FrameApplyOutcome::StreamTransitioned { state: StreamState::HalfClosedRemote }
    );

    let reset =
        session.apply_inbound_frame(sid, priority(1), InboundFrame::Rst).expect("rst frame");
    assert_eq!(reset, FrameApplyOutcome::StreamTransitioned { state: StreamState::Reset });
}

#[test]
fn data_frame_rejects_oversize_payload() {
    let mut session = MuxSessionState::new_authenticated(0);
    let sid = stream_id(9);
    let _ = session.apply_inbound_frame(sid, priority(0), InboundFrame::Open).expect("open frame");

    let err = session
        .apply_inbound_frame(sid, priority(0), InboundFrame::Data { payload_len: 32 * 1024 + 1 })
        .expect_err("oversize payload must reject");
    assert_eq!(err.label(), REJECT_FRAME_OVERSIZE);
}

#[test]
fn open_ack_without_open_rejects() {
    let mut session = MuxSessionState::new_authenticated(0);
    let err = session
        .apply_inbound_frame(stream_id(10), priority(0), InboundFrame::OpenAck)
        .expect_err("open_ack without open must reject");
    assert_eq!(err.label(), REJECT_INVALID_STREAM_STATE_TRANSITION);
}

#[test]
fn keepalive_pong_resets_timeout_budget() {
    let mut session = MuxSessionState::new_authenticated(0);
    let sid = stream_id(1);
    session.open_stream(sid, priority(0), WindowCredit::new(32)).expect("open stream");

    assert_eq!(session.keepalive_tick(3), KeepaliveVerdict::SendPing);
    let pong =
        session.apply_inbound_frame(sid, priority(0), InboundFrame::Pong).expect("pong frame");
    assert_eq!(pong, FrameApplyOutcome::KeepaliveObserved);

    // Timeout budget is measured since the latest peer activity; no immediate timeout after PONG.
    assert_eq!(session.keepalive_tick(10), KeepaliveVerdict::SendPing);
    assert_eq!(session.keepalive_tick(11), KeepaliveVerdict::Healthy);
}

#[test]
fn window_update_frame_applies_credit_delta() {
    let mut session = MuxSessionState::new_authenticated(0);
    let sid = stream_id(11);
    let _ = session.apply_inbound_frame(sid, priority(0), InboundFrame::Open).expect("open frame");

    let updated = session
        .apply_inbound_frame(sid, priority(0), InboundFrame::WindowUpdate { delta: 32 })
        .expect("window update");
    assert_eq!(updated, FrameApplyOutcome::WindowUpdated { credit: WindowCredit::new(65_568) });
}

#[test]
fn seeded_state_machine_sequence_preserves_credit_invariants() {
    let mut session = MuxSessionState::new_authenticated(100);
    let sid = stream_id(19);
    session.open_stream(sid, priority(2), WindowCredit::new(64)).expect("open stream");

    let first_send = session.send_data(sid, 24).expect("first send");
    assert_eq!(first_send, SendBudgetOutcome::Sent { remaining_credit: WindowCredit::new(40) });

    let updated = session
        .apply_inbound_frame(sid, priority(2), InboundFrame::WindowUpdate { delta: 8 })
        .expect("window update");
    assert_eq!(updated, FrameApplyOutcome::WindowUpdated { credit: WindowCredit::new(48) });

    let second_send = session.send_data(sid, 48).expect("second send");
    assert_eq!(second_send, SendBudgetOutcome::Sent { remaining_credit: WindowCredit::new(0) });

    let blocked = session.send_data(sid, 1).expect("bounded backpressure");
    assert_eq!(blocked, SendBudgetOutcome::WouldBlock { remaining_credit: WindowCredit::new(0) });
}

#[test]
fn rst_transition_is_idempotent_and_close_after_rst_rejects() {
    let mut session = MuxSessionState::new_authenticated(0);
    let sid = stream_id(31);
    session.open_stream(sid, priority(1), WindowCredit::new(32)).expect("open stream");

    let first_rst = session.apply_transition(sid, StreamTransition::Reset).expect("first reset");
    assert_eq!(first_rst.next_state, StreamState::Reset);

    let second_rst = session
        .apply_transition(sid, StreamTransition::Reset)
        .expect("second reset must be idempotent");
    assert_eq!(second_rst.next_state, StreamState::Reset);

    let err = session
        .apply_transition(sid, StreamTransition::SendClose)
        .expect_err("close after reset must reject");
    assert_eq!(err.label(), REJECT_INVALID_STREAM_STATE_TRANSITION);
}
