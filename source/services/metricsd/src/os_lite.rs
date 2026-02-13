// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: metricsd os-lite runtime backend and bounded request handling
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker proofs via selftest-client
//! ADR: docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md
//!
//! INVARIANTS:
//! - `sender_service_id` is the sole identity source for policy/bounds decisions
//! - All reject classes emit deterministic markers once
//! - Export to logd happens via nexus-log sink-logd (bounded, best-effort)

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use nexus_abi::{debug_putc, nsec, yield_};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Server as _, Wait};
use nexus_metrics::{
    decode_request, encode_status_response, DecodeError, Request, OP_COUNTER_INC, OP_GAUGE_SET,
    OP_HIST_OBSERVE, OP_PING, OP_SPAN_END, OP_SPAN_START, STATUS_INVALID_ARGS, STATUS_NOT_FOUND,
    STATUS_OK, STATUS_OVER_LIMIT, STATUS_RATE_LIMITED,
};

use crate::{RateLimiter, Registry, RejectReason, RetentionEngine, RetentionEventKind, RuntimeLimits};

use statefs::protocol as statefs_proto;

/// Result type for metricsd service loop.
pub type MetricsResult<T> = Result<T, MetricsError>;

// Deterministic slots distributed by init-lite for metricsd:
// - statefsd send: 0x07
// - logd send: 0x08
const METRICSD_STATEFSD_SEND_SLOT: u32 = 0x07;
const METRICSD_LOGD_SEND_SLOT: u32 = 0x08;
const METRICSD_REPLY_SEND_SLOT: u32 = 0x06;
const METRICSD_REPLY_RECV_SLOT: u32 = 0x05;

/// Errors surfaced by the metricsd os-lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "metricsd errors must be handled"]
pub enum MetricsError {
    Ipc,
}

struct RetentionSink {
    limits: RuntimeLimits,
    engine: RetentionEngine,
    client: Option<KernelClient>,
    wal_proof_emitted: bool,
    rollup_10s_proof_emitted: bool,
    rollup_60s_proof_emitted: bool,
}

impl RetentionSink {
    fn new(limits: RuntimeLimits) -> Self {
        let client = if limits.retention_enabled {
            KernelClient::new_with_slots(METRICSD_STATEFSD_SEND_SLOT, 0).ok()
        } else {
            None
        };
        if limits.retention_enabled && client.is_none() {
            emit_line("metricsd: retention statefs unavailable");
        }
        Self {
            limits,
            engine: RetentionEngine::new(limits),
            client,
            wal_proof_emitted: false,
            rollup_10s_proof_emitted: false,
            rollup_60s_proof_emitted: false,
        }
    }

    fn record_metric(&mut self, record: &str) {
        self.record(RetentionEventKind::Metric, record.as_bytes());
    }

    fn record_span(&mut self, record: &str) {
        self.record(RetentionEventKind::Span, record.as_bytes());
    }

    fn record(&mut self, kind: RetentionEventKind, record: &[u8]) {
        let Some(update) = self.engine.append(kind, record) else {
            return;
        };
        let Some(client) = self.client.as_ref() else {
            return;
        };
        let retries = match kind {
            RetentionEventKind::Metric => self.limits.retention_best_effort_retries,
            RetentionEventKind::Span => self.limits.retention_critical_retries,
        };
        let key = format!("/state/observability/metricsd/wal/seg_{}", update.wal_slot);
        let wal_ok = put_with_retries(client, key.as_str(), &update.wal_bytes, retries);
        if !wal_ok {
            return;
        }
        if !self.wal_proof_emitted {
            self.wal_proof_emitted = true;
            nexus_log::info("metricsd", |line| {
                line.text("retention wal active");
            });
        }
        if let Some(rollup_10s) = update.rollup_10s.as_ref() {
            let key = format!("/state/observability/metricsd/rollup/10s/w_{}", rollup_10s.window_id);
            let _ = put_with_retries(
                client,
                key.as_str(),
                &rollup_10s.bytes,
                self.limits.retention_best_effort_retries,
            );
            if !self.rollup_10s_proof_emitted {
                self.rollup_10s_proof_emitted = true;
                nexus_log::info("metricsd", |line| {
                    line.text("retention rollup 10s active");
                });
            }
        }
        for window_id in update.gc_rollup_10s.iter().copied() {
            let key = format!("/state/observability/metricsd/rollup/10s/w_{}", window_id);
            let _ = statefs_delete_nonblocking(client, key.as_str());
        }
        if let Some(rollup_60s) = update.rollup_60s.as_ref() {
            let key = format!("/state/observability/metricsd/rollup/60s/w_{}", rollup_60s.window_id);
            let _ = put_with_retries(
                client,
                key.as_str(),
                &rollup_60s.bytes,
                self.limits.retention_critical_retries,
            );
            if !self.rollup_60s_proof_emitted {
                self.rollup_60s_proof_emitted = true;
                nexus_log::info("metricsd", |line| {
                    line.text("retention rollup 60s active");
                });
            }
        }
        for window_id in update.gc_rollup_60s.iter().copied() {
            let key = format!("/state/observability/metricsd/rollup/60s/w_{}", window_id);
            let _ = statefs_delete_nonblocking(client, key.as_str());
        }
    }
}

