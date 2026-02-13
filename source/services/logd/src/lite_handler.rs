// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: logd OS-lite request handler (host-testable, side-effect free).
//!
//! This is a deterministic, in-process version of the OS-lite `handle_frame` logic. It does not
//! perform syscalls or emit UART markers; it only parses request frames (v1+v2) and mutates a [`Journal`].
//! It exists so we can write stable host tests that cover the brittle parts (bounded query paging,
//! truncation behavior, and basic append/query/stats semantics) without relying on QEMU timing.
//!
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: Covered by `tests/journal_protocol.rs`
//!
//! INVARIANTS:
//! - Bounded parsing and responses (never panics on malformed input)
//! - QUERY responses use the fixed-size 512-byte encoding and skip records that do not fit

#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

use crate::journal::{AppendOutcome, Journal, JournalError, RecordId, TimestampNsec};
use crate::protocol::{
    decode_request, encode_append_response, encode_append_response_v2,
    encode_query_response_bounded_iter, encode_query_response_bounded_iter_v2,
    encode_stats_response, encode_stats_response_v2, DecodeError, OP_APPEND, STATUS_INVALID_ARGS,
    STATUS_OK, STATUS_OVER_LIMIT, STATUS_RATE_LIMITED, STATUS_UNSUPPORTED, VERSION_V2,
};
use crate::security::{has_payload_identity_claim, SenderRateLimiter};

/// Handles one OS-lite request frame and returns the encoded response bytes.
pub fn handle_frame(
    journal: &mut Journal,
    sender_service_id: u64,
    now: TimestampNsec,
    frame: &[u8],
) -> Vec<u8> {
    let mut limiter = SenderRateLimiter::new();
    handle_frame_with_limiter(journal, sender_service_id, now, frame, &mut limiter)
}

/// Same as [`handle_frame`] but allows caller-managed rate limiter state.
pub fn handle_frame_with_limiter(
    journal: &mut Journal,
    sender_service_id: u64,
    now: TimestampNsec,
    frame: &[u8],
    limiter: &mut SenderRateLimiter,
) -> Vec<u8> {
    let stats = journal.stats();
    let req = match decode_request(frame) {
        Ok(req) => req,
        Err(err) => {
            if frame.get(3).copied().unwrap_or(0) == OP_APPEND {
                return encode_append_error_for_decode(frame, err, stats.dropped_records);
            }
            return match err {
                DecodeError::Malformed => encode_stats_response(STATUS_INVALID_ARGS, stats),
                DecodeError::Unsupported => encode_stats_response(STATUS_UNSUPPORTED, stats),
                DecodeError::TooLarge => encode_stats_response(STATUS_OVER_LIMIT, stats),
            };
        }
    };

    match req {
        crate::protocol::Request::Append(a) => {
            if has_payload_identity_claim(&a.fields) {
                return encode_append_response(
                    STATUS_INVALID_ARGS,
                    RecordId(0),
                    journal.stats().dropped_records,
                );
            }
            if limiter.is_rate_limited(sender_service_id, now.0) {
                return encode_append_response(
                    STATUS_RATE_LIMITED,
                    RecordId(0),
                    journal.stats().dropped_records,
                );
            }
            match journal.append(sender_service_id, now, a.level, &a.scope, &a.message, &a.fields) {
                Ok(AppendOutcome { record_id, dropped_records }) => {
                    encode_append_response(STATUS_OK, record_id, dropped_records)
                }
                Err(JournalError::TooLarge) => encode_append_response(
                    STATUS_OVER_LIMIT,
                    RecordId(0),
                    journal.stats().dropped_records,
                ),
            }
        }
        crate::protocol::Request::AppendV2(a) => {
            if has_payload_identity_claim(&a.fields) {
                return encode_append_response_v2(
                    STATUS_INVALID_ARGS,
                    a.nonce,
                    RecordId(0),
                    journal.stats().dropped_records,
                );
            }
            if limiter.is_rate_limited(sender_service_id, now.0) {
                return encode_append_response_v2(
                    STATUS_RATE_LIMITED,
                    a.nonce,
                    RecordId(0),
                    journal.stats().dropped_records,
                );
            }
            match journal.append(sender_service_id, now, a.level, &a.scope, &a.message, &a.fields) {
                Ok(AppendOutcome { record_id, dropped_records }) => {
                    encode_append_response_v2(STATUS_OK, a.nonce, record_id, dropped_records)
                }
                Err(JournalError::TooLarge) => encode_append_response_v2(
                    STATUS_OVER_LIMIT,
                    a.nonce,
                    RecordId(0),
                    journal.stats().dropped_records,
                ),
            }
        }
        crate::protocol::Request::Query(q) => {
            let bounded = encode_query_response_bounded_iter(
                STATUS_OK,
                journal.stats(),
                journal,
                q.since_nsec,
                q.max_count,
            );
            bounded.as_slice().to_vec()
        }
        crate::protocol::Request::QueryV2(q) => {
            let bounded = encode_query_response_bounded_iter_v2(
                STATUS_OK,
                q.nonce,
                journal.stats(),
                journal,
                q.since_nsec,
                q.max_count,
            );
            bounded.as_slice().to_vec()
        }
        crate::protocol::Request::Stats(_) => encode_stats_response(STATUS_OK, journal.stats()),
        crate::protocol::Request::StatsV2(s) => {
            encode_stats_response_v2(STATUS_OK, s.nonce, journal.stats())
        }
    }
}

fn encode_append_error_for_decode(frame: &[u8], err: DecodeError, dropped: u64) -> Vec<u8> {
    let status = match err {
        DecodeError::Malformed => STATUS_INVALID_ARGS,
        DecodeError::Unsupported => STATUS_UNSUPPORTED,
        DecodeError::TooLarge => STATUS_OVER_LIMIT,
    };
    if frame.get(2).copied() == Some(VERSION_V2) {
        let nonce = parse_nonce(frame).unwrap_or(0);
        encode_append_response_v2(status, nonce, RecordId(0), dropped)
    } else {
        encode_append_response(status, RecordId(0), dropped)
    }
}

fn parse_nonce(frame: &[u8]) -> Option<u64> {
    if frame.len() < 12 {
        return None;
    }
    Some(u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]))
}
