// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: logd OS-lite wire protocol v1+v2 helpers (host-testable parser utilities).
//!
//! This module intentionally mirrors the byte-level framing in `source/services/logd/src/protocol.rs`
//! without depending on the logd service crate. The helpers are pure functions over `&[u8]` so
//! host tests can validate paging / bounds logic deterministically.
//!
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (used by selftest + host tests)
//! TEST_COVERAGE: Unit tests (host)
//!
//! PUBLIC API:
//!   - `parse_stats_response_prefix`
//!   - `scan_query_page`
//!   - `QueryRecordIter`
//!
//! INVARIANTS:
//!   - Never panics on malformed/truncated input
//!   - Bounded record field sizes (scope/message/fields) per v1/v2 limits (same caps)
//!   - No allocations

#![forbid(unsafe_code)]

use core::cmp;

/// Frame magic (byte 0).
pub const MAGIC0: u8 = b'L';
/// Frame magic (byte 1).
pub const MAGIC1: u8 = b'O';
/// Protocol version.
pub const VERSION: u8 = 1;
/// Protocol version v2 (nonce-correlated).
pub const VERSION_V2: u8 = 2;

/// logd opcode: APPEND.
pub const OP_APPEND: u8 = 1;
/// logd opcode: QUERY.
pub const OP_QUERY: u8 = 2;
/// logd opcode: STATS.
pub const OP_STATS: u8 = 3;

/// Status: OK.
pub const STATUS_OK: u8 = 0;

/// Maximum scope length in bytes.
pub const MAX_SCOPE_LEN: usize = 64;
/// Maximum message length in bytes.
pub const MAX_MSG_LEN: usize = 256;
/// Maximum fields length in bytes.
pub const MAX_FIELDS_LEN: usize = 512;

/// Errors when decoding logd wire frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// Buffer is too short for the expected structure.
    Truncated,
    /// Magic bytes were incorrect.
    BadMagic,
    /// Unsupported protocol version.
    BadVersion,
    /// Unexpected opcode in response.
    BadOpcode,
    /// Response status was not `STATUS_OK`.
    BadStatus(u8),
    /// Response nonce did not match the expected value.
    BadNonce {
        /// Nonce the caller was waiting for.
        expected: u64,
        /// Nonce observed in the decoded response frame.
        got: u64,
    },
    /// Field lengths exceeded the v1 caps.
    TooLarge,
}

/// Parses an APPEND response and returns its status byte.
///
/// Frame shape: `[L,O,ver,OP_APPEND|0x80, status]`
pub fn parse_append_response_status(buf: &[u8]) -> Result<u8, WireError> {
    if buf.len() < 5 {
        return Err(WireError::Truncated);
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 {
        return Err(WireError::BadMagic);
    }
    if buf[2] != VERSION {
        return Err(WireError::BadVersion);
    }
    if buf[3] != (OP_APPEND | 0x80) {
        return Err(WireError::BadOpcode);
    }
    Ok(buf[4])
}

/// Parses a v2 APPEND response and returns `(status, nonce)`.
///
/// Frame shape (prefix): `[L,O,2,OP_APPEND|0x80, status, nonce:u64le, ...]`
pub fn parse_append_response_v2_prefix(buf: &[u8]) -> Result<(u8, u64), WireError> {
    if buf.len() < 4 + 1 + 8 {
        return Err(WireError::Truncated);
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 {
        return Err(WireError::BadMagic);
    }
    if buf[2] != VERSION_V2 {
        return Err(WireError::BadVersion);
    }
    if buf[3] != (OP_APPEND | 0x80) {
        return Err(WireError::BadOpcode);
    }
    let status = buf[4];
    let nonce = u64::from_le_bytes([
        buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12],
    ]);
    Ok((status, nonce))
}

/// Extracts a nonce from a v2 logd response frame (APPEND/QUERY/STATS).
pub fn extract_nonce_v2(buf: &[u8]) -> Option<u64> {
    if buf.len() < 4 + 1 + 8 {
        return None;
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION_V2 {
        return None;
    }
    // Any v2 response has nonce immediately after status.
    Some(u64::from_le_bytes([
        buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12],
    ]))
}

/// Parsed prefix of a STATS response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsPrefix {
    /// Response status.
    pub status: u8,
    /// Total records in the journal.
    pub total_records: u64,
    /// Dropped records in the journal.
    pub dropped_records: u64,
}

