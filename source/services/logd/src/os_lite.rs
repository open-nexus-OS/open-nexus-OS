// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: logd os-lite backend (kernel IPC server; byte-frame protocol v1+v2)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;
use nexus_abi::{cap_close, debug_putc, nsec, service_id_from_name, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::journal::{Journal, RecordId, TimestampNsec};
use crate::protocol::{
    encode_query_response_bounded_iter as encode_query_response_bounded_iter_proto,
    encode_query_response_bounded_iter_v2 as encode_query_response_bounded_iter_proto_v2,
    BoundedFrame, MAGIC0, MAGIC1, MAX_FIELDS_LEN, MAX_MSG_LEN, MAX_SCOPE_LEN, OP_APPEND, OP_QUERY,
    OP_STATS, STATUS_INVALID_ARGS, STATUS_OK, STATUS_OVER_LIMIT, STATUS_RATE_LIMITED, STATUS_MALFORMED,
    STATUS_TOO_LARGE, STATUS_UNSUPPORTED, VERSION, VERSION_V2,
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

const JOURNAL_CAP_RECORDS: u32 = 128;
const JOURNAL_CAP_BYTES: u32 = 16 * 1024;
// Bump allocator budget for logd heap allocations. Must cover bring-up + selftests without
// exhausting the service heap (see `alloc-fail svc=logd` diagnostics).
const JOURNAL_ALLOC_CAP_BYTES: u32 = 256 * 1024;

/// Main logd bring-up service loop (os-lite).
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    let server = match route_logd_blocking() {
        Some(server) => server,
        None => {
            emit_line("logd: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| ServerError::Unsupported)?
        }
    };
    notifier.notify();
    // Emit only after the IPC endpoint exists.
    emit_line("logd: ready");
    let _ = yield_();
    emit_line("logd: ready");

    let mut journal = Journal::new_with_alloc_cap(
        JOURNAL_CAP_RECORDS,
        JOURNAL_CAP_BYTES,
        JOURNAL_ALLOC_CAP_BYTES,
    );
    // If the kernel time source is unavailable (or too coarse), fall back to a deterministic, strictly
    // monotonic counter. This enables bounded pagination in tests without relying on wall-clock.
    let mut fallback_ts: u64 = 0;
    let mut last_ts: u64 = 0;
    let mut saw_any_rx = false;
    let mut saw_drop_nonself = false;
    let mut saw_allow_selftest = false;
    let mut saw_selftest_append = false;
    let mut saw_selftest_query = false;
    let mut saw_any_append = false;
    let mut saw_any_append_rsp = false;
    let mut saw_reject_invalid_args = false;
    let mut saw_reject_over_limit = false;
    let mut saw_reject_rate_limited = false;
    let mut rate_limiter = crate::security::SenderRateLimiter::new();
    let selftest_sid = service_id_from_name(b"selftest-client");
    loop {
        let mut inbuf = [0u8; 512];
        match server.recv_request_with_meta_into(Wait::Blocking, &mut inbuf) {
            Ok((n, sender_service_id, reply)) => {
                let frame = &inbuf[..n];
                if !saw_any_rx {
                    emit_line("logd: rx first");
                    saw_any_rx = true;
                }
                let rsp = handle_frame(
                    &mut journal,
                    sender_service_id,
                    frame,
                    &mut fallback_ts,
                    &mut last_ts,
                    &mut rate_limiter,
                );
                if frame.get(3).copied().unwrap_or(0) == OP_APPEND {
                    if let Some(status) = append_response_status(&rsp) {
                        match status {
                            STATUS_INVALID_ARGS if !saw_reject_invalid_args => {
                                emit_line("logd: reject invalid_args");
                                saw_reject_invalid_args = true;
                            }
                            STATUS_OVER_LIMIT if !saw_reject_over_limit => {
                                emit_line("logd: reject over_limit");
                                saw_reject_over_limit = true;
                            }
                            STATUS_RATE_LIMITED if !saw_reject_rate_limited => {
                                emit_line("logd: reject rate_limited");
                                saw_reject_rate_limited = true;
                            }
                            _ => {}
                        }
                    }
                }
                if !saw_any_append && frame.get(3).copied().unwrap_or(0) == OP_APPEND {
                    emit_line("logd: append rx");
                    saw_any_append = true;
                }
                if !saw_any_append_rsp && frame.get(3).copied().unwrap_or(0) == OP_APPEND {
                    if let Some(status) = rsp.as_slice().get(4).copied() {
                        emit_line_no_nl("logd: append rsp status=0x");
                        emit_hex_u8(status);
                        emit_line("");
                        saw_any_append_rsp = true;
                    }
                }
                if reply.is_none() && sender_service_id == selftest_sid {
                    let op = frame.get(3).copied().unwrap_or(0);
                    if op == OP_APPEND && !saw_selftest_append {
                        emit_line("logd: selftest append rx");
                        saw_selftest_append = true;
                    } else if op == OP_QUERY && !saw_selftest_query {
                        emit_line("logd: selftest query rx");
                        let stats = journal.stats();
                        emit_line_no_nl("logd: used_records=0x");
                        emit_hex_u64(stats.used_records as u64);
                        emit_line("");
                        let rsp_bytes = rsp.as_slice();
                        if rsp_bytes.len() >= 23 {
                            let count = u16::from_le_bytes([rsp_bytes[21], rsp_bytes[22]]);
                            emit_line_no_nl("logd: query rsp count=0x");
                            emit_hex_u8((count >> 8) as u8);
                            emit_hex_u8((count & 0xff) as u8);
                            emit_line("");
                        }
                        saw_selftest_query = true;
                    }
                }
                // If a reply cap was moved, reply on it and close it.
                if let Some(reply) = reply {
                    let clock = nexus_ipc::budget::OsClock;
                    let cap_slot = reply.slot();
                    // CAP_MOVE replies are critical control-plane signals (audit + crash reports).
                    // Use deterministic non-blocking retries with a generous explicit time budget
                    // rather than relying on kernel timeout semantics.
                    let deadline_ns = match nexus_ipc::budget::deadline_after(
                        &clock,
                        core::time::Duration::from_secs(15),
                    ) {
                        Ok(v) => v,
                        Err(_) => u64::MAX,
                    };
                    let sent = nexus_ipc::budget::retry_ipc_until(&clock, deadline_ns, || {
                        KernelServer::send_on_cap_wait(cap_slot, rsp.as_slice(), Wait::NonBlocking)
                    });
                    if sent.is_err() {
                        emit_line("logd: capmove reply send fail");
                    }
                    let _ = cap_close(cap_slot as u32);
                } else {
                    // Only the selftest-client has a dedicated response channel to logd.
                    // For all other senders, require CAP_MOVE so we don't spam the selftest queue.
                    if sender_service_id == selftest_sid {
                        if !saw_allow_selftest {
                            emit_line("logd: allow selftest replies");
                            saw_allow_selftest = true;
                        }
                        let clock = nexus_ipc::budget::OsClock;
                        let deadline_ns = match nexus_ipc::budget::deadline_after(
                            &clock,
                            core::time::Duration::from_secs(2),
                        ) {
                            Ok(v) => v,
                            Err(_) => u64::MAX,
                        };
                        let sent = nexus_ipc::budget::retry_ipc_until(&clock, deadline_ns, || {
                            server.send(rsp.as_slice(), Wait::NonBlocking)
                        });
                        if sent.is_err() {
                            emit_line("logd: selftest reply send fail");
                        }
                    } else if !saw_drop_nonself {
                        // Diagnostic marker: prove what identity the kernel reports, and what op we saw.
                        let op = frame.get(3).copied().unwrap_or(0);
                        emit_line_no_nl("logd: drop nonself sid=0x");
                        emit_hex_u64(sender_service_id);
                        emit_line_no_nl(" op=0x");
                        emit_hex_u8(op);
                        emit_line("");
                        saw_drop_nonself = true;
                    }
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

fn route_logd_blocking() -> Option<KernelServer> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    let name = b"logd";
    static ROUTE_NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // Routing v1+nonce extension:
    // GET: [R,T,1,OP_ROUTE_GET, name_len, name..., nonce:u32le]
    // RSP: [R,T,1,OP_ROUTE_RSP, status, send_slot:u32le, recv_slot:u32le, nonce:u32le]
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()])?;
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
    // Bounded probing: if routing isn't available yet, fall back quickly to deterministic slots.
    for _ in 0..64 {
        // Avoid blocking IPC on the routing control plane (can deadlock under cooperative scheduling).
        if nexus_abi::ipc_send_v1(
            CTRL_SEND_SLOT,
            &hdr,
            &req[..req_len],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        )
        .is_err()
        {
            let _ = yield_();
            continue;
        }
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n == 17 {
                    let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                    if got_nonce != nonce {
                        let _ = yield_();
                        continue;
                    }
                }
                if n != 17 {
                    let _ = yield_();
                    continue;
                }
                let (status, send_slot, recv_slot) =
                    nexus_abi::routing::decode_route_rsp(&buf[..13])?;
                if status != nexus_abi::routing::STATUS_OK {
                    let _ = yield_();
                    continue;
                }
                return KernelServer::new_with_slots(recv_slot, send_slot).ok();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => {}
        }
    }
    None
}

enum ResponseFrame {
    Small { buf: [u8; 64], len: usize },
    Medium { buf: [u8; 512], len: usize },
}

impl ResponseFrame {
    fn as_slice(&self) -> &[u8] {
        match self {
            ResponseFrame::Small { buf, len } => &buf[..*len],
            ResponseFrame::Medium { buf, len } => &buf[..*len],
        }
    }
}

fn handle_frame(
    journal: &mut Journal,
    sender_service_id: u64,
    frame: &[u8],
    fallback_ts: &mut u64,
    last_ts: &mut u64,
    rate_limiter: &mut crate::security::SenderRateLimiter,
) -> ResponseFrame {
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
    let stats = journal.stats();
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return encode_stats_response_small(STATUS_MALFORMED, stats);
    }
    match frame[2] {
        VERSION => match frame[3] {
            OP_APPEND => match decode_append_v1(frame) {
                Ok((level, scope, message, fields)) => {
                    if crate::security::has_payload_identity_claim(fields) {
                        return encode_append_response_small(
                            STATUS_INVALID_ARGS,
                            RecordId(0),
                            journal.stats().dropped_records,
                        );
                    }
                    if rate_limiter.is_rate_limited(sender_service_id, now.0) {
                        return encode_append_response_small(
                            STATUS_RATE_LIMITED,
                            RecordId(0),
                            journal.stats().dropped_records,
                        );
                    }
                    match journal.append(sender_service_id, now, level, scope, message, fields) {
                        Ok(outcome) => encode_append_response_small(
                            STATUS_OK,
                            outcome.record_id,
                            outcome.dropped_records,
                        ),
                        Err(_) => encode_append_response_small(
                            STATUS_OVER_LIMIT,
                            RecordId(0),
                            journal.stats().dropped_records,
                        ),
                    }
                }
                Err(err) => encode_append_response_small(
                    map_append_decode_status(err),
                    RecordId(0),
                    stats.dropped_records,
                ),
            },
            OP_QUERY => match decode_query_v1(frame) {
                Ok((since, max_count)) => {
                    let bounded = encode_query_response_bounded_iter_proto(
                        STATUS_OK, stats, journal, since, max_count,
                    );
                    ResponseFrame::Medium { buf: bounded.buf, len: bounded.len }
                }
                Err(err) => {
                    let bounded = encode_query_response_bounded_proto_v1(err, stats);
                    ResponseFrame::Medium { buf: bounded.buf, len: bounded.len }
                }
            },
            OP_STATS => encode_stats_response_small(STATUS_OK, stats),
            _ => encode_stats_response_small(STATUS_UNSUPPORTED, stats),
        },
        VERSION_V2 => {
            let nonce = match decode_nonce_v2(frame) {
                Some(v) => v,
                None => {
                    return encode_stats_response_small_v2(STATUS_MALFORMED, 0, stats);
                }
            };
            match frame[3] {
                OP_APPEND => match decode_append_v2(frame) {
                    Ok((level, scope, message, fields)) => {
                        if crate::security::has_payload_identity_claim(fields) {
                            return encode_append_response_small_v2(
                                STATUS_INVALID_ARGS,
                                nonce,
                                RecordId(0),
                                journal.stats().dropped_records,
                            );
                        }
                        if rate_limiter.is_rate_limited(sender_service_id, now.0) {
                            return encode_append_response_small_v2(
                                STATUS_RATE_LIMITED,
                                nonce,
                                RecordId(0),
                                journal.stats().dropped_records,
                            );
                        }
                        match journal.append(sender_service_id, now, level, scope, message, fields)
                        {
                            Ok(outcome) => encode_append_response_small_v2(
                                STATUS_OK,
                                nonce,
                                outcome.record_id,
                                outcome.dropped_records,
                            ),
                            Err(_) => encode_append_response_small_v2(
                                STATUS_OVER_LIMIT,
                                nonce,
                                RecordId(0),
                                journal.stats().dropped_records,
                            ),
                        }
                    }
                    Err(err) => encode_append_response_small_v2(
                        map_append_decode_status(err),
                        nonce,
                        RecordId(0),
                        stats.dropped_records,
                    ),
                },
                OP_QUERY => match decode_query_v2(frame) {
                    Ok((since, max_count)) => {
                        let bounded = encode_query_response_bounded_iter_proto_v2(
                            STATUS_OK, nonce, stats, journal, since, max_count,
                        );
                        ResponseFrame::Medium { buf: bounded.buf, len: bounded.len }
                    }
                    Err(err) => {
                        let bounded = encode_query_response_bounded_proto_v2(err, nonce, stats);
                        ResponseFrame::Medium { buf: bounded.buf, len: bounded.len }
                    }
                },
                OP_STATS => encode_stats_response_small_v2(STATUS_OK, nonce, stats),
                _ => encode_stats_response_small_v2(STATUS_UNSUPPORTED, nonce, stats),
            }
        }
        _ => encode_stats_response_small(STATUS_UNSUPPORTED, stats),
    }
}

fn map_append_decode_status(status: u8) -> u8 {
    match status {
        STATUS_TOO_LARGE => STATUS_OVER_LIMIT,
        STATUS_MALFORMED | STATUS_UNSUPPORTED => STATUS_INVALID_ARGS,
        other => other,
    }
}

fn append_response_status(frame: &ResponseFrame) -> Option<u8> {
    let bytes = frame.as_slice();
    if bytes.len() < 5 {
        return None;
    }
    if bytes[0] != MAGIC0 || bytes[1] != MAGIC1 {
        return None;
    }
    if bytes[3] != (OP_APPEND | 0x80) {
        return None;
    }
    Some(bytes[4])
}

fn decode_level(byte: u8) -> Result<crate::journal::LogLevel, u8> {
    match byte {
        0 => Ok(crate::journal::LogLevel::Error),
        1 => Ok(crate::journal::LogLevel::Warn),
        2 => Ok(crate::journal::LogLevel::Info),
        3 => Ok(crate::journal::LogLevel::Debug),
        4 => Ok(crate::journal::LogLevel::Trace),
        _ => Err(STATUS_MALFORMED),
    }
}

fn decode_append_v1(frame: &[u8]) -> Result<(crate::journal::LogLevel, &[u8], &[u8], &[u8]), u8> {
    if frame.len() < 10 {
        return Err(STATUS_MALFORMED);
    }
    let level = decode_level(frame[4])?;
    let scope_len = frame[5] as usize;
    let msg_len = u16::from_le_bytes([frame[6], frame[7]]) as usize;
    let fields_len = u16::from_le_bytes([frame[8], frame[9]]) as usize;
    if scope_len > MAX_SCOPE_LEN || msg_len > MAX_MSG_LEN || fields_len > MAX_FIELDS_LEN {
        return Err(STATUS_TOO_LARGE);
    }
    let start = 10;
    let end_scope = start + scope_len;
    let end_msg = end_scope + msg_len;
    let end_fields = end_msg + fields_len;
    if frame.len() != end_fields {
        return Err(STATUS_MALFORMED);
    }
    Ok((level, &frame[start..end_scope], &frame[end_scope..end_msg], &frame[end_msg..end_fields]))
}

fn decode_query_v1(frame: &[u8]) -> Result<(TimestampNsec, u16), u8> {
    if frame.len() != 14 {
        return Err(STATUS_MALFORMED);
    }
    let since = u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]);
    let max_count = u16::from_le_bytes([frame[12], frame[13]]);
    Ok((TimestampNsec(since), max_count))
}

