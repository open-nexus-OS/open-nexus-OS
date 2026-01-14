//! CONTEXT: logd journal end-to-end integration tests
//! INTENT: Journal roundtrip, overflow, crash reports, multi-service logging
//! IDL (target): APPEND → Journal → QUERY → STATS
//! DEPS: logd (service integration)
//! READINESS: logd ready; loopback transport established
//! TESTS: APPEND/QUERY/STATS roundtrip, overflow (drop-oldest), crash events, concurrent appends
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(nexus_env = "host")]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use logd::journal::{Journal, LogLevel, RecordId, TimestampNsec};
use logd::protocol::{
    decode_request, encode_append_response, encode_query_response, encode_stats_response,
    AppendRequest, QueryRequest, Request, StatsRequest, MAGIC0, MAGIC1, OP_APPEND, OP_QUERY,
    OP_STATS, STATUS_OK, VERSION,
};
use nexus_ipc::{Client, LoopbackClient, Server, Wait};

/// Simulates a logd service loop with a bounded journal.
fn spawn_logd_service(cap_records: u32, cap_bytes: u32) -> LoopbackClient {
    let (client, server) = nexus_ipc::loopback_channel();
    thread::spawn(move || {
        let mut journal = Journal::new(cap_records, cap_bytes);
        let mut next_service_id = 1000u64;
        let mut next_timestamp = 1000u64;
        while let Ok(frame) = server.recv(Wait::Blocking) {
            let request = match decode_request(&frame) {
                Ok(req) => req,
                Err(_) => continue, // Malformed frame, skip
            };
            let response_frame = match request {
                Request::Append(AppendRequest { level, scope, message, fields }) => {
                    let service_id = next_service_id;
                    next_service_id += 1;
                    let timestamp_nsec = TimestampNsec(next_timestamp);
                    next_timestamp += 1000; // Increment by 1µs for determinism
                    match journal.append(
                        service_id,
                        timestamp_nsec,
                        level,
                        &scope,
                        &message,
                        &fields,
                    ) {
                        Ok(outcome) => encode_append_response(
                            STATUS_OK,
                            outcome.record_id,
                            outcome.dropped_records,
                        ),
                        Err(_) => encode_append_response(3, RecordId(0), 0), // TooLarge
                    }
                }
                Request::Query(QueryRequest { since_nsec, max_count }) => {
                    let records = journal.query(since_nsec, max_count);
                    let stats = journal.stats();
                    encode_query_response(STATUS_OK, stats, &records)
                }
                Request::Stats(StatsRequest) => {
                    let stats = journal.stats();
                    encode_stats_response(STATUS_OK, stats)
                }
            };
            if server.send(&response_frame, Wait::Blocking).is_err() {
                break; // Client disconnected
            }
        }
    });
    client
}

/// Sends an APPEND request and returns (record_id, dropped).
fn append(
    client: &LoopbackClient,
    level: LogLevel,
    scope: &[u8],
    message: &[u8],
    fields: &[u8],
) -> (RecordId, u64) {
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_APPEND];
    frame.push(level as u8);
    frame.push(scope.len() as u8);
    let msg_len = (message.len() as u16).to_le_bytes();
    frame.extend_from_slice(&msg_len);
    let fields_len = (fields.len() as u16).to_le_bytes();
    frame.extend_from_slice(&fields_len);
    frame.extend_from_slice(scope);
    frame.extend_from_slice(message);
    frame.extend_from_slice(fields);

    client.send(&frame, Wait::Blocking).expect("send APPEND");
    let response = client.recv(Wait::Blocking).expect("recv APPEND response");
    assert_eq!(response[0], MAGIC0);
    assert_eq!(response[1], MAGIC1);
    assert_eq!(response[2], VERSION);
    assert_eq!(response[3], OP_APPEND | 0x80);
    assert_eq!(response[4], 0); // status OK

    let record_id = u64::from_le_bytes(response[5..13].try_into().unwrap());
    let dropped = u64::from_le_bytes(response[13..21].try_into().unwrap());
    (RecordId(record_id), dropped)
}

/// Parsed query record (without private fields).
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryRecord {
    record_id: u64,
    timestamp_nsec: u64,
    service_id: u64,
    level: LogLevel,
    scope: Vec<u8>,
    message: Vec<u8>,
    fields: Vec<u8>,
}

