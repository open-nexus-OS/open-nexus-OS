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
    decode_request, encode_append_response, encode_append_response_v2, encode_query_response_bounded_iter,
    encode_query_response_bounded_iter_v2, encode_stats_response, encode_stats_response_v2, DecodeError,
    STATUS_MALFORMED, STATUS_OK, STATUS_TOO_LARGE, STATUS_UNSUPPORTED,
};

/// Handles one OS-lite request frame and returns the encoded response bytes.
pub fn handle_frame(
    journal: &mut Journal,
    sender_service_id: u64,
    now: TimestampNsec,
    frame: &[u8],
) -> Vec<u8> {
    let stats = journal.stats();
    let req = match decode_request(frame) {
        Ok(req) => req,
        Err(DecodeError::Malformed) => return encode_stats_response(STATUS_MALFORMED, stats),
        Err(DecodeError::Unsupported) => return encode_stats_response(STATUS_UNSUPPORTED, stats),
        Err(DecodeError::TooLarge) => return encode_stats_response(STATUS_TOO_LARGE, stats),
    };

    match req {
        crate::protocol::Request::Append(a) => {
            match journal.append(sender_service_id, now, a.level, &a.scope, &a.message, &a.fields) {
                Ok(AppendOutcome { record_id, dropped_records }) => {
                    encode_append_response(STATUS_OK, record_id, dropped_records)
                }
                Err(JournalError::TooLarge) => {
                    encode_append_response(STATUS_TOO_LARGE, RecordId(0), journal.stats().dropped_records)
                }
            }
        }
        crate::protocol::Request::AppendV2(a) => match journal.append(
            sender_service_id,
            now,
            a.level,
            &a.scope,
            &a.message,
            &a.fields,
        ) {
            Ok(AppendOutcome { record_id, dropped_records }) => {
                encode_append_response_v2(STATUS_OK, a.nonce, record_id, dropped_records)
            }
            Err(JournalError::TooLarge) => encode_append_response_v2(
                STATUS_TOO_LARGE,
                a.nonce,
                RecordId(0),
                journal.stats().dropped_records,
            ),
        },
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
        crate::protocol::Request::StatsV2(s) => encode_stats_response_v2(STATUS_OK, s.nonce, journal.stats()),
    }
}
