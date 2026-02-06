// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: logd OS-lite wire protocol v1+v2 (versioned byte frames; bounded inputs; v2 adds nonce correlation)
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

use crate::journal::{Journal, JournalStats, LogLevel, LogRecord, RecordId, TimestampNsec};

pub const MAGIC0: u8 = b'L';
pub const MAGIC1: u8 = b'O';
/// Protocol version v1 (legacy, no nonce correlation).
pub const VERSION: u8 = 1;
/// Protocol version v2 (nonce-correlated request/reply frames).
pub const VERSION_V2: u8 = 2;

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

/// Fixed upper bound for OS-lite QUERY responses.
pub const QUERY_BOUNDED_CAP: usize = 512;

/// A fixed-size encoded response frame (no heap allocations).
#[derive(Clone, Copy, Debug)]
pub struct BoundedFrame {
    pub buf: [u8; QUERY_BOUNDED_CAP],
    pub len: usize,
}

impl BoundedFrame {
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..core::cmp::min(self.len, self.buf.len())]
    }
}

/// A decoded logd request (v1 or v2).
#[derive(Debug, PartialEq)]
pub enum Request {
    Append(AppendRequest),
    Query(QueryRequest),
    Stats(StatsRequest),
    AppendV2(AppendRequestV2),
    QueryV2(QueryRequestV2),
    StatsV2(StatsRequestV2),
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

/// A decoded v2 APPEND request (nonce-correlated).
#[derive(Debug, PartialEq)]
pub struct AppendRequestV2 {
    pub nonce: u64,
    pub level: LogLevel,
    pub scope: Vec<u8>,
    pub message: Vec<u8>,
    pub fields: Vec<u8>,
}

/// A decoded v2 QUERY request (nonce-correlated).
#[derive(Debug, PartialEq)]
pub struct QueryRequestV2 {
    pub nonce: u64,
    pub since_nsec: TimestampNsec,
    pub max_count: u16,
}

/// A decoded v2 STATS request (nonce-correlated).
#[derive(Debug, PartialEq)]
pub struct StatsRequestV2 {
    pub nonce: u64,
}

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
    match frame[2] {
        VERSION => decode_request_v1(frame),
        VERSION_V2 => decode_request_v2(frame),
        _ => Err(DecodeError::Unsupported),
    }
}

fn decode_request_v1(frame: &[u8]) -> Result<Request, DecodeError> {
    match frame[3] {
        OP_APPEND => decode_append_v1(frame),
        OP_QUERY => decode_query_v1(frame),
        OP_STATS => decode_stats_v1(frame),
        _ => Err(DecodeError::Unsupported),
    }
}

fn decode_request_v2(frame: &[u8]) -> Result<Request, DecodeError> {
    // v2 frames insert `nonce:u64le` immediately after the 4-byte header:
    // [L,O,ver=2,OP, nonce:u64le, ...]
    if frame.len() < 12 {
        return Err(DecodeError::Malformed);
    }
    match frame[3] {
        OP_APPEND => decode_append_v2(frame),
        OP_QUERY => decode_query_v2(frame),
        OP_STATS => decode_stats_v2(frame),
        _ => Err(DecodeError::Unsupported),
    }
}

fn decode_append_v1(frame: &[u8]) -> Result<Request, DecodeError> {
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

fn decode_append_v2(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver=2,OP, nonce:u64le, level:u8, scope_len:u8, msg_len:u16le, fields_len:u16le, scope, msg, fields]
    if frame.len() < 18 {
        return Err(DecodeError::Malformed);
    }
    let nonce = u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]);
    let level = decode_level(frame[12])?;
    let scope_len = frame[13] as usize;
    let msg_len = u16::from_le_bytes([frame[14], frame[15]]) as usize;
    let fields_len = u16::from_le_bytes([frame[16], frame[17]]) as usize;
    if scope_len > MAX_SCOPE_LEN || msg_len > MAX_MSG_LEN || fields_len > MAX_FIELDS_LEN {
        return Err(DecodeError::TooLarge);
    }
    let start = 18;
    let end_scope = start + scope_len;
    let end_msg = end_scope + msg_len;
    let end_fields = end_msg + fields_len;
    if frame.len() != end_fields {
        return Err(DecodeError::Malformed);
    }
    Ok(Request::AppendV2(AppendRequestV2 {
        nonce,
        level,
        scope: frame[start..end_scope].to_vec(),
        message: frame[end_scope..end_msg].to_vec(),
        fields: frame[end_msg..end_fields].to_vec(),
    }))
}