fn decode_nonce_v2(frame: &[u8]) -> Option<u64> {
    if frame.len() < 12 {
        return None;
    }
    Some(u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]))
}

fn decode_append_v2(frame: &[u8]) -> Result<(crate::journal::LogLevel, &[u8], &[u8], &[u8]), u8> {
    // [L,O,2,OP_APPEND, nonce:u64le, level:u8, scope_len:u8, msg_len:u16le, fields_len:u16le, scope, msg, fields]
    if frame.len() < 18 {
        return Err(STATUS_MALFORMED);
    }
    let level = decode_level(frame[12])?;
    let scope_len = frame[13] as usize;
    let msg_len = u16::from_le_bytes([frame[14], frame[15]]) as usize;
    let fields_len = u16::from_le_bytes([frame[16], frame[17]]) as usize;
    if scope_len > MAX_SCOPE_LEN || msg_len > MAX_MSG_LEN || fields_len > MAX_FIELDS_LEN {
        return Err(STATUS_TOO_LARGE);
    }
    let start = 18;
    let end_scope = start + scope_len;
    let end_msg = end_scope + msg_len;
    let end_fields = end_msg + fields_len;
    if frame.len() != end_fields {
        return Err(STATUS_MALFORMED);
    }
    Ok((level, &frame[start..end_scope], &frame[end_scope..end_msg], &frame[end_msg..end_fields]))
}

