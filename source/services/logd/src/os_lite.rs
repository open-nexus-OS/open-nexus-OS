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

use nexus_abi::{cap_close, debug_putc, nsec, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::journal::{Journal, RecordId, TimestampNsec};
use crate::protocol::{
    encode_stats_response, MAGIC0, MAGIC1, MAX_FIELDS_LEN, MAX_MSG_LEN, MAX_SCOPE_LEN, OP_APPEND,
    OP_QUERY, OP_STATS, STATUS_MALFORMED, STATUS_OK, STATUS_TOO_LARGE, STATUS_UNSUPPORTED, VERSION,
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
const JOURNAL_ALLOC_CAP_BYTES: u32 = 24 * 1024;

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
                    let _ = KernelServer::send_on_cap(hdr.src, rsp.as_slice());
                    let _ = cap_close(hdr.src as u32);
                } else {
                    let _ = server.send(rsp.as_slice(), Wait::Blocking);
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
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN];
    let req_len = nexus_abi::routing::encode_route_get(name, &mut req)?;
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
                let (status, send_slot, recv_slot) =
                    nexus_abi::routing::decode_route_rsp(&buf[..n])?;
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
    Large(Vec<u8>),
}

impl ResponseFrame {
    fn as_slice(&self) -> &[u8] {
        match self {
            ResponseFrame::Small { buf, len } => &buf[..*len],
            ResponseFrame::Medium { buf, len } => &buf[..*len],
            ResponseFrame::Large(buf) => buf.as_slice(),
        }
    }
}

fn handle_frame(
    journal: &mut Journal,
    sender_service_id: u64,
    frame: &[u8],
    fallback_ts: &mut u64,
    last_ts: &mut u64,
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
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return ResponseFrame::Large(encode_stats_response(STATUS_MALFORMED, stats));
    }
    match frame[3] {
        OP_APPEND => match decode_append(frame) {
            Ok((level, scope, message, fields)) => {
                // Keep stored records compact to stay within the os-lite heap budget.
                const STORE_MAX_SCOPE_LEN: usize = 32;
                const STORE_MAX_MSG_LEN: usize = 128;
                const STORE_MAX_FIELDS_LEN: usize = 128;
                let scope = &scope[..core::cmp::min(scope.len(), STORE_MAX_SCOPE_LEN)];
                let message = &message[..core::cmp::min(message.len(), STORE_MAX_MSG_LEN)];
                let fields = &fields[..core::cmp::min(fields.len(), STORE_MAX_FIELDS_LEN)];
                match journal.append(sender_service_id, now, level, scope, message, fields) {
                    Ok(outcome) => encode_append_response_small(
                        STATUS_OK,
                        outcome.record_id,
                        outcome.dropped_records,
                    ),
                    Err(_) => encode_append_response_small(
                        STATUS_TOO_LARGE,
                        RecordId(0),
                        journal.stats().dropped_records,
                    ),
                }
            }
            Err(err) => encode_append_response_small(err, RecordId(0), stats.dropped_records),
        },
        OP_QUERY => match decode_query(frame) {
            Ok((since, max_count)) => {
                let records = journal.query(since, max_count);
                encode_query_response_bounded(STATUS_OK, stats, &records)
            }
            Err(err) => encode_query_response_bounded(err, stats, &[]),
        },
        OP_STATS => encode_stats_response_small(STATUS_OK, stats),
        _ => encode_stats_response_small(STATUS_UNSUPPORTED, stats),
    }
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

fn decode_append(frame: &[u8]) -> Result<(crate::journal::LogLevel, &[u8], &[u8], &[u8]), u8> {
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

fn decode_query(frame: &[u8]) -> Result<(TimestampNsec, u16), u8> {
    if frame.len() != 14 {
        return Err(STATUS_MALFORMED);
    }
    let since = u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]);
    let max_count = u16::from_le_bytes([frame[12], frame[13]]);
    Ok((TimestampNsec(since), max_count))
}

fn encode_level(level: crate::journal::LogLevel) -> u8 {
    match level {
        crate::journal::LogLevel::Error => 0,
        crate::journal::LogLevel::Warn => 1,
        crate::journal::LogLevel::Info => 2,
        crate::journal::LogLevel::Debug => 3,
        crate::journal::LogLevel::Trace => 4,
    }
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

fn encode_query_response_bounded(
    status: u8,
    stats: crate::journal::JournalStats,
    records: &[crate::journal::LogRecord],
) -> ResponseFrame {
    const BUF_CAP: usize = 512;
    let mut buf = [0u8; BUF_CAP];
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
    for rec in records.iter() {
        let scope_len = core::cmp::min(rec.scope.len(), MAX_SCOPE_LEN) as u16;
        let msg_len = core::cmp::min(rec.message.len(), MAX_MSG_LEN) as u16;
        let fields_len = core::cmp::min(rec.fields.len(), MAX_FIELDS_LEN) as u16;
        let record_len =
            8 + 8 + 1 + 8 + 1 + 2 + 2 + scope_len as usize + msg_len as usize + fields_len as usize;
        if idx.saturating_add(record_len) > buf.len() {
            break;
        }
        write_u64(rec.record_id.0, &mut buf, &mut idx);
        write_u64(rec.timestamp_nsec.0, &mut buf, &mut idx);
        write_u8(encode_level(rec.level), &mut buf, &mut idx);
        write_u64(rec.service_id, &mut buf, &mut idx);
        write_u8(scope_len as u8, &mut buf, &mut idx);
        write_u16(msg_len, &mut buf, &mut idx);
        write_u16(fields_len, &mut buf, &mut idx);
        write_bytes(&rec.scope[..scope_len as usize], &mut buf, &mut idx);
        write_bytes(&rec.message[..msg_len as usize], &mut buf, &mut idx);
        write_bytes(&rec.fields[..fields_len as usize], &mut buf, &mut idx);
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

    let len = core::cmp::min(idx, buf.len());
    ResponseFrame::Medium { buf, len }
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}