/// Parses the common prefix of a logd STATS response.
///
/// Frame shape (prefix): `[L,O,ver,OP|0x80, status, total:u64le, dropped:u64le, ...]`
pub fn parse_stats_response_prefix(buf: &[u8]) -> Result<StatsPrefix, WireError> {
    if buf.len() < 4 + 1 + 8 + 8 {
        return Err(WireError::Truncated);
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 {
        return Err(WireError::BadMagic);
    }
    if buf[2] != VERSION {
        return Err(WireError::BadVersion);
    }
    if buf[3] != (OP_STATS | 0x80) {
        return Err(WireError::BadOpcode);
    }
    let status = buf[4];
    let total_records = u64::from_le_bytes([
        buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12],
    ]);
    let dropped_records = u64::from_le_bytes([
        buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20],
    ]);
    Ok(StatsPrefix { status, total_records, dropped_records })
}

/// Parses the common prefix of a logd v2 STATS response and returns `(nonce, prefix)`.
///
/// Frame shape (prefix): `[L,O,2,OP_STATS|0x80, status, nonce:u64le, total:u64le, dropped:u64le, ...]`
pub fn parse_stats_response_prefix_v2(buf: &[u8]) -> Result<(u64, StatsPrefix), WireError> {
    if buf.len() < 4 + 1 + 8 + 8 + 8 {
        return Err(WireError::Truncated);
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 {
        return Err(WireError::BadMagic);
    }
    if buf[2] != VERSION_V2 {
        return Err(WireError::BadVersion);
    }
    if buf[3] != (OP_STATS | 0x80) {
        return Err(WireError::BadOpcode);
    }
    let status = buf[4];
    let nonce = u64::from_le_bytes([
        buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12],
    ]);
    let total_records = u64::from_le_bytes([
        buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20],
    ]);
    let dropped_records = u64::from_le_bytes([
        buf[21], buf[22], buf[23], buf[24], buf[25], buf[26], buf[27], buf[28],
    ]);
    Ok((nonce, StatsPrefix { status, total_records, dropped_records }))
}

/// Parsed header of a QUERY response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryHeader {
    /// Response status.
    pub status: u8,
    /// Total records in the journal.
    pub total_records: u64,
    /// Dropped records in the journal.
    pub dropped_records: u64,
    /// Number of records encoded in this page.
    pub count: u16,
}

/// A single decoded record in a QUERY response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryRecord<'a> {
    /// Record id.
    pub record_id: u64,
    /// Timestamp (nsec).
    pub timestamp_nsec: u64,
    /// Log level (0..=4).
    pub level: u8,
    /// Kernel-derived sender service id.
    pub service_id: u64,
    /// Scope bytes (bounded).
    pub scope: &'a [u8],
    /// Message bytes (bounded).
    pub message: &'a [u8],
    /// Fields bytes (bounded).
    pub fields: &'a [u8],
}

/// Iterator over QUERY records inside a decoded response buffer.
pub struct QueryRecordIter<'a> {
    buf: &'a [u8],
    idx: usize,
    remaining: u16,
}

impl<'a> QueryRecordIter<'a> {
    fn new(buf: &'a [u8], idx: usize, remaining: u16) -> Self {
        Self { buf, idx, remaining }
    }
}

impl<'a> Iterator for QueryRecordIter<'a> {
    type Item = Result<QueryRecord<'a>, WireError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let buf = self.buf;
        let mut idx = self.idx;

