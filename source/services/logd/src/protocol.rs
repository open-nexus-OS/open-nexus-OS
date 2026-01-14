// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: logd OS-lite wire protocol v1 (versioned byte frames; bounded inputs)
//!
//! OWNERS: @runtime
//!
//! STATUS: Experimental
//!
//! API_STABILITY: Unstable
//!
//! TEST_COVERAGE: Tests in `source/services/logd/tests/journal_protocol.rs`
//!   - Decode: APPEND/QUERY/STATS happy path, reject oversized/malformed/invalid inputs
//!   - Encode: response roundtrips for all 3 opcodes
//!   - Property tests for panic-freedom on arbitrary input
//!
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use crate::journal::{JournalStats, LogLevel, LogRecord, RecordId, TimestampNsec};

pub const MAGIC0: u8 = b'L';
pub const MAGIC1: u8 = b'O';
pub const VERSION: u8 = 1;

pub const OP_APPEND: u8 = 1;
pub const OP_QUERY: u8 = 2;
pub const OP_STATS: u8 = 3;

pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_UNSUPPORTED: u8 = 2;
pub const STATUS_TOO_LARGE: u8 = 3;

pub const MAX_SCOPE_LEN: usize = 64;
pub const MAX_MSG_LEN: usize = 256;
pub const MAX_FIELDS_LEN: usize = 512;

/// A decoded v1 request.
#[derive(Debug, PartialEq)]
pub enum Request {
    Append(AppendRequest),
    Query(QueryRequest),
    Stats(StatsRequest),
}

#[derive(Debug, PartialEq)]
pub struct AppendRequest {
    pub level: LogLevel,
    pub scope: Vec<u8>,
    pub message: Vec<u8>,
    pub fields: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct QueryRequest {
    pub since_nsec: TimestampNsec,
    pub max_count: u16,
}

#[derive(Debug, PartialEq)]
pub struct StatsRequest;

/// Decode errors for v1 frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "decode errors must be handled"]
pub enum DecodeError {
    Malformed,
    Unsupported,
    TooLarge,
}

pub fn decode_request(frame: &[u8]) -> Result<Request, DecodeError> {
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(DecodeError::Malformed);
    }
    if frame[2] != VERSION {
        return Err(DecodeError::Unsupported);
    }
    match frame[3] {
        OP_APPEND => decode_append(frame),
        OP_QUERY => decode_query(frame),
        OP_STATS => decode_stats(frame),
        _ => Err(DecodeError::Unsupported),
    }
}

fn decode_append(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver,OP, level:u8, scope_len:u8, msg_len:u16le, fields_len:u16le, scope, msg, fields]
    if frame.len() < 10 {
        return Err(DecodeError::Malformed);
    }
    let level = decode_level(frame[4])?;
    let scope_len = frame[5] as usize;
    let msg_len = u16::from_le_bytes([frame[6], frame[7]]) as usize;
    let fields_len = u16::from_le_bytes([frame[8], frame[9]]) as usize;
    if scope_len > MAX_SCOPE_LEN || msg_len > MAX_MSG_LEN || fields_len > MAX_FIELDS_LEN {
        return Err(DecodeError::TooLarge);
    }
    let start = 10;
    let end_scope = start + scope_len;
    let end_msg = end_scope + msg_len;
    let end_fields = end_msg + fields_len;
    if frame.len() != end_fields {
        return Err(DecodeError::Malformed);
    }
    Ok(Request::Append(AppendRequest {
        level,
        scope: frame[start..end_scope].to_vec(),
        message: frame[end_scope..end_msg].to_vec(),
        fields: frame[end_msg..end_fields].to_vec(),
    }))
}

fn decode_query(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver,OP, since_nsec:u64le, max_count:u16le]
    if frame.len() != 14 {
        return Err(DecodeError::Malformed);
    }
    let since = u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]);
    let max_count = u16::from_le_bytes([frame[12], frame[13]]);
    Ok(Request::Query(QueryRequest { since_nsec: TimestampNsec(since), max_count }))
}