fn statefs_put_nonblocking(client: &KernelClient, key: &str, value: &[u8]) -> bool {
    let Ok(frame) = statefs_proto::encode_put_request(key, value) else {
        return false;
    };
    client.send(&frame, Wait::NonBlocking).is_ok()
}

fn statefs_delete_nonblocking(client: &KernelClient, key: &str) -> bool {
    let Ok(frame) = statefs_proto::encode_key_only_request(statefs_proto::OP_DEL, key) else {
        return false;
    };
    client.send(&frame, Wait::NonBlocking).is_ok()
}

fn put_with_retries(client: &KernelClient, key: &str, value: &[u8], retries: u32) -> bool {
    for _ in 0..retries {
        if statefs_put_nonblocking(client, key, value) {
            return true;
        }
        let _ = yield_();
    }
    false
}

/// Ready notifier invoked by init glue once service bootstrap is complete.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    pub fn notify(self) {
        (self.0)();
    }
}

/// Main metricsd runtime loop.
pub fn service_main_loop(notifier: ReadyNotifier) -> MetricsResult<()> {
    let server = match route_metricsd_blocking() {
        Some(server) => server,
        None => return Err(MetricsError::Ipc),
    };
    let _ = nexus_log::configure_sink_logd_slots(
        METRICSD_LOGD_SEND_SLOT,
        METRICSD_REPLY_SEND_SLOT,
        METRICSD_REPLY_RECV_SLOT,
    );
    notifier.notify();
    emit_line("metricsd: ready");

    let limits = load_runtime_limits();
    let mut registry = Registry::new_with_limits(limits);
    let mut limiter = RateLimiter::new_with_limits(limits);
    let mut retention = RetentionSink::new(limits);
    let mut reject_invalid_args_emitted = false;
    let mut reject_over_limit_emitted = false;
    let mut reject_rate_limited_emitted = false;
    let mut fallback_now = 0u64;

    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                let now = match nsec() {
                    Ok(v) => v,
                    Err(_) => {
                        fallback_now = fallback_now.saturating_add(1);
                        fallback_now
                    }
                };
                let (rsp, reject_status) =
                    handle_frame(&mut registry, &mut limiter, &mut retention, sender_service_id, now, frame.as_slice());
                if let Some(status) = reject_status {
                    match status {
                        STATUS_INVALID_ARGS if !reject_invalid_args_emitted => {
                            emit_line("metricsd: reject invalid_args");
                            reject_invalid_args_emitted = true;
                        }
                        STATUS_OVER_LIMIT if !reject_over_limit_emitted => {
                            emit_line("metricsd: reject over_limit");
                            reject_over_limit_emitted = true;
                        }
                        STATUS_RATE_LIMITED if !reject_rate_limited_emitted => {
                            emit_line("metricsd: reject rate_limited");
                            reject_rate_limited_emitted = true;
                        }
                        _ => {}
                    }
                }
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::NonBlocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(_) => return Err(MetricsError::Ipc),
        }
    }
}