fn decode_query_v2(frame: &[u8]) -> Result<(TimestampNsec, u16), u8> {
    // [L,O,2,OP_QUERY, nonce:u64le, since_nsec:u64le, max_count:u16le]
    if frame.len() != 22 {
        return Err(STATUS_MALFORMED);
    }
    let since = u64::from_le_bytes([
        frame[12], frame[13], frame[14], frame[15], frame[16], frame[17], frame[18], frame[19],
    ]);
    let max_count = u16::from_le_bytes([frame[20], frame[21]]);
    Ok((TimestampNsec(since), max_count))
}

fn encode_append_response_small(status: u8, record_id: RecordId, dropped: u64) -> ResponseFrame {
    let mut buf = [0u8; 64];
    buf[0] = MAGIC0;
    buf[1] = MAGIC1;
    buf[2] = VERSION;
    buf[3] = OP_APPEND | 0x80;
    buf[4] = status;
    buf[5..13].copy_from_slice(&record_id.0.to_le_bytes());
    buf[13..21].copy_from_slice(&dropped.to_le_bytes());
    ResponseFrame::Small { buf, len: 21 }
}

fn encode_append_response_small_v2(
    status: u8,
    nonce: u64,
    record_id: RecordId,
    dropped: u64,
) -> ResponseFrame {
    let mut buf = [0u8; 64];
    buf[0] = MAGIC0;
    buf[1] = MAGIC1;
    buf[2] = VERSION_V2;
    buf[3] = OP_APPEND | 0x80;
    buf[4] = status;
    buf[5..13].copy_from_slice(&nonce.to_le_bytes());
    buf[13..21].copy_from_slice(&record_id.0.to_le_bytes());
    buf[21..29].copy_from_slice(&dropped.to_le_bytes());
    ResponseFrame::Small { buf, len: 29 }
}

