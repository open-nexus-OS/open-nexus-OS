// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for logd journal + protocol bounds/decoding
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 unit tests
//! ADR: docs/adr/0017-service-architecture.md

use logd::journal::{Journal, LogLevel, TimestampNsec};
use logd::protocol::{decode_request, Request, MAGIC0, MAGIC1, OP_APPEND, OP_QUERY, OP_STATS, VERSION};

#[test]
fn journal_drop_oldest_by_records() {
    let mut j = Journal::new(2, 16 * 1024);
    j.append(1, TimestampNsec(1), LogLevel::Info, b"s", b"m1", b"").unwrap();
    j.append(1, TimestampNsec(2), LogLevel::Info, b"s", b"m2", b"").unwrap();
    j.append(1, TimestampNsec(3), LogLevel::Info, b"s", b"m3", b"").unwrap();
    let out = j.query(TimestampNsec(0), 10);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].message, b"m2");
    assert_eq!(out[1].message, b"m3");
    assert_eq!(j.stats().dropped_records, 1);
}

#[test]
fn protocol_decode_append_smoke() {
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(2); // level INFO
    frame.push(3); // scope_len
    frame.extend_from_slice(&(5u16).to_le_bytes()); // msg_len
    frame.extend_from_slice(&(0u16).to_le_bytes()); // fields_len
    frame.extend_from_slice(b"svc");
    frame.extend_from_slice(b"hello");

    match decode_request(&frame).expect("decode") {
        Request::Append(a) => {
            assert_eq!(a.scope, b"svc");
            assert_eq!(a.message, b"hello");
            assert!(a.fields.is_empty());
        }
        _ => panic!("wrong request"),
    }
}

#[test]
fn protocol_decode_query_smoke() {
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_QUERY]);
    frame.extend_from_slice(&123u64.to_le_bytes());
    frame.extend_from_slice(&7u16.to_le_bytes());
    match decode_request(&frame).expect("decode") {
        Request::Query(q) => {
            assert_eq!(q.since_nsec.0, 123);
            assert_eq!(q.max_count, 7);
        }
        _ => panic!("wrong request"),
    }
}

#[test]
fn protocol_decode_stats_smoke() {
    let frame = [MAGIC0, MAGIC1, VERSION, OP_STATS];
    match decode_request(&frame).expect("decode") {
        Request::Stats(_) => {}
        _ => panic!("wrong request"),
    }
}