fn decode_query_v1(frame: &[u8]) -> Result<Request, DecodeError> {
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

fn decode_query_v2(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver=2,OP, nonce:u64le, since_nsec:u64le, max_count:u16le]
    if frame.len() != 22 {
        return Err(DecodeError::Malformed);
    }
    let nonce = u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]);
    let since = u64::from_le_bytes([
        frame[12], frame[13], frame[14], frame[15], frame[16], frame[17], frame[18], frame[19],
    ]);
    let max_count = u16::from_le_bytes([frame[20], frame[21]]);
    Ok(Request::QueryV2(QueryRequestV2 { nonce, since_nsec: TimestampNsec(since), max_count }))
}

fn decode_stats_v1(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver,OP]
    if frame.len() != 4 {
        return Err(DecodeError::Malformed);
    }
    Ok(Request::Stats(StatsRequest))
}

fn decode_stats_v2(frame: &[u8]) -> Result<Request, DecodeError> {
    // [L,O,ver=2,OP, nonce:u64le]
    if frame.len() != 12 {
        return Err(DecodeError::Malformed);
    }
    let nonce = u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]);
    Ok(Request::StatsV2(StatsRequestV2 { nonce }))
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

pub fn encode_append_response_v2(status: u8, nonce: u64, record_id: RecordId, dropped: u64) -> Vec<u8> {
    // [L,O,ver=2,OP|0x80, status:u8, nonce:u64le, record_id:u64le, dropped:u64le]
    let mut out = Vec::with_capacity(29);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION_V2, OP_APPEND | 0x80, status]);
    out.extend_from_slice(&nonce.to_le_bytes());
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

pub fn encode_stats_response_v2(status: u8, nonce: u64, stats: JournalStats) -> Vec<u8> {
    // [L,O,ver=2,OP|0x80, status, nonce:u64, total:u64, dropped:u64, cap_records:u32, cap_bytes:u32, used_records:u32, used_bytes:u32]
    let mut out = Vec::with_capacity(4 + 1 + 8 + 8 + 8 + 4 + 4 + 4 + 4);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION_V2, OP_STATS | 0x80, status]);
    out.extend_from_slice(&nonce.to_le_bytes());
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

pub fn encode_query_response_v2(
    status: u8,
    nonce: u64,
    stats: JournalStats,
    records: &[LogRecord],
) -> Vec<u8> {
    // [L,O,ver=2,OP|0x80, status, nonce:u64, total:u64, dropped:u64, count:u16, ...records]
    let count = core::cmp::min(records.len(), u16::MAX as usize) as u16;
    let mut out = Vec::new();
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION_V2, OP_QUERY | 0x80, status]);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(&stats.total_records.to_le_bytes());
    out.extend_from_slice(&stats.dropped_records.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for rec in records.iter().take(count as usize) {
        encode_record(&mut out, rec);
    }
    out
}