fn encode_stats_response_small(status: u8, stats: crate::journal::JournalStats) -> ResponseFrame {
    let mut buf = [0u8; 64];
    buf[0] = MAGIC0;
    buf[1] = MAGIC1;
    buf[2] = VERSION;
    buf[3] = OP_STATS | 0x80;
    buf[4] = status;
    buf[5..13].copy_from_slice(&stats.total_records.to_le_bytes());
    buf[13..21].copy_from_slice(&stats.dropped_records.to_le_bytes());
    buf[21..25].copy_from_slice(&stats.capacity_records.to_le_bytes());
    buf[25..29].copy_from_slice(&stats.capacity_bytes.to_le_bytes());
    buf[29..33].copy_from_slice(&stats.used_records.to_le_bytes());
    buf[33..37].copy_from_slice(&stats.used_bytes.to_le_bytes());
    ResponseFrame::Small { buf, len: 37 }
}

fn encode_stats_response_small_v2(
    status: u8,
    nonce: u64,
    stats: crate::journal::JournalStats,
) -> ResponseFrame {
    let mut buf = [0u8; 64];
    buf[0] = MAGIC0;
    buf[1] = MAGIC1;
    buf[2] = VERSION_V2;
    buf[3] = OP_STATS | 0x80;
    buf[4] = status;
    buf[5..13].copy_from_slice(&nonce.to_le_bytes());
    buf[13..21].copy_from_slice(&stats.total_records.to_le_bytes());
    buf[21..29].copy_from_slice(&stats.dropped_records.to_le_bytes());
    buf[29..33].copy_from_slice(&stats.capacity_records.to_le_bytes());
    buf[33..37].copy_from_slice(&stats.capacity_bytes.to_le_bytes());
    buf[37..41].copy_from_slice(&stats.used_records.to_le_bytes());
    buf[41..45].copy_from_slice(&stats.used_bytes.to_le_bytes());
    ResponseFrame::Small { buf, len: 45 }
}