fn handle_frame(
    registry: &mut Registry,
    limiter: &mut RateLimiter,
    retention: &mut RetentionSink,
    sender_service_id: u64,
    now_ns: u64,
    frame: &[u8],
) -> (Vec<u8>, Option<u8>) {
    let op = frame.get(3).copied().unwrap_or(0);
    let decoded = match decode_request(frame) {
        Ok(req) => req,
        Err(DecodeError::Malformed) => return (encode_status_response(op, 0, STATUS_INVALID_ARGS), Some(STATUS_INVALID_ARGS)),
        Err(DecodeError::OverLimit) => return (encode_status_response(op, 0, STATUS_OVER_LIMIT), Some(STATUS_OVER_LIMIT)),
        Err(DecodeError::Unsupported) => return (encode_status_response(op, 0, STATUS_INVALID_ARGS), Some(STATUS_INVALID_ARGS)),
    };

    // Budget all mutating operations except ping.
    if !matches!(decoded, Request::Ping { .. }) && limiter.is_limited(sender_service_id, now_ns) {
        let (op, nonce) = req_op_nonce(decoded);
        return (encode_status_response(op, nonce, STATUS_RATE_LIMITED), Some(STATUS_RATE_LIMITED));
    }

    match decoded {
        Request::CounterInc { nonce, name, labels, delta } => {
            let result = registry.counter_inc(sender_service_id, name, labels, delta);
            match result {
                Ok(value) => {
                    log_counter_snapshot(name, value);
                    retention.record_metric(metric_counter_record(name, value).as_str());
                    (encode_status_response(OP_COUNTER_INC, nonce, STATUS_OK), None)
                }
                Err(reject) => reject_rsp(OP_COUNTER_INC, nonce, reject),
            }
        }
        Request::GaugeSet { nonce, name, labels, value } => {
            let result = registry.gauge_set(sender_service_id, name, labels, value);
            match result {
                Ok(current) => {
                    log_gauge_snapshot(name, current);
                    retention.record_metric(metric_gauge_record(name, current).as_str());
                    (encode_status_response(OP_GAUGE_SET, nonce, STATUS_OK), None)
                }
                Err(reject) => reject_rsp(OP_GAUGE_SET, nonce, reject),
            }
        }
        Request::HistObserve { nonce, name, labels, value } => {
            let result = registry.hist_observe(sender_service_id, name, labels, value);
            match result {
                Ok((count, sum)) => {
                    log_hist_snapshot(name, count, sum);
                    retention.record_metric(metric_hist_record(name, count, sum).as_str());
                    (encode_status_response(OP_HIST_OBSERVE, nonce, STATUS_OK), None)
                }
                Err(reject) => reject_rsp(OP_HIST_OBSERVE, nonce, reject),
            }
        }
        Request::SpanStart {
            nonce,
            span_id,
            trace_id,
            parent_span_id,
            start_ns,
            name,
            attrs,
        } => {
            let result =
                registry.span_start(sender_service_id, span_id.0, trace_id.0, parent_span_id.0, start_ns, name, attrs);
            match result {
                Ok(()) => (encode_status_response(OP_SPAN_START, nonce, STATUS_OK), None),
                Err(reject) => reject_rsp(OP_SPAN_START, nonce, reject),
            }
        }
        Request::SpanEnd { nonce, span_id, end_ns, status, attrs } => {
            let result = registry.span_end(sender_service_id, span_id.0, end_ns, status, attrs);
            match result {
                Ok(ended) => {
                    log_span_end(
                        &ended.name,
                        ended.parent_span_id,
                        ended.duration_ns,
                        ended.status,
                        &ended.start_attrs,
                        &ended.end_attrs,
                    );
                    retention.record_span(
                        span_end_record(
                            &ended.name,
                            ended.parent_span_id,
                            ended.duration_ns,
                            ended.status,
                            &ended.start_attrs,
                            &ended.end_attrs,
                        )
                        .as_str(),
                    );
                    (encode_status_response(OP_SPAN_END, nonce, STATUS_OK), None)
                }
                Err(reject) => reject_rsp(OP_SPAN_END, nonce, reject),
            }
        }
        Request::Ping { nonce } => (encode_status_response(OP_PING, nonce, STATUS_OK), None),
    }
}

fn req_op_nonce(req: Request<'_>) -> (u8, u32) {
    match req {
        Request::CounterInc { nonce, .. } => (OP_COUNTER_INC, nonce),
        Request::GaugeSet { nonce, .. } => (OP_GAUGE_SET, nonce),
        Request::HistObserve { nonce, .. } => (OP_HIST_OBSERVE, nonce),
        Request::SpanStart { nonce, .. } => (OP_SPAN_START, nonce),
        Request::SpanEnd { nonce, .. } => (OP_SPAN_END, nonce),
        Request::Ping { nonce } => (OP_PING, nonce),
    }
}

fn reject_rsp(op: u8, nonce: u32, reject: RejectReason) -> (Vec<u8>, Option<u8>) {
    let status = match reject {
        RejectReason::InvalidArgs => STATUS_INVALID_ARGS,
        RejectReason::OverLimit => STATUS_OVER_LIMIT,
        RejectReason::RateLimited => STATUS_RATE_LIMITED,
        RejectReason::NotFound => STATUS_NOT_FOUND,
    };
    (encode_status_response(op, nonce, status), Some(status))
}