/// Encodes a QUERY response into a fixed-size 512-byte frame, iterating records from the journal.
///
/// Determinism policy:
/// - if a particular record does not fit, it is skipped (so oversized early records can't starve later ones)
/// - `max_count` is still respected
pub fn encode_query_response_bounded_iter(
    status: u8,
    stats: JournalStats,
    journal: &Journal,
    since: TimestampNsec,
    max_count: u16,
) -> BoundedFrame {
    let mut buf = [0u8; QUERY_BOUNDED_CAP];
    let mut idx = 0usize;

    let write_u8 = |value: u8, out: &mut [u8], pos: &mut usize| {
        if *pos < out.len() {
            out[*pos] = value;
        }
        *pos += 1;
    };
    let write_u16 = |value: u16, out: &mut [u8], pos: &mut usize| {
        let bytes = value.to_le_bytes();
        for b in bytes {
            write_u8(b, out, pos);
        }
    };
    let write_u64 = |value: u64, out: &mut [u8], pos: &mut usize| {
        let bytes = value.to_le_bytes();
        for b in bytes {
            write_u8(b, out, pos);
        }
    };
    let write_bytes = |data: &[u8], out: &mut [u8], pos: &mut usize| {
        for b in data {
            write_u8(*b, out, pos);
        }
    };

    write_u8(MAGIC0, &mut buf, &mut idx);
    write_u8(MAGIC1, &mut buf, &mut idx);
    write_u8(VERSION, &mut buf, &mut idx);
    write_u8(OP_QUERY | 0x80, &mut buf, &mut idx);
    write_u8(status, &mut buf, &mut idx);
    write_u64(stats.total_records, &mut buf, &mut idx);
    write_u64(stats.dropped_records, &mut buf, &mut idx);
    let count_pos = idx;
    write_u16(0, &mut buf, &mut idx); // placeholder

    let mut count: u16 = 0;
    for rec in journal.iter_since(since) {
        if count >= max_count {
            break;
        }
        let scope = rec.scope.as_slice();
        let msg = rec.message.as_slice();
        let fields = rec.fields.as_slice();
        let scope_len = core::cmp::min(scope.len(), MAX_SCOPE_LEN) as u16;
        let msg_len = core::cmp::min(msg.len(), MAX_MSG_LEN) as u16;
        let fields_len = core::cmp::min(fields.len(), MAX_FIELDS_LEN) as u16;
        let record_len =
            8 + 8 + 1 + 8 + 1 + 2 + 2 + scope_len as usize + msg_len as usize + fields_len as usize;
        if idx.saturating_add(record_len) > buf.len() {
            // Skip records that don't fit.
            continue;
        }
        write_u64(rec.record_id.0, &mut buf, &mut idx);
        write_u64(rec.timestamp_nsec.0, &mut buf, &mut idx);
        write_u8(encode_level(rec.level), &mut buf, &mut idx);
        write_u64(rec.service_id, &mut buf, &mut idx);
        write_u8(scope_len as u8, &mut buf, &mut idx);
        write_u16(msg_len, &mut buf, &mut idx);
        write_u16(fields_len, &mut buf, &mut idx);
        write_bytes(&scope[..scope_len as usize], &mut buf, &mut idx);
        write_bytes(&msg[..msg_len as usize], &mut buf, &mut idx);
        write_bytes(&fields[..fields_len as usize], &mut buf, &mut idx);
        count = count.saturating_add(1);
        if count == u16::MAX {
            break;
        }
    }

    // Write count into reserved slot.
    if count_pos + 1 < buf.len() {
        let count_bytes = count.to_le_bytes();
        buf[count_pos] = count_bytes[0];
        buf[count_pos + 1] = count_bytes[1];
    }

    BoundedFrame { buf, len: core::cmp::min(idx, buf.len()) }
}