fn decode_stats(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver,OP]
    if frame.len() != 4 {
        return Err(DecodeError::Malformed);
    }
    Ok(Request::Stats(StatsRequest))
}

fn decode_level(byte: u8) -> Result<LogLevel, DecodeError> {
    match byte {
        0 => Ok(LogLevel::Error),
        1 => Ok(LogLevel::Warn),
        2 => Ok(LogLevel::Info),
        3 => Ok(LogLevel::Debug),
        4 => Ok(LogLevel::Trace),
        _ => Err(DecodeError::Malformed),
    }
}

fn encode_level(level: LogLevel) -> u8 {
    match level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Debug => 3,
        LogLevel::Trace => 4,
    }
}

pub fn encode_append_response(status: u8, record_id: RecordId, dropped: u64) -> Vec<u8> {
    // [L,O,ver,OP|0x80, status:u8, record_id:u64le, dropped:u64le]
    let mut out = Vec::with_capacity(21);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND | 0x80, status]);
    out.extend_from_slice(&record_id.0.to_le_bytes());
    out.extend_from_slice(&dropped.to_le_bytes());
    out
}

pub fn encode_stats_response(status: u8, stats: JournalStats) -> Vec<u8> {
    // [L,O,ver,OP|0x80, status, total:u64, dropped:u64, cap_records:u32, cap_bytes:u32, used_records:u32, used_bytes:u32]
    let mut out = Vec::with_capacity(4 + 1 + 8 + 8 + 4 + 4 + 4 + 4);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_STATS | 0x80, status]);
    out.extend_from_slice(&stats.total_records.to_le_bytes());
    out.extend_from_slice(&stats.dropped_records.to_le_bytes());
    out.extend_from_slice(&stats.capacity_records.to_le_bytes());
    out.extend_from_slice(&stats.capacity_bytes.to_le_bytes());
    out.extend_from_slice(&stats.used_records.to_le_bytes());
    out.extend_from_slice(&stats.used_bytes.to_le_bytes());
    out
}

pub fn encode_query_response(status: u8, stats: JournalStats, records: &[LogRecord]) -> Vec<u8> {
    // [L,O,ver,OP|0x80, status, total:u64, dropped:u64, count:u16, ...records]
    let count = core::cmp::min(records.len(), u16::MAX as usize) as u16;
    let mut out = Vec::new();
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_QUERY | 0x80, status]);
    out.extend_from_slice(&stats.total_records.to_le_bytes());
    out.extend_from_slice(&stats.dropped_records.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for rec in records.iter().take(count as usize) {
        encode_record(&mut out, rec);
    }
    out
}

fn encode_record(out: &mut Vec<u8>, rec: &LogRecord) {
    // [record_id:u64, ts:u64, level:u8, service_id:u64, scope_len:u8, msg_len:u16, fields_len:u16, scope, msg, fields]
    let scope_len = core::cmp::min(rec.scope.len(), MAX_SCOPE_LEN) as u8;
    let msg_len = core::cmp::min(rec.message.len(), MAX_MSG_LEN) as u16;
    let fields_len = core::cmp::min(rec.fields.len(), MAX_FIELDS_LEN) as u16;
    out.extend_from_slice(&rec.record_id.0.to_le_bytes());
    out.extend_from_slice(&rec.timestamp_nsec.0.to_le_bytes());
    out.push(encode_level(rec.level));
    out.extend_from_slice(&rec.service_id.to_le_bytes());
    out.push(scope_len);
    out.extend_from_slice(&msg_len.to_le_bytes());
    out.extend_from_slice(&fields_len.to_le_bytes());
    out.extend_from_slice(&rec.scope[..scope_len as usize]);
    out.extend_from_slice(&rec.message[..msg_len as usize]);
    out.extend_from_slice(&rec.fields[..fields_len as usize]);
}