fn route_metricsd_blocking() -> Option<KernelServer> {
    let (send_slot, recv_slot) = route_blocking(b"metricsd")?;
    KernelServer::new_with_slots(recv_slot, send_slot).ok()
}

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return None;
    }
    static ROUTE_NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()])?;
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);

    // init wires service slots asynchronously during bring-up; keep retrying deterministically.
    loop {
        loop {
            match nexus_abi::ipc_send_v1(
                CTRL_SEND_SLOT,
                &hdr,
                &req[..req_len],
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    let _ = yield_();
                }
                Err(_) => return None,
            }
        }

        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        loop {
            match nexus_abi::ipc_recv_v1(
                CTRL_RECV_SLOT,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    let n = n as usize;
                    if n != 17 {
                        let _ = yield_();
                        continue;
                    }
                    let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                    if got_nonce != nonce {
                        continue;
                    }
                    let (status, send_slot, recv_slot) = nexus_abi::routing::decode_route_rsp(&buf[..13])?;
                    if status == nexus_abi::routing::STATUS_OK {
                        return Some((send_slot, recv_slot));
                    }
                    break;
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => return None,
            }
        }
    }
}

fn log_counter_snapshot(name: &[u8], value: u64) {
    nexus_log::info("metricsd", |line| {
        line.text("metrics snapshot counter name=");
        line.text(as_utf8_or_placeholder(name));
        line.text(" value=");
        line.dec(value);
    });
}

fn log_gauge_snapshot(name: &[u8], value: i64) {
    nexus_log::info("metricsd", |line| {
        line.text("metrics snapshot gauge name=");
        line.text(as_utf8_or_placeholder(name));
        line.text(" value=");
        if value < 0 {
            line.text("-");
            line.dec(value.unsigned_abs());
        } else {
            line.dec(value as u64);
        }
    });
}

fn log_hist_snapshot(name: &[u8], count: u64, sum: u64) {
    nexus_log::info("metricsd", |line| {
        line.text("metrics snapshot histogram name=");
        line.text(as_utf8_or_placeholder(name));
        line.text(" count=");
        line.dec(count);
        line.text(" sum=");
        line.dec(sum);
    });
}

fn log_span_end(name: &[u8], parent_span_id: u64, duration_ns: u64, status: u8, start_attrs: &[u8], end_attrs: &[u8]) {
    let start_attrs_text = escaped_attrs_or_placeholder(start_attrs);
    let end_attrs_text = escaped_attrs_or_placeholder(end_attrs);
    nexus_log::info("metricsd", |line| {
        line.text("tracing span end name=");
        line.text(as_utf8_or_placeholder(name));
        line.text(" parent_span_id=");
        line.dec(parent_span_id);
        line.text(" duration_ns=");
        line.dec(duration_ns);
        line.text(" status=");
        line.dec(status as u64);
        line.text(" start_attrs=");
        line.text(start_attrs_text.as_str());
        line.text(" end_attrs=");
        line.text(end_attrs_text.as_str());
    });
}

fn metric_counter_record(name: &[u8], value: u64) -> String {
    format!(
        "metric counter name={} value={}",
        as_utf8_or_placeholder(name),
        value
    )
}

fn metric_gauge_record(name: &[u8], value: i64) -> String {
    format!(
        "metric gauge name={} value={}",
        as_utf8_or_placeholder(name),
        value
    )
}

fn metric_hist_record(name: &[u8], count: u64, sum: u64) -> String {
    format!(
        "metric histogram name={} count={} sum={}",
        as_utf8_or_placeholder(name),
        count,
        sum
    )
}

fn span_end_record(
    name: &[u8],
    parent_span_id: u64,
    duration_ns: u64,
    status: u8,
    start_attrs: &[u8],
    end_attrs: &[u8],
) -> String {
    format!(
        "span end name={} parent_span_id={} duration_ns={} status={} start_attrs={} end_attrs={}",
        as_utf8_or_placeholder(name),
        parent_span_id,
        duration_ns,
        status,
        escaped_attrs_or_placeholder(start_attrs),
        escaped_attrs_or_placeholder(end_attrs),
    )
}

fn as_utf8_or_placeholder(bytes: &[u8]) -> &str {
    match core::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => "<bin>",
    }
}

fn escaped_attrs_or_placeholder(bytes: &[u8]) -> String {
    let Ok(text) = core::str::from_utf8(bytes) else {
        return String::from("<bin>");
    };
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

fn emit_line(message: &str) {
    for b in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(b);
    }
}

fn load_runtime_limits() -> RuntimeLimits {
    let source = include_str!("../../../../recipes/observability/metrics.toml");
    match RuntimeLimits::parse_toml(source) {
        Ok(cfg) => cfg,
        Err(_) => {
            emit_line("metricsd: config fallback defaults");
            RuntimeLimits::default()
        }
    }
}