/// v2 variant of `encode_query_response_bounded_iter` that inserts a nonce after the status byte.
pub fn encode_query_response_bounded_iter_v2(
    status: u8,
    nonce: u64,
    stats: JournalStats,
    journal: &Journal,
    since: TimestampNsec,
    max_count: u16,
) -> BoundedFrame {
    let mut buf = [0u8; QUERY_BOUNDED_CAP];
    let mut idx = 0usize;

    let write_u8 = |value: u8, out: &mut [u8], pos: &mut usize| {
        if *pos < out.len() {
            out[*pos] = value;
        }
        *pos += 1;
    };
    let write_u16 = |value: u16, out: &mut [u8], pos: &mut usize| {
        let bytes = value.to_le_bytes();
        for b in bytes {
            write_u8(b, out, pos);
        }
    };
    let write_u64 = |value: u64, out: &mut [u8], pos: &mut usize| {
        let bytes = value.to_le_bytes();
        for b in bytes {
            write_u8(b, out, pos);
        }
    };
    let write_bytes = |data: &[u8], out: &mut [u8], pos: &mut usize| {
        for b in data {
            write_u8(*b, out, pos);
        }
    };

    write_u8(MAGIC0, &mut buf, &mut idx);
    write_u8(MAGIC1, &mut buf, &mut idx);
    write_u8(VERSION_V2, &mut buf, &mut idx);
    write_u8(OP_QUERY | 0x80, &mut buf, &mut idx);
    write_u8(status, &mut buf, &mut idx);
    write_u64(nonce, &mut buf, &mut idx);
    write_u64(stats.total_records, &mut buf, &mut idx);
    write_u64(stats.dropped_records, &mut buf, &mut idx);
    let count_pos = idx;
    write_u16(0, &mut buf, &mut idx); // placeholder

    let mut count: u16 = 0;
    for rec in journal.iter_since(since) {
        if count >= max_count {
            break;
        }
        let scope = rec.scope.as_slice();
        let msg = rec.message.as_slice();
        let fields = rec.fields.as_slice();
        let scope_len = core::cmp::min(scope.len(), MAX_SCOPE_LEN) as u16;
        let msg_len = core::cmp::min(msg.len(), MAX_MSG_LEN) as u16;
        let fields_len = core::cmp::min(fields.len(), MAX_FIELDS_LEN) as u16;
        let record_len =
            8 + 8 + 1 + 8 + 1 + 2 + 2 + scope_len as usize + msg_len as usize + fields_len as usize;
        if idx.saturating_add(record_len) > buf.len() {
            continue;
        }
        write_u64(rec.record_id.0, &mut buf, &mut idx);
        write_u64(rec.timestamp_nsec.0, &mut buf, &mut idx);
        write_u8(encode_level(rec.level), &mut buf, &mut idx);
        write_u64(rec.service_id, &mut buf, &mut idx);
        write_u8(scope_len as u8, &mut buf, &mut idx);
        write_u16(msg_len, &mut buf, &mut idx);
        write_u16(fields_len, &mut buf, &mut idx);
        write_bytes(&scope[..scope_len as usize], &mut buf, &mut idx);
        write_bytes(&msg[..msg_len as usize], &mut buf, &mut idx);
        write_bytes(&fields[..fields_len as usize], &mut buf, &mut idx);
        count = count.saturating_add(1);
        if count == u16::MAX {
            break;
        }
    }

    if count_pos + 1 < buf.len() {
        let count_bytes = count.to_le_bytes();
        buf[count_pos] = count_bytes[0];
        buf[count_pos + 1] = count_bytes[1];
    }

    BoundedFrame { buf, len: core::cmp::min(idx, buf.len()) }
}

fn encode_record(out: &mut Vec<u8>, rec: &LogRecord) {
    // [record_id:u64, ts:u64, level:u8, service_id:u64, scope_len:u8, msg_len:u16, fields_len:u16, scope, msg, fields]
    let scope = rec.scope.as_slice();
    let msg = rec.message.as_slice();
    let fields = rec.fields.as_slice();
    let scope_len = core::cmp::min(scope.len(), MAX_SCOPE_LEN) as u8;
    let msg_len = core::cmp::min(msg.len(), MAX_MSG_LEN) as u16;
    let fields_len = core::cmp::min(fields.len(), MAX_FIELDS_LEN) as u16;
    out.extend_from_slice(&rec.record_id.0.to_le_bytes());
    out.extend_from_slice(&rec.timestamp_nsec.0.to_le_bytes());
    out.push(encode_level(rec.level));
    out.extend_from_slice(&rec.service_id.to_le_bytes());
    out.push(scope_len);
    out.extend_from_slice(&msg_len.to_le_bytes());
    out.extend_from_slice(&fields_len.to_le_bytes());
    out.extend_from_slice(&scope[..scope_len as usize]);
    out.extend_from_slice(&msg[..msg_len as usize]);
    out.extend_from_slice(&fields[..fields_len as usize]);
}
