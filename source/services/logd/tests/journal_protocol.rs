// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for logd journal + protocol bounds/decoding
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 28 unit tests + 3 property tests (journal, protocol, security)
//! ADR: docs/adr/0017-service-architecture.md
//!
//! Test categories (per SECURITY_STANDARDS.md):
//!   - Happy path: decode smoke, journal append/query/stats
//!   - Security (MUST): test_reject_* for all bounds and validation
//!   - Robustness (SHOULD): edge cases, encode-decode roundtrips
//!   - Fuzzing (MAY): proptest for protocol decoder

use logd::journal::{Journal, JournalError, LogLevel, LogRecord, RecordId, TimestampNsec};
use logd::protocol::{
    decode_request, encode_append_response, encode_query_response, encode_stats_response,
    DecodeError, Request, MAGIC0, MAGIC1, MAX_FIELDS_LEN, MAX_MSG_LEN, MAX_SCOPE_LEN, OP_APPEND,
    OP_QUERY, OP_STATS, STATUS_OK, VERSION,
};
use proptest::prelude::*;

// ============================================================================
// HAPPY PATH TESTS (existing)
// ============================================================================

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

// ============================================================================
// SECURITY TESTS (MUST) - test_reject_* per SECURITY_STANDARDS.md
// ============================================================================

#[test]
fn test_reject_oversized_scope() {
    let oversized_scope = vec![b'x'; MAX_SCOPE_LEN + 1];
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(2); // level INFO
    frame.push(oversized_scope.len() as u8); // scope_len > MAX_SCOPE_LEN
    frame.extend_from_slice(&(5u16).to_le_bytes()); // msg_len
    frame.extend_from_slice(&(0u16).to_le_bytes()); // fields_len
    frame.extend_from_slice(&oversized_scope);
    frame.extend_from_slice(b"hello");

    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::TooLarge));
}

#[test]
fn test_reject_oversized_message() {
    let oversized_msg = vec![b'x'; MAX_MSG_LEN + 1];
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(2); // level INFO
    frame.push(3); // scope_len
    frame.extend_from_slice(&(oversized_msg.len() as u16).to_le_bytes()); // msg_len > MAX_MSG_LEN
    frame.extend_from_slice(&(0u16).to_le_bytes()); // fields_len
    frame.extend_from_slice(b"svc");
    frame.extend_from_slice(&oversized_msg);

    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::TooLarge));
}

#[test]
fn test_reject_oversized_fields() {
    let oversized_fields = vec![b'x'; MAX_FIELDS_LEN + 1];
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(2); // level INFO
    frame.push(3); // scope_len
    frame.extend_from_slice(&(5u16).to_le_bytes()); // msg_len
    frame.extend_from_slice(&(oversized_fields.len() as u16).to_le_bytes()); // fields_len > MAX
    frame.extend_from_slice(b"svc");
    frame.extend_from_slice(b"hello");
    frame.extend_from_slice(&oversized_fields);

    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::TooLarge));
}

#[test]
fn test_reject_wrong_magic_first_byte() {
    let frame = [b'X', MAGIC1, VERSION, OP_STATS];
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_wrong_magic_second_byte() {
    let frame = [MAGIC0, b'X', VERSION, OP_STATS];
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_wrong_version() {
    let frame = [MAGIC0, MAGIC1, 99, OP_STATS]; // version 99 unsupported
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Unsupported));
}

#[test]
fn test_reject_unknown_opcode() {
    let frame = [MAGIC0, MAGIC1, VERSION, 99]; // opcode 99 unsupported
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Unsupported));
}

