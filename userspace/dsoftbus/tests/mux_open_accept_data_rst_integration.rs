// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host integration tests for mux open/accept/data/close/rst endpoint behavior
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 tests
//!
//! TEST_SCOPE:
//!   - endpoint registry and stream acceptance semantics
//!   - integrated data multiplexing and fail-closed teardown behavior
//!
//! TEST_SCENARIOS:
//!   - open_accept_and_control_bulk_multiplexing(): parallel control and bulk stream flow
//!   - close_then_rst_propagates_fail_closed(): close->rst propagation and rejected post-rst send
//!   - ingest_duplicate_stream_name_rejects(): fail-closed duplicate name rejection on ingest path
//!   - unauthenticated_endpoint_rejects_open_and_ingest(): fail-closed unauthenticated endpoint behavior
//!   - close_after_reset_rejects_on_integration_path(): invalid teardown transition rejected on endpoint API
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg(nexus_env = "host")]

use dsoftbus::{
    MuxHostEndpoint, MuxWireEvent, PriorityClass, SendBudgetOutcome, StreamId, StreamName,
    StreamState, WindowCredit, REJECT_DUPLICATE_STREAM_NAME,
    REJECT_INVALID_STREAM_STATE_TRANSITION, REJECT_UNAUTHENTICATED_SESSION,
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

fn exchange(a: &mut MuxHostEndpoint, b: &mut MuxHostEndpoint) {
    for event in a.drain_outbound() {
        let _ = b.ingest(event).expect("ingest a->b");
    }
    for event in b.drain_outbound() {
        let _ = a.ingest(event).expect("ingest b->a");
    }
}

#[test]
fn open_accept_and_control_bulk_multiplexing() {
    let mut client = MuxHostEndpoint::new_authenticated(0);
    let mut server = MuxHostEndpoint::new_authenticated(0);

    let control_id = stream_id(1);
    let bulk_id = stream_id(2);

    client
        .open_stream(
            control_id,
            priority(0),
            stream_name("control"),
            WindowCredit::new(64 * 1024),
        )
        .expect("open control");
    client
        .open_stream(
            bulk_id,
            priority(3),
            stream_name("bulk"),
            WindowCredit::new(64 * 1024),
        )
        .expect("open bulk");
    exchange(&mut client, &mut server);

    let accept1 = server.accept_stream().expect("accept first");
    let accept2 = server.accept_stream().expect("accept second");
    let accepted_names = [
        accept1.name.as_str().to_string(),
        accept2.name.as_str().to_string(),
    ];
    assert!(accepted_names.iter().any(|n| n == "control"));
    assert!(accepted_names.iter().any(|n| n == "bulk"));

    let control_send = client
        .send_data(control_id, priority(0), 32)
        .expect("send control");
    assert!(matches!(control_send, SendBudgetOutcome::Sent { .. }));
    let bulk_send = client
        .send_data(bulk_id, priority(3), 8 * 1024)
        .expect("send bulk");
    assert!(matches!(bulk_send, SendBudgetOutcome::Sent { .. }));
    exchange(&mut client, &mut server);

    assert_eq!(server.buffered_bytes(control_id), Some(32));
    assert_eq!(server.buffered_bytes(bulk_id), Some(8 * 1024));
}

#[test]
fn close_then_rst_propagates_fail_closed() {
    let mut client = MuxHostEndpoint::new_authenticated(0);
    let mut server = MuxHostEndpoint::new_authenticated(0);
    let sid = stream_id(7);

    client
        .open_stream(sid, priority(1), stream_name("rpc"), WindowCredit::new(64))
        .expect("open");
    exchange(&mut client, &mut server);

    let _ = client.close_stream(sid, priority(1)).expect("close");
    exchange(&mut client, &mut server);
    assert_eq!(
        server.stream_state(sid),
        Some(StreamState::HalfClosedRemote)
    );

    let _ = client.reset_stream(sid, priority(1)).expect("reset");
    exchange(&mut client, &mut server);
    assert_eq!(server.stream_state(sid), Some(StreamState::Reset));

    let err = server
        .send_data(sid, priority(1), 8)
        .expect_err("reset stream must reject send");
    assert_eq!(err.label(), REJECT_INVALID_STREAM_STATE_TRANSITION);
}

#[test]
fn ingest_duplicate_stream_name_rejects() {
    let mut server = MuxHostEndpoint::new_authenticated(0);
    let first = MuxWireEvent::Open {
        stream_id: stream_id(30),
        priority: priority(0),
        name: stream_name("dup"),
    };
    let second = MuxWireEvent::Open {
        stream_id: stream_id(31),
        priority: priority(0),
        name: stream_name("dup"),
    };

    let _ = server.ingest(first).expect("first open ingest");
    let err = server
        .ingest(second)
        .expect_err("duplicate ingest name must reject");
    assert_eq!(err.label(), REJECT_DUPLICATE_STREAM_NAME);
}

#[test]
fn unauthenticated_endpoint_rejects_open_and_ingest() {
    let mut endpoint = MuxHostEndpoint::new_unauthenticated(0);
    let err = endpoint
        .open_stream(
            stream_id(40),
            priority(1),
            stream_name("rpc"),
            WindowCredit::new(64),
        )
        .expect_err("open must reject when endpoint is unauthenticated");
    assert_eq!(err.label(), REJECT_UNAUTHENTICATED_SESSION);

    let ingest_err = endpoint
        .ingest(MuxWireEvent::Open {
            stream_id: stream_id(41),
            priority: priority(1),
            name: stream_name("rpc2"),
        })
        .expect_err("ingest must reject when endpoint is unauthenticated");
    assert_eq!(ingest_err.label(), REJECT_UNAUTHENTICATED_SESSION);
}

#[test]
fn close_after_reset_rejects_on_integration_path() {
    let mut endpoint = MuxHostEndpoint::new_authenticated(0);
    let sid = stream_id(50);
    endpoint
        .open_stream(
            sid,
            priority(1),
            stream_name("rpc-close-reset"),
            WindowCredit::new(64),
        )
        .expect("open");
    let _ = endpoint.reset_stream(sid, priority(1)).expect("reset");

    let err = endpoint
        .close_stream(sid, priority(1))
        .expect_err("close after reset must reject");
    assert_eq!(err.label(), REJECT_INVALID_STREAM_STATE_TRANSITION);
}