        // Fixed header:
        // [record_id:u64, ts:u64, level:u8, service_id:u64, scope_len:u8, msg_len:u16, fields_len:u16, ...]
        if buf.len() < idx + 8 + 8 + 1 + 8 + 1 + 2 + 2 {
            self.remaining = 0;
            return Some(Err(WireError::Truncated));
        }
        let record_id = u64::from_le_bytes([
            buf[idx],
            buf[idx + 1],
            buf[idx + 2],
            buf[idx + 3],
            buf[idx + 4],
            buf[idx + 5],
            buf[idx + 6],
            buf[idx + 7],
        ]);
        idx += 8;
        let timestamp_nsec = u64::from_le_bytes([
            buf[idx],
            buf[idx + 1],
            buf[idx + 2],
            buf[idx + 3],
            buf[idx + 4],
            buf[idx + 5],
            buf[idx + 6],
            buf[idx + 7],
        ]);
        idx += 8;
        let level = buf[idx];
        idx += 1;
        let service_id = u64::from_le_bytes([
            buf[idx],
            buf[idx + 1],
            buf[idx + 2],
            buf[idx + 3],
            buf[idx + 4],
            buf[idx + 5],
            buf[idx + 6],
            buf[idx + 7],
        ]);
        idx += 8;
        let scope_len = buf[idx] as usize;
        idx += 1;
        let msg_len = u16::from_le_bytes([buf[idx], buf[idx + 1]]) as usize;
        idx += 2;
        let fields_len = u16::from_le_bytes([buf[idx], buf[idx + 1]]) as usize;
        idx += 2;

        if scope_len > MAX_SCOPE_LEN || msg_len > MAX_MSG_LEN || fields_len > MAX_FIELDS_LEN {
            self.remaining = 0;
            return Some(Err(WireError::TooLarge));
        }
        if buf.len() < idx + scope_len + msg_len + fields_len {
            self.remaining = 0;
            return Some(Err(WireError::Truncated));
        }
        let scope = &buf[idx..idx + scope_len];
        idx += scope_len;
        let message = &buf[idx..idx + msg_len];
        idx += msg_len;
        let fields = &buf[idx..idx + fields_len];
        idx += fields_len;

        self.idx = idx;
        self.remaining -= 1;
        Some(Ok(QueryRecord { record_id, timestamp_nsec, level, service_id, scope, message, fields }))
    }
}

/// Parses the QUERY response header and returns `(header, records_iter)`.
///
/// Frame shape:
/// `[L,O,ver,OP|0x80, status, total:u64le, dropped:u64le, count:u16le, ...records]`
pub fn parse_query_response<'a>(
    buf: &'a [u8],
) -> Result<(QueryHeader, QueryRecordIter<'a>), WireError> {
    if buf.len() < 4 + 1 + 8 + 8 + 2 {
        return Err(WireError::Truncated);
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 {
        return Err(WireError::BadMagic);
    }
    if buf[2] != VERSION {
        return Err(WireError::BadVersion);
    }
    if buf[3] != (OP_QUERY | 0x80) {
        return Err(WireError::BadOpcode);
    }
    let status = buf[4];
    let total_records = u64::from_le_bytes([
        buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12],
    ]);
    let dropped_records = u64::from_le_bytes([
        buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20],
    ]);
    let count = u16::from_le_bytes([buf[21], buf[22]]);
    let hdr = QueryHeader { status, total_records, dropped_records, count };
    let iter = QueryRecordIter::new(buf, 23, count);
    Ok((hdr, iter))
}