#[test]
fn test_reject_invalid_level() {
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(99); // level 99 invalid (only 0-4 valid)
    frame.push(3); // scope_len
    frame.extend_from_slice(&(5u16).to_le_bytes());
    frame.extend_from_slice(&(0u16).to_le_bytes());
    frame.extend_from_slice(b"svc");
    frame.extend_from_slice(b"hello");

    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_truncated_header() {
    // Frame too short (< 4 bytes)
    let frame = [MAGIC0, MAGIC1, VERSION]; // missing opcode
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_truncated_append() {
    // APPEND frame too short (< 10 bytes header)
    let frame = [MAGIC0, MAGIC1, VERSION, OP_APPEND, 2, 3]; // missing lengths
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_truncated_query() {
    // QUERY frame wrong length (!= 14 bytes)
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_QUERY]);
    frame.extend_from_slice(&123u64.to_le_bytes());
    // missing max_count
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_append_length_mismatch() {
    // APPEND frame: declared lengths don't match actual payload
    let mut frame = Vec::new();
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(2); // level INFO
    frame.push(3); // scope_len = 3
    frame.extend_from_slice(&(5u16).to_le_bytes()); // msg_len = 5
    frame.extend_from_slice(&(0u16).to_le_bytes()); // fields_len = 0
    frame.extend_from_slice(b"sv"); // only 2 bytes, but declared 3

    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

#[test]
fn test_reject_empty_frame() {
    let frame: [u8; 0] = [];
    let result = decode_request(&frame);
    assert_eq!(result, Err(DecodeError::Malformed));
}

// ============================================================================
// ROBUSTNESS TESTS (SHOULD) - edge cases, encode roundtrips
// ============================================================================

#[test]
fn journal_drop_oldest_by_bytes() {
    // cap_bytes = 200, each record ~70 bytes overhead + payload
    // Insert records until bytes overflow triggers drop
    let mut j = Journal::new(100, 200);
    j.append(1, TimestampNsec(1), LogLevel::Info, b"scope", b"message1", b"")
        .unwrap();
    j.append(1, TimestampNsec(2), LogLevel::Info, b"scope", b"message2", b"")
        .unwrap();
    j.append(1, TimestampNsec(3), LogLevel::Info, b"scope", b"message3", b"")
        .unwrap();

    let stats = j.stats();
    // Should have dropped at least one record due to bytes constraint
    assert!(stats.dropped_records >= 1 || stats.used_records <= 2);
}

#[test]
fn journal_query_empty_result_future_timestamp() {
    let mut j = Journal::new(10, 16 * 1024);
    j.append(1, TimestampNsec(100), LogLevel::Info, b"s", b"m", b"").unwrap();

    // Query with since_nsec > all timestamps → empty result
    let out = j.query(TimestampNsec(999), 10);
    assert!(out.is_empty());
}

#[test]
fn journal_query_max_count_zero() {
    let mut j = Journal::new(10, 16 * 1024);
    j.append(1, TimestampNsec(1), LogLevel::Info, b"s", b"m", b"").unwrap();

    // Query with max_count = 0 → empty result
    let out = j.query(TimestampNsec(0), 0);
    assert!(out.is_empty());
}

#[test]
fn journal_query_max_count_exceeds_records() {
    let mut j = Journal::new(10, 16 * 1024);
    j.append(1, TimestampNsec(1), LogLevel::Info, b"s", b"m1", b"").unwrap();
    j.append(1, TimestampNsec(2), LogLevel::Info, b"s", b"m2", b"").unwrap();

    // Query with max_count > actual records → returns all
    let out = j.query(TimestampNsec(0), 100);
    assert_eq!(out.len(), 2);
}

#[test]
fn journal_stats_initial() {
    let j = Journal::new(10, 1024);
    let stats = j.stats();
    assert_eq!(stats.total_records, 0);
    assert_eq!(stats.dropped_records, 0);
    assert_eq!(stats.capacity_records, 10);
    assert_eq!(stats.capacity_bytes, 1024);
    assert_eq!(stats.used_records, 0);
    assert_eq!(stats.used_bytes, 0);
}

#[test]
fn journal_record_too_large_for_capacity() {
    // Journal with very small capacity
    let mut j = Journal::new(1, 50);

    // Record that's larger than capacity_bytes
    let big_msg = vec![b'x'; 100];
    let result = j.append(1, TimestampNsec(1), LogLevel::Info, b"s", &big_msg, b"");
    assert_eq!(result, Err(JournalError::TooLarge));
}

#[test]
fn protocol_encode_append_response_roundtrip() {
    let response = encode_append_response(STATUS_OK, RecordId(42), 3);

    // Verify response structure: [L,O,ver,OP|0x80,status,record_id:u64,dropped:u64]
    assert_eq!(&response[0..4], &[MAGIC0, MAGIC1, VERSION, OP_APPEND | 0x80]);
    assert_eq!(response[4], STATUS_OK);
    assert_eq!(u64::from_le_bytes(response[5..13].try_into().unwrap()), 42);
    assert_eq!(u64::from_le_bytes(response[13..21].try_into().unwrap()), 3);
}

#[test]
fn protocol_encode_stats_response_roundtrip() {
    let stats = logd::journal::JournalStats {
        total_records: 100,
        dropped_records: 5,
        capacity_records: 50,
        capacity_bytes: 4096,
        used_records: 45,
        used_bytes: 3500,
    };
    let response = encode_stats_response(STATUS_OK, stats);

    // Verify response structure
    assert_eq!(&response[0..4], &[MAGIC0, MAGIC1, VERSION, OP_STATS | 0x80]);
    assert_eq!(response[4], STATUS_OK);

    let total = u64::from_le_bytes(response[5..13].try_into().unwrap());
    let dropped = u64::from_le_bytes(response[13..21].try_into().unwrap());
    let cap_records = u32::from_le_bytes(response[21..25].try_into().unwrap());
    let cap_bytes = u32::from_le_bytes(response[25..29].try_into().unwrap());
    let used_records = u32::from_le_bytes(response[29..33].try_into().unwrap());
    let used_bytes = u32::from_le_bytes(response[33..37].try_into().unwrap());

    assert_eq!(total, 100);
    assert_eq!(dropped, 5);
    assert_eq!(cap_records, 50);
    assert_eq!(cap_bytes, 4096);
    assert_eq!(used_records, 45);
    assert_eq!(used_bytes, 3500);
}

#[test]
fn protocol_encode_query_response_with_records() {
    // Build records via journal to avoid accessing private size_bytes field
    let mut j = Journal::new(10, 4096);
    j.append(42, TimestampNsec(1000), LogLevel::Info, b"test", b"hello", b"k=v")
        .unwrap();
    j.append(42, TimestampNsec(2000), LogLevel::Warn, b"test", b"world", b"")
        .unwrap();
    let records = j.query(TimestampNsec(0), 10);
    let stats = logd::journal::JournalStats {
        total_records: 2,
        dropped_records: 0,
        capacity_records: 10,
        capacity_bytes: 1024,
        used_records: 2,
        used_bytes: 190,
    };

    let response = encode_query_response(STATUS_OK, stats, &records);

    // Verify header
    assert_eq!(&response[0..4], &[MAGIC0, MAGIC1, VERSION, OP_QUERY | 0x80]);
    assert_eq!(response[4], STATUS_OK);

    // Verify stats
    let total = u64::from_le_bytes(response[5..13].try_into().unwrap());
    let dropped = u64::from_le_bytes(response[13..21].try_into().unwrap());
    let count = u16::from_le_bytes(response[21..23].try_into().unwrap());

    assert_eq!(total, 2);
    assert_eq!(dropped, 0);
    assert_eq!(count, 2);

    // Records follow after byte 23
    assert!(response.len() > 23);
}

#[test]
fn protocol_encode_query_response_empty() {
    let records: Vec<LogRecord> = vec![];
    let stats = logd::journal::JournalStats {
        total_records: 0,
        dropped_records: 0,
        capacity_records: 10,
        capacity_bytes: 1024,
        used_records: 0,
        used_bytes: 0,
    };

    let response = encode_query_response(STATUS_OK, stats, &records);

    // Verify header + stats + count=0
    assert_eq!(&response[0..4], &[MAGIC0, MAGIC1, VERSION, OP_QUERY | 0x80]);
    let count = u16::from_le_bytes(response[21..23].try_into().unwrap());
    assert_eq!(count, 0);

    // No records follow
    assert_eq!(response.len(), 23);
}

// ============================================================================
// INTEGRATION TESTS - full APPEND → query → response flow
// ============================================================================

#[test]
fn integration_append_query_roundtrip() {
    let mut journal = Journal::new(10, 4096);

    // Simulate APPEND request
    let append_frame = {
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
        frame.push(2); // INFO
        frame.push(4); // scope_len
        frame.extend_from_slice(&(5u16).to_le_bytes()); // msg_len
        frame.extend_from_slice(&(3u16).to_le_bytes()); // fields_len
        frame.extend_from_slice(b"test");
        frame.extend_from_slice(b"hello");
        frame.extend_from_slice(b"k=v");
        frame
    };

    // Decode and apply to journal
    match decode_request(&append_frame).unwrap() {
        Request::Append(req) => {
            let outcome = journal
                .append(
                    123, // service_id
                    TimestampNsec(1000),
                    req.level,
                    &req.scope,
                    &req.message,
                    &req.fields,
                )
                .unwrap();
            assert_eq!(outcome.record_id.0, 1);
        }
        _ => panic!("expected Append"),
    }

    // Query back
    let records = journal.query(TimestampNsec(0), 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].scope, b"test");
    assert_eq!(records[0].message, b"hello");
    assert_eq!(records[0].fields, b"k=v");
    assert_eq!(records[0].service_id, 123);
}

// ============================================================================
// PROPERTY TESTS (MAY) - fuzzing for protocol decoder
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn proptest_decode_never_panics(data: Vec<u8>) {
        // The decoder must never panic on arbitrary input
        let _ = decode_request(&data);
    }

    #[test]
    fn proptest_valid_append_decodes(
        level in 0u8..5,
        scope_len in 0usize..=MAX_SCOPE_LEN,
        msg_len in 0usize..=MAX_MSG_LEN,
        fields_len in 0usize..=MAX_FIELDS_LEN,
    ) {
        // Build a valid APPEND frame with random (but bounded) sizes
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
        frame.push(level);
        frame.push(scope_len as u8);
        frame.extend_from_slice(&(msg_len as u16).to_le_bytes());
        frame.extend_from_slice(&(fields_len as u16).to_le_bytes());
        frame.extend(core::iter::repeat(b'a').take(scope_len));
        frame.extend(core::iter::repeat(b'b').take(msg_len));
        frame.extend(core::iter::repeat(b'c').take(fields_len));

        // Must decode successfully
        let result = decode_request(&frame);
        prop_assert!(result.is_ok(), "valid frame should decode: {:?}", result);
    }

    #[test]
    fn proptest_journal_never_panics(
        cap_records in 1u32..100,
        cap_bytes in 64u32..4096,
        append_count in 0usize..50,
    ) {
        // Journal operations must never panic
        let mut j = Journal::new(cap_records, cap_bytes);
        for i in 0..append_count {
            let ts = TimestampNsec(i as u64);
            let _ = j.append(1, ts, LogLevel::Info, b"s", b"msg", b"");
        }
        let _ = j.query(TimestampNsec(0), 100);
        let _ = j.stats();
    }
}
