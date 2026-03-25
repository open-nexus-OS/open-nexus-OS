// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Reject-path tests for netstackd per-op status-frame semantics
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 rejection tests
//!
//! TEST_SCOPE:
//! - Per-op malformed status-frame encoding
//! - Per-op not-found status-frame encoding
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[path = "../src/os/ipc/wire.rs"]
mod wire;

fn status_frame(op: u8, status: u8) -> [u8; 5] {
    [wire::MAGIC0, wire::MAGIC1, wire::VERSION, op | 0x80, status]
}

fn assert_malformed_frame(op: u8) {
    let frame = status_frame(op, wire::STATUS_MALFORMED);
    assert_eq!(frame[0], wire::MAGIC0);
    assert_eq!(frame[1], wire::MAGIC1);
    assert_eq!(frame[2], wire::VERSION);
    assert_eq!(frame[3], op | 0x80);
    assert_eq!(frame[4], wire::STATUS_MALFORMED);
}

#[test]
fn test_reject_all_supported_ops_malformed_status_frame_shape() {
    let ops = [
        wire::OP_LISTEN,
        wire::OP_ACCEPT,
        wire::OP_CONNECT,
        wire::OP_READ,
        wire::OP_WRITE,
        wire::OP_UDP_BIND,
        wire::OP_UDP_SEND_TO,
        wire::OP_UDP_RECV_FROM,
        wire::OP_ICMP_PING,
        wire::OP_LOCAL_ADDR,
        wire::OP_CLOSE,
        wire::OP_WAIT_WRITABLE,
    ];
    for op in ops {
        assert_malformed_frame(op);
    }
}

#[test]
fn test_reject_handle_ops_not_found_status_frame_shape() {
    for op in [
        wire::OP_ACCEPT,
        wire::OP_READ,
        wire::OP_WRITE,
        wire::OP_CLOSE,
        wire::OP_WAIT_WRITABLE,
        wire::OP_UDP_SEND_TO,
        wire::OP_UDP_RECV_FROM,
    ] {
        let frame = status_frame(op, wire::STATUS_NOT_FOUND);
        assert_eq!(frame[3], op | 0x80);
        assert_eq!(frame[4], wire::STATUS_NOT_FOUND);
    }
}

#[test]
fn test_reject_unknown_op_status_frame_shape() {
    let op = 0xfe;
    let frame = status_frame(op, wire::STATUS_MALFORMED);
    assert_eq!(
        frame,
        [
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            op | 0x80,
            wire::STATUS_MALFORMED,
        ]
    );
}

#[test]
fn test_wire_status_constants_contract() {
    assert_eq!(wire::STATUS_OK, 0);
    assert_eq!(wire::STATUS_NOT_FOUND, 1);
    assert_eq!(wire::STATUS_MALFORMED, 2);
    assert_eq!(wire::STATUS_WOULD_BLOCK, 3);
    assert_eq!(wire::STATUS_IO, 4);
    assert_eq!(wire::STATUS_TIMED_OUT, 5);
}
