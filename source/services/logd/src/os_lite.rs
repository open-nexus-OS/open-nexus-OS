// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: logd os-lite backend (kernel IPC server; byte-frame protocol v1)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{cap_close, debug_println, nsec, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::journal::{Journal, RecordId, TimestampNsec};
use crate::protocol::{
    decode_request, encode_append_response, encode_query_response, encode_stats_response,
    DecodeError, Request, STATUS_MALFORMED, STATUS_OK, STATUS_TOO_LARGE, STATUS_UNSUPPORTED,
};

/// Result alias surfaced by the lite logd backend.
pub type LiteResult<T> = core::result::Result<T, ServerError>;

/// Ready notifier invoked when logd startup finishes.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Errors surfaced by the lite logd backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "logd errors must be handled"]
pub enum ServerError {
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "logd unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

const JOURNAL_CAP_RECORDS: u32 = 256;
const JOURNAL_CAP_BYTES: u32 = 64 * 1024;

/// Main logd bring-up service loop (os-lite).
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    let server = KernelServer::new_for("logd").map_err(|_| ServerError::Unsupported)?;
    notifier.notify();
    // Marker contract (RFC-0003 / scripts/qemu-test.sh): emit only after the IPC endpoint exists.
    let _ = debug_println("logd: ready");

    let mut journal = Journal::new(JOURNAL_CAP_RECORDS, JOURNAL_CAP_BYTES);
    // If the kernel time source is unavailable (or too coarse), fall back to a deterministic, strictly
    // monotonic counter. This enables bounded pagination in tests without relying on wall-clock.
    let mut fallback_ts: u64 = 0;
    let mut last_ts: u64 = 0;
    loop {
        match server.recv_with_header_meta(Wait::Blocking) {
            Ok((hdr, sender_service_id, frame)) => {
                let rsp = handle_frame(
                    &mut journal,
                    sender_service_id,
                    frame.as_slice(),
                    &mut fallback_ts,
                    &mut last_ts,
                );
                // If a reply cap was moved, reply on it and close it.
                if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
                    let _ = KernelServer::send_on_cap(hdr.src, &rsp);
                    let _ = cap_close(hdr.src as u32);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }
    }
}

fn handle_frame(
    journal: &mut Journal,
    sender_service_id: u64,
    frame: &[u8],
    fallback_ts: &mut u64,
    last_ts: &mut u64,
) -> Vec<u8> {
    let candidate = match nsec() {
        Ok(value) => value,
        Err(_) => {
            *fallback_ts = fallback_ts.saturating_add(1);
            *fallback_ts
        }
    };
    // Enforce strict monotonicity to avoid pagination gaps when time is coarse.
    let ts = if candidate <= *last_ts {
        *last_ts = last_ts.saturating_add(1);
        *last_ts
    } else {
        *last_ts = candidate;
        candidate
    };
    let now = TimestampNsec(ts);
    match decode_request(frame) {
        Ok(Request::Append(req)) => match journal.append(
            sender_service_id,
            now,
            req.level,
            &req.scope,
            &req.message,
            &req.fields,
        ) {
            Ok(outcome) => {
                encode_append_response(STATUS_OK, outcome.record_id, outcome.dropped_records)
            }
            Err(_) => encode_append_response(
                STATUS_TOO_LARGE,
                RecordId(0),
                journal.stats().dropped_records,
            ),
        },
        Ok(Request::Query(req)) => {
            let stats = journal.stats();
            let records = journal.query(req.since_nsec, req.max_count);
            encode_query_response(STATUS_OK, stats, &records)
        }
        Ok(Request::Stats(_)) => {
            let stats = journal.stats();
            encode_stats_response(STATUS_OK, stats)
        }
        Err(err) => {
            // Best-effort: if the caller gave us a versioned header, respond on the same op.
            // Otherwise fall back to STATS response shape (clients treat status!=OK as failure).
            let stats = journal.stats();
            let status = match err {
                DecodeError::Malformed => STATUS_MALFORMED,
                DecodeError::Unsupported => STATUS_UNSUPPORTED,
                DecodeError::TooLarge => STATUS_TOO_LARGE,
            };
            if frame.len() >= 4
                && frame[0] == crate::protocol::MAGIC0
                && frame[1] == crate::protocol::MAGIC1
                && frame[2] == crate::protocol::VERSION
            {
                match frame[3] {
                    crate::protocol::OP_APPEND => {
                        encode_append_response(status, RecordId(0), stats.dropped_records)
                    }
                    crate::protocol::OP_QUERY => encode_query_response(status, stats, &[]),
                    crate::protocol::OP_STATS => encode_stats_response(status, stats),
                    _ => encode_stats_response(status, stats),
                }
            } else {
                encode_stats_response(status, stats)
            }
        }
    }
}