/// Parses a v2 QUERY response and returns `(nonce, header, records_iter)`.
///
/// Frame shape:
/// `[L,O,2,OP_QUERY|0x80, status, nonce:u64le, total:u64le, dropped:u64le, count:u16le, ...records]`
pub fn parse_query_response_v2<'a>(
    buf: &'a [u8],
) -> Result<(u64, QueryHeader, QueryRecordIter<'a>), WireError> {
    if buf.len() < 4 + 1 + 8 + 8 + 8 + 2 {
        return Err(WireError::Truncated);
    }
    if buf[0] != MAGIC0 || buf[1] != MAGIC1 {
        return Err(WireError::BadMagic);
    }
    if buf[2] != VERSION_V2 {
        return Err(WireError::BadVersion);
    }
    if buf[3] != (OP_QUERY | 0x80) {
        return Err(WireError::BadOpcode);
    }
    let status = buf[4];
    let nonce = u64::from_le_bytes([
        buf[5], buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12],
    ]);
    let total_records = u64::from_le_bytes([
        buf[13], buf[14], buf[15], buf[16], buf[17], buf[18], buf[19], buf[20],
    ]);
    let dropped_records = u64::from_le_bytes([
        buf[21], buf[22], buf[23], buf[24], buf[25], buf[26], buf[27], buf[28],
    ]);
    let count = u16::from_le_bytes([buf[29], buf[30]]);
    let hdr = QueryHeader { status, total_records, dropped_records, count };
    let iter = QueryRecordIter::new(buf, 31, count);
    Ok((nonce, hdr, iter))
}

/// Result of scanning a single QUERY page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryPageScan {
    /// Number of records in the page (declared count).
    pub count: u16,
    /// Whether `needle` appeared in any record's scope/message/fields.
    pub found: bool,
    /// Maximum timestamp observed in this page (0 if none).
    pub max_timestamp_nsec: u64,
}

/// Scans a single QUERY response page for `needle`.
///
/// This is a convenience wrapper used by paging logic (update `since_nsec = max_ts + 1`).
pub fn scan_query_page(buf: &[u8], needle: &[u8]) -> Result<QueryPageScan, WireError> {
    let (hdr, mut it) = parse_query_response(buf)?;
    if hdr.status != STATUS_OK {
        return Err(WireError::BadStatus(hdr.status));
    }
    let mut found = false;
    let mut max_ts: u64 = 0;
    while let Some(rec) = it.next() {
        let rec = rec?;
        max_ts = cmp::max(max_ts, rec.timestamp_nsec);
        if !needle.is_empty() && !found {
            let n = needle.len();
            if rec.scope.windows(n).any(|w| w == needle)
                || rec.message.windows(n).any(|w| w == needle)
                || rec.fields.windows(n).any(|w| w == needle)
            {
                found = true;
            }
        }
    }
    Ok(QueryPageScan { count: hdr.count, found, max_timestamp_nsec: max_ts })
}

/// Scans a single v2 QUERY response page for `needle` and validates the nonce.
pub fn scan_query_page_v2(
    buf: &[u8],
    expected_nonce: u64,
    needle: &[u8],
) -> Result<QueryPageScan, WireError> {
    let (nonce, hdr, mut it) = parse_query_response_v2(buf)?;
    if nonce != expected_nonce {
        return Err(WireError::BadNonce { expected: expected_nonce, got: nonce });
    }
    if hdr.status != STATUS_OK {
        return Err(WireError::BadStatus(hdr.status));
    }
    let mut found = false;
    let mut max_ts: u64 = 0;
    while let Some(rec) = it.next() {
        let rec = rec?;
        max_ts = cmp::max(max_ts, rec.timestamp_nsec);
        if !needle.is_empty() && !found {
            let n = needle.len();
            if rec.scope.windows(n).any(|w| w == needle)
                || rec.message.windows(n).any(|w| w == needle)
                || rec.fields.windows(n).any(|w| w == needle)
            {
                found = true;
            }
        }
    }
    Ok(QueryPageScan { count: hdr.count, found, max_timestamp_nsec: max_ts })
}