/// Sends a QUERY request and returns the records.
fn query(client: &LoopbackClient, since_nsec: u64, max_count: u16) -> Vec<QueryRecord> {
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_QUERY];
    frame.extend_from_slice(&since_nsec.to_le_bytes());
    frame.extend_from_slice(&max_count.to_le_bytes());

    client.send(&frame, Wait::Blocking).expect("send QUERY");
    let response = client.recv(Wait::Blocking).expect("recv QUERY response");
    assert_eq!(response[0], MAGIC0);
    assert_eq!(response[1], MAGIC1);
    assert_eq!(response[2], VERSION);
    assert_eq!(response[3], OP_QUERY | 0x80);
    assert_eq!(response[4], 0); // status OK

    // Skip total_records (8) + dropped_records (8)
    let count = u16::from_le_bytes(response[21..23].try_into().unwrap());
    let mut records = Vec::new();
    let mut offset = 23;
    for _ in 0..count {
        let record_id = u64::from_le_bytes(response[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let timestamp_nsec = u64::from_le_bytes(response[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let level_byte = response[offset];
        offset += 1;
        let level = match level_byte {
            0 => LogLevel::Error,
            1 => LogLevel::Warn,
            2 => LogLevel::Info,
            3 => LogLevel::Debug,
            4 => LogLevel::Trace,
            _ => LogLevel::Info,
        };
        let service_id = u64::from_le_bytes(response[offset..offset + 8].try_into().unwrap());
        offset += 8;
        let scope_len = response[offset] as usize;
        offset += 1;
        let msg_len = u16::from_le_bytes(response[offset..offset + 2].try_into().unwrap()) as usize;
        offset += 2;
        let fields_len =
            u16::from_le_bytes(response[offset..offset + 2].try_into().unwrap()) as usize;
        offset += 2;
        let scope = response[offset..offset + scope_len].to_vec();
        offset += scope_len;
        let message = response[offset..offset + msg_len].to_vec();
        offset += msg_len;
        let fields = response[offset..offset + fields_len].to_vec();
        offset += fields_len;

        records.push(QueryRecord {
            record_id,
            timestamp_nsec,
            service_id,
            level,
            scope,
            message,
            fields,
        });
    }
    records
}

/// Sends a STATS request and returns (total, dropped, capacity_records, capacity_bytes).
fn stats(client: &LoopbackClient) -> (u64, u64, u32, u32) {
    let frame = vec![MAGIC0, MAGIC1, VERSION, OP_STATS];
    client.send(&frame, Wait::Blocking).expect("send STATS");
    let response = client.recv(Wait::Blocking).expect("recv STATS response");
    assert_eq!(response[0], MAGIC0);
    assert_eq!(response[1], MAGIC1);
    assert_eq!(response[2], VERSION);
    assert_eq!(response[3], OP_STATS | 0x80);
    assert_eq!(response[4], 0); // status OK

    let total = u64::from_le_bytes(response[5..13].try_into().unwrap());
    let dropped = u64::from_le_bytes(response[13..21].try_into().unwrap());
    let capacity_records = u32::from_le_bytes(response[21..25].try_into().unwrap());
    let capacity_bytes = u32::from_le_bytes(response[25..29].try_into().unwrap());
    (total, dropped, capacity_records, capacity_bytes)
}

#[test]
fn logd_append_query_stats_roundtrip() {
    let client = spawn_logd_service(10, 4096);

    // Initial STATS: empty journal
    let (total, dropped, capacity_records, _capacity_bytes) = stats(&client);
    assert_eq!(total, 0);
    assert_eq!(dropped, 0);
    assert_eq!(capacity_records, 10);

    // Append 5 records
    for i in 0..5 {
        let (record_id, dropped_count) =
            append(&client, LogLevel::Info, b"samgrd", format!("message {}", i).as_bytes(), b"");
        assert_eq!(record_id.0, i + 1);
        assert_eq!(dropped_count, 0);
    }

    // STATS: 5 records, 0 dropped
    let (total, dropped, _, _) = stats(&client);
    assert_eq!(total, 5);
    assert_eq!(dropped, 0);

    // QUERY: all 5 records
    let records = query(&client, 0, 100);
    assert_eq!(records.len(), 5);
    assert_eq!(records[0].scope, b"samgrd");
    assert_eq!(records[0].message, b"message 0");
    assert_eq!(records[4].message, b"message 4");

    drop(client);
}

#[test]
fn logd_overflow_drops_oldest() {
    let client = spawn_logd_service(5, 4096);

    // Append 5 records (fill capacity)
    for i in 0..5 {
        append(&client, LogLevel::Info, b"test", format!("msg {}", i).as_bytes(), b"");
    }

    let (total, dropped, _, _) = stats(&client);
    assert_eq!(total, 5);
    assert_eq!(dropped, 0);

    // Append 3 more → overflow, drop oldest 3
    for i in 5..8 {
        let (_, dropped_count) =
            append(&client, LogLevel::Info, b"test", format!("msg {}", i).as_bytes(), b"");
        assert_eq!(dropped_count, i - 4); // cumulative dropped
    }

    let (total, dropped, _, _) = stats(&client);
    assert_eq!(total, 8);
    assert_eq!(dropped, 3);

    // QUERY: only last 5 records remain
    let records = query(&client, 0, 100);
    assert_eq!(records.len(), 5);
    assert_eq!(records[0].message, b"msg 3");
    assert_eq!(records[4].message, b"msg 7");

    drop(client);
}

#[test]
fn logd_query_pagination_since_nsec() {
    let client = spawn_logd_service(100, 16384);

    // Append 10 records with artificial timestamps
    for i in 0..10 {
        append(&client, LogLevel::Info, b"test", format!("msg {}", i).as_bytes(), b"");
        // Sleep to ensure monotonic timestamps
        thread::sleep(Duration::from_millis(2));
    }

    let all_records = query(&client, 0, 100);
    assert_eq!(all_records.len(), 10);

    // Query since 5th record's timestamp
    let since = all_records[4].timestamp_nsec;
    let recent = query(&client, since, 100);
    assert!(recent.len() >= 5, "should return records >= since_nsec");

    // Query with max_count limit
    let limited = query(&client, 0, 3);
    assert_eq!(limited.len(), 3);

    drop(client);
}

#[test]
fn logd_crash_report_event() {
    let client = spawn_logd_service(50, 16384);

    // Append normal logs
    append(&client, LogLevel::Info, b"execd", b"spawned pid=42", b"");

    // Append crash event (simulates execd crash report)
    let crash_fields = b"event=crash.v1\npid=42\ncode=1\nname=demo.crash\n";
    append(&client, LogLevel::Error, b"execd", b"process crashed", crash_fields);

    // Query for crash events
    let records = query(&client, 0, 100);
    let crash_record =
        records.iter().find(|r| r.fields.windows(14).any(|w| w == b"event=crash.v1"));
    assert!(crash_record.is_some(), "crash event should be in journal");

    let crash = crash_record.unwrap();
    assert_eq!(crash.level, LogLevel::Error);
    assert_eq!(crash.scope, b"execd");
    assert!(crash.fields.windows(6).any(|w| w == b"pid=42"));
    assert!(crash.fields.windows(6).any(|w| w == b"code=1"));

    drop(client);
}

#[test]
fn logd_multi_service_concurrent_appends() {
    let client = spawn_logd_service(100, 16384);
    let client = Arc::new(Mutex::new(client));

    // Spawn 3 threads simulating different services
    let handles: Vec<_> = (0..3)
        .map(|service_id| {
            let client_clone = Arc::clone(&client);
            thread::spawn(move || {
                for i in 0..10 {
                    let client_guard = client_clone.lock().unwrap();
                    append(
                        &client_guard,
                        LogLevel::Info,
                        format!("service{}", service_id).as_bytes(),
                        format!("msg {}", i).as_bytes(),
                        b"",
                    );
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("thread completes");
    }

    // Verify all 30 records were appended
    let client_guard = client.lock().unwrap();
    let (total, dropped, _, _) = stats(&client_guard);
    assert_eq!(total, 30);
    assert_eq!(dropped, 0);

    let records = query(&client_guard, 0, 100);
    assert_eq!(records.len(), 30);

    drop(client_guard);
}

#[test]
fn logd_empty_fields_allowed() {
    let client = spawn_logd_service(10, 4096);

    let (record_id, dropped) = append(&client, LogLevel::Info, b"test", b"empty fields ok", b"");
    assert_eq!(record_id.0, 1);
    assert_eq!(dropped, 0);

    let records = query(&client, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fields, b"");

    drop(client);
}

#[test]
fn logd_bounded_scope_message_fields() {
    let client = spawn_logd_service(10, 4096);

    // Max allowed sizes
    let max_scope = vec![b'a'; 64];
    let max_msg = vec![b'b'; 256];
    let max_fields = vec![b'c'; 512];

    let (record_id, dropped) = append(&client, LogLevel::Info, &max_scope, &max_msg, &max_fields);
    assert_eq!(record_id.0, 1);
    assert_eq!(dropped, 0);

    let records = query(&client, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].scope.len(), 64);
    assert_eq!(records[0].message.len(), 256);
    assert_eq!(records[0].fields.len(), 512);

    drop(client);
}