fn encode_query_response_bounded_proto_v1(
    status: u8,
    stats: crate::journal::JournalStats,
) -> BoundedFrame {
    // This os-lite backend only uses the iterator-based encoder for determinism. For malformed
    // query requests we return a bounded, empty-record response via a temporary journal view.
    //
    // NOTE: keep a minimal record list encoding here (bounded, empty).
    let mut buf = [0u8; crate::protocol::QUERY_BOUNDED_CAP];
    let mut idx = 0usize;
    buf[idx] = MAGIC0;
    idx += 1;
    buf[idx] = MAGIC1;
    idx += 1;
    buf[idx] = VERSION;
    idx += 1;
    buf[idx] = OP_QUERY | 0x80;
    idx += 1;
    buf[idx] = status;
    idx += 1;
    buf[idx..idx + 8].copy_from_slice(&stats.total_records.to_le_bytes());
    idx += 8;
    buf[idx..idx + 8].copy_from_slice(&stats.dropped_records.to_le_bytes());
    idx += 8;
    // count = 0
    buf[idx..idx + 2].copy_from_slice(&0u16.to_le_bytes());
    idx += 2;
    BoundedFrame { buf, len: idx }
}

fn encode_query_response_bounded_proto_v2(
    status: u8,
    nonce: u64,
    stats: crate::journal::JournalStats,
) -> BoundedFrame {
    let mut buf = [0u8; crate::protocol::QUERY_BOUNDED_CAP];
    let mut idx = 0usize;
    buf[idx] = MAGIC0;
    idx += 1;
    buf[idx] = MAGIC1;
    idx += 1;
    buf[idx] = VERSION_V2;
    idx += 1;
    buf[idx] = OP_QUERY | 0x80;
    idx += 1;
    buf[idx] = status;
    idx += 1;
    buf[idx..idx + 8].copy_from_slice(&nonce.to_le_bytes());
    idx += 8;
    buf[idx..idx + 8].copy_from_slice(&stats.total_records.to_le_bytes());
    idx += 8;
    buf[idx..idx + 8].copy_from_slice(&stats.dropped_records.to_le_bytes());
    idx += 8;
    buf[idx..idx + 2].copy_from_slice(&0u16.to_le_bytes());
    idx += 2;
    BoundedFrame { buf, len: idx }
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

fn emit_line_no_nl(message: &str) {
    for byte in message.as_bytes().iter().copied() {
        let _ = debug_putc(byte);
    }
}

fn emit_hex_u8(value: u8) {
    fn hex(n: u8) -> u8 {
        if n < 10 {
            b'0' + n
        } else {
            b'a' + (n - 10)
        }
    }
    let _ = debug_putc(hex((value >> 4) & 0x0f));
    let _ = debug_putc(hex(value & 0x0f));
}

fn emit_hex_u64(value: u64) {
    fn hex(n: u8) -> u8 {
        if n < 10 {
            b'0' + n
        } else {
            b'a' + (n - 10)
        }
    }
    // Print fixed-width 16 hex digits for stable parsing.
    for shift in (0..16).rev() {
        let nib = ((value >> (shift * 4)) & 0x0f) as u8;
        let _ = debug_putc(hex(nib));
    }
}