/// Computes the next `since_nsec` value for a paged query.
pub fn next_since_nsec(prev_since_nsec: u64, max_timestamp_nsec: u64) -> Option<u64> {
    if max_timestamp_nsec == 0 || max_timestamp_nsec <= prev_since_nsec {
        None
    } else {
        Some(max_timestamp_nsec.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn push_record(
        out: &mut Vec<u8>,
        record_id: u64,
        ts: u64,
        level: u8,
        service_id: u64,
        scope: &[u8],
        msg: &[u8],
        fields: &[u8],
    ) {
        out.extend_from_slice(&record_id.to_le_bytes());
        out.extend_from_slice(&ts.to_le_bytes());
        out.push(level);
        out.extend_from_slice(&service_id.to_le_bytes());
        out.push(scope.len() as u8);
        out.extend_from_slice(&(msg.len() as u16).to_le_bytes());
        out.extend_from_slice(&(fields.len() as u16).to_le_bytes());
        out.extend_from_slice(scope);
        out.extend_from_slice(msg);
        out.extend_from_slice(fields);
    }

    #[test]
    fn stats_prefix_parses() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_STATS | 0x80, STATUS_OK]);
        buf.extend_from_slice(&123u64.to_le_bytes());
        buf.extend_from_slice(&7u64.to_le_bytes());
        let p = parse_stats_response_prefix(&buf).unwrap();
        assert_eq!(p.status, STATUS_OK);
        assert_eq!(p.total_records, 123);
        assert_eq!(p.dropped_records, 7);
    }

    #[test]
    fn append_response_parses_status() {
        let ok = [MAGIC0, MAGIC1, VERSION, OP_APPEND | 0x80, STATUS_OK];
        assert_eq!(parse_append_response_status(&ok).unwrap(), STATUS_OK);

        let bad = [MAGIC0, MAGIC1, VERSION, OP_QUERY | 0x80, STATUS_OK];
        assert_eq!(
            parse_append_response_status(&bad).unwrap_err(),
            WireError::BadOpcode
        );
    }

    #[test]
    fn append_response_v2_prefix_parses_status_and_nonce() {
        let mut ok = Vec::new();
        ok.extend_from_slice(&[MAGIC0, MAGIC1, VERSION_V2, OP_APPEND | 0x80, STATUS_OK]);
        ok.extend_from_slice(&0x0102030405060708u64.to_le_bytes());
        ok.extend_from_slice(&[0u8; 16]); // record_id + dropped (ignored by prefix parser)
        let (status, nonce) = parse_append_response_v2_prefix(&ok).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(nonce, 0x0102030405060708);
        assert_eq!(extract_nonce_v2(&ok), Some(0x0102030405060708));
    }

    #[test]
    fn query_scan_finds_needle_and_advances_since() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_QUERY | 0x80, STATUS_OK]);
        buf.extend_from_slice(&10u64.to_le_bytes()); // total
        buf.extend_from_slice(&0u64.to_le_bytes()); // dropped
        buf.extend_from_slice(&2u16.to_le_bytes()); // count
        push_record(
            &mut buf,
            1,
            100,
            2,
            0xAA,
            b"selftest",
            b"hello world",
            b"",
        );
        push_record(
            &mut buf,
            2,
            150,
            2,
            0xBB,
            b"other",
            b"msg",
            b"fields contain needle",
        );

        let scan = scan_query_page(&buf, b"needle").unwrap();
        assert_eq!(scan.count, 2);
        assert!(scan.found);
        assert_eq!(scan.max_timestamp_nsec, 150);
        assert_eq!(next_since_nsec(0, scan.max_timestamp_nsec), Some(151));
    }

    #[test]
    fn query_scan_rejects_truncated_record() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_QUERY | 0x80, STATUS_OK]);
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        // record header only, missing body
        buf.extend_from_slice(&1u64.to_le_bytes());
        buf.extend_from_slice(&2u64.to_le_bytes());
        buf.push(2);
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.push(3);
        buf.extend_from_slice(&5u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());

        assert_eq!(scan_query_page(&buf, b"x").unwrap_err(), WireError::Truncated);
    }
}
