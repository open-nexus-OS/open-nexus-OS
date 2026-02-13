// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Metrics/tracing client contract and wire helpers for metricsd v1
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host unit tests in this crate
//! ADR: docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md
//!
//! INVARIANTS:
//! - Deterministic IDs (no RNG dependency)
//! - Bounded wire fields and lengths
//! - Explicit newtypes at metrics/tracing boundaries

#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

extern crate alloc;

use alloc::vec::Vec;

/// Wire magic byte 0.
pub const MAGIC0: u8 = b'M';
/// Wire magic byte 1.
pub const MAGIC1: u8 = b'T';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Counter increment operation.
pub const OP_COUNTER_INC: u8 = 1;
/// Gauge set operation.
pub const OP_GAUGE_SET: u8 = 2;
/// Histogram observe operation.
pub const OP_HIST_OBSERVE: u8 = 3;
/// Span start operation.
pub const OP_SPAN_START: u8 = 4;
/// Span end operation.
pub const OP_SPAN_END: u8 = 5;
/// Ping operation (liveness probe).
pub const OP_PING: u8 = 6;

/// Response status: operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Response status: malformed or invalid payload.
pub const STATUS_INVALID_ARGS: u8 = 1;
/// Response status: bounded resource cap exceeded.
pub const STATUS_OVER_LIMIT: u8 = 2;
/// Response status: sender budget exceeded.
pub const STATUS_RATE_LIMITED: u8 = 3;
/// Response status: requested entity was not found.
pub const STATUS_NOT_FOUND: u8 = 4;

/// Maximum metric name size.
pub const MAX_METRIC_NAME_LEN: usize = 48;
/// Maximum labels payload size (RFC-0011 key=value\\n convention).
pub const MAX_LABELS_LEN: usize = 192;
/// Maximum span name size.
pub const MAX_SPAN_NAME_LEN: usize = 48;
/// Maximum span attributes payload size.
pub const MAX_ATTRS_LEN: usize = 192;

/// Opaque series identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SeriesId(pub u64);

/// Opaque span identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SpanId(pub u64);

/// Opaque trace identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct TraceId(pub u64);

/// Bounded metric name wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricName<'a>(&'a [u8]);

impl<'a> MetricName<'a> {
    /// Validates and wraps a metric name.
    pub fn new(name: &'a [u8]) -> Result<Self, EncodeError> {
        if name.is_empty() || name.len() > MAX_METRIC_NAME_LEN {
            return Err(EncodeError::InvalidArgs);
        }
        Ok(Self(name))
    }

    fn as_bytes(self) -> &'a [u8] {
        self.0
    }
}

/// Bounded span name wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpanName<'a>(&'a [u8]);

impl<'a> SpanName<'a> {
    /// Validates and wraps a span name.
    pub fn new(name: &'a [u8]) -> Result<Self, EncodeError> {
        if name.is_empty() || name.len() > MAX_SPAN_NAME_LEN {
            return Err(EncodeError::InvalidArgs);
        }
        Ok(Self(name))
    }

    fn as_bytes(self) -> &'a [u8] {
        self.0
    }
}

/// Bounded labels/attributes wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedFields<'a>(&'a [u8]);

impl<'a> BoundedFields<'a> {
    /// Validates and wraps bounded fields.
    pub fn labels(data: &'a [u8]) -> Result<Self, EncodeError> {
        if data.len() > MAX_LABELS_LEN {
            return Err(EncodeError::OverLimit);
        }
        Ok(Self(data))
    }

    /// Validates and wraps bounded span attributes.
    pub fn attrs(data: &'a [u8]) -> Result<Self, EncodeError> {
        if data.len() > MAX_ATTRS_LEN {
            return Err(EncodeError::OverLimit);
        }
        Ok(Self(data))
    }

    fn as_bytes(self) -> &'a [u8] {
        self.0
    }
}

/// Deterministic ID source derived from sender identity and a local monotonic counter.
pub struct DeterministicIdSource {
    sender_service_id: u64,
    next_local: u64,
}

impl DeterministicIdSource {
    /// Creates a deterministic ID source for one sender.
    pub const fn new(sender_service_id: u64) -> Self {
        Self { sender_service_id, next_local: 1 }
    }

    /// Returns the next deterministic span ID.
    pub fn next_span_id(&mut self) -> SpanId {
        let id = compose_deterministic_id(self.sender_service_id, self.next_local);
        self.next_local = self.next_local.saturating_add(1);
        SpanId(id)
    }

    /// Returns the next deterministic trace ID.
    pub fn next_trace_id(&mut self) -> TraceId {
        let id = compose_deterministic_id(self.sender_service_id ^ 0x5443_525f_4944, self.next_local);
        self.next_local = self.next_local.saturating_add(1);
        TraceId(id)
    }
}

fn compose_deterministic_id(sender_service_id: u64, local: u64) -> u64 {
    ((sender_service_id & 0xffff_ffff) << 32) | (local & 0xffff_ffff)
}

/// Minimal span-end client contract used by the span guard.
pub trait SpanEndClient {
    type Error;

    fn end_span(&self, span_id: SpanId, end_ns: u64, status: u8, attrs: &[u8]) -> Result<u8, Self::Error>;
}

/// RAII guard that emits `span_end` on drop unless ended explicitly.
pub struct SpanGuard<'a, C: SpanEndClient> {
    client: &'a C,
    span_id: SpanId,
    end_now: fn() -> u64,
    closed: bool,
}

impl<'a, C: SpanEndClient> SpanGuard<'a, C> {
    /// Creates a span guard with a deterministic end-time provider.
    pub fn new(client: &'a C, span_id: SpanId, end_now: fn() -> u64) -> Self {
        Self { client, span_id, end_now, closed: false }
    }

    /// Returns the guarded span id.
    pub const fn span_id(&self) -> SpanId {
        self.span_id
    }

    /// Ends the span explicitly and consumes the guard.
    pub fn end(mut self, end_ns: u64, status: u8, attrs: &[u8]) -> Result<u8, C::Error> {
        let rsp = self.client.end_span(self.span_id, end_ns, status, attrs)?;
        self.closed = true;
        Ok(rsp)
    }
}

impl<C: SpanEndClient> Drop for SpanGuard<'_, C> {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let _ = self.client.end_span(self.span_id, (self.end_now)(), STATUS_OK, b"");
        self.closed = true;
    }
}

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
fn default_guard_end_ns() -> u64 {
    0
}

/// Encoding error for metrics/tracing requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "encode failures must be handled"]
pub enum EncodeError {
    /// Input violates protocol shape.
    InvalidArgs,
    /// Bounded limits were exceeded.
    OverLimit,
}

/// Decode error for metrics/tracing wire requests/responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "decode failures must be handled"]
pub enum DecodeError {
    /// Frame shape is malformed.
    Malformed,
    /// Frame exceeds configured limits.
    OverLimit,
    /// Unsupported operation or version.
    Unsupported,
}

/// Decoded request frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    CounterInc {
        nonce: u32,
        name: &'a [u8],
        labels: &'a [u8],
        delta: u64,
    },
    GaugeSet {
        nonce: u32,
        name: &'a [u8],
        labels: &'a [u8],
        value: i64,
    },
    HistObserve {
        nonce: u32,
        name: &'a [u8],
        labels: &'a [u8],
        value: u64,
    },
    SpanStart {
        nonce: u32,
        span_id: SpanId,
        trace_id: TraceId,
        parent_span_id: SpanId,
        start_ns: u64,
        name: &'a [u8],
        attrs: &'a [u8],
    },
    SpanEnd {
        nonce: u32,
        span_id: SpanId,
        end_ns: u64,
        status: u8,
        attrs: &'a [u8],
    },
    Ping {
        nonce: u32,
    },
}

/// Encodes a COUNTER_INC frame.
pub fn encode_counter_inc(
    nonce: u32,
    name: MetricName<'_>,
    labels: BoundedFields<'_>,
    delta: u64,
) -> Result<Vec<u8>, EncodeError> {
    encode_metric_value_frame(OP_COUNTER_INC, nonce, name.as_bytes(), labels.as_bytes(), delta as i64)
}

/// Encodes a GAUGE_SET frame.
pub fn encode_gauge_set(
    nonce: u32,
    name: MetricName<'_>,
    labels: BoundedFields<'_>,
    value: i64,
) -> Result<Vec<u8>, EncodeError> {
    encode_metric_value_frame(OP_GAUGE_SET, nonce, name.as_bytes(), labels.as_bytes(), value)
}

/// Encodes a HIST_OBSERVE frame.
pub fn encode_hist_observe(
    nonce: u32,
    name: MetricName<'_>,
    labels: BoundedFields<'_>,
    value: u64,
) -> Result<Vec<u8>, EncodeError> {
    encode_metric_value_frame(OP_HIST_OBSERVE, nonce, name.as_bytes(), labels.as_bytes(), value as i64)
}

fn encode_metric_value_frame(
    op: u8,
    nonce: u32,
    name: &[u8],
    labels: &[u8],
    value: i64,
) -> Result<Vec<u8>, EncodeError> {
    if name.is_empty() || name.len() > MAX_METRIC_NAME_LEN {
        return Err(EncodeError::InvalidArgs);
    }
    if labels.len() > MAX_LABELS_LEN {
        return Err(EncodeError::OverLimit);
    }
    let mut out = Vec::with_capacity(4 + 4 + 1 + 2 + 8 + name.len() + labels.len());
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, op]);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(&(labels.len() as u16).to_le_bytes());
    out.extend_from_slice(&(value as i64).to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(labels);
    Ok(out)
}

/// Encodes a SPAN_START frame.
pub fn encode_span_start(
    nonce: u32,
    span_id: SpanId,
    trace_id: TraceId,
    parent_span_id: SpanId,
    start_ns: u64,
    name: SpanName<'_>,
    attrs: BoundedFields<'_>,
) -> Result<Vec<u8>, EncodeError> {
    let name = name.as_bytes();
    let attrs = attrs.as_bytes();
    if attrs.len() > MAX_ATTRS_LEN {
        return Err(EncodeError::OverLimit);
    }
    let mut out = Vec::with_capacity(4 + 4 + 8 + 8 + 8 + 8 + 1 + 2 + name.len() + attrs.len());
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_SPAN_START]);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(&span_id.0.to_le_bytes());
    out.extend_from_slice(&trace_id.0.to_le_bytes());
    out.extend_from_slice(&parent_span_id.0.to_le_bytes());
    out.extend_from_slice(&start_ns.to_le_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(&(attrs.len() as u16).to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(attrs);
    Ok(out)
}

/// Encodes a SPAN_END frame.
pub fn encode_span_end(
    nonce: u32,
    span_id: SpanId,
    end_ns: u64,
    status: u8,
    attrs: BoundedFields<'_>,
) -> Result<Vec<u8>, EncodeError> {
    let attrs = attrs.as_bytes();
    if attrs.len() > MAX_ATTRS_LEN {
        return Err(EncodeError::OverLimit);
    }
    let mut out = Vec::with_capacity(4 + 4 + 8 + 8 + 1 + 2 + attrs.len());
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_SPAN_END]);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(&span_id.0.to_le_bytes());
    out.extend_from_slice(&end_ns.to_le_bytes());
    out.push(status);
    out.extend_from_slice(&(attrs.len() as u16).to_le_bytes());
    out.extend_from_slice(attrs);
    Ok(out)
}

/// Encodes a PING frame.
pub fn encode_ping(nonce: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_PING]);
    out.extend_from_slice(&nonce.to_le_bytes());
    out
}

/// Decodes a metricsd request frame.
pub fn decode_request(frame: &[u8]) -> Result<Request<'_>, DecodeError> {
    if frame.len() < 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(DecodeError::Malformed);
    }
    if frame[2] != VERSION {
        return Err(DecodeError::Unsupported);
    }
    let op = frame[3];
    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    match op {
        OP_COUNTER_INC | OP_GAUGE_SET | OP_HIST_OBSERVE => decode_metric_value(op, nonce, &frame[8..]),
        OP_SPAN_START => decode_span_start(nonce, &frame[8..]),
        OP_SPAN_END => decode_span_end(nonce, &frame[8..]),
        OP_PING => {
            if frame.len() != 8 {
                Err(DecodeError::Malformed)
            } else {
                Ok(Request::Ping { nonce })
            }
        }
        _ => Err(DecodeError::Unsupported),
    }
}

fn decode_metric_value<'a>(op: u8, nonce: u32, payload: &'a [u8]) -> Result<Request<'a>, DecodeError> {
    if payload.len() < 1 + 2 + 8 {
        return Err(DecodeError::Malformed);
    }
    let name_len = payload[0] as usize;
    let labels_len = u16::from_le_bytes([payload[1], payload[2]]) as usize;
    let value = i64::from_le_bytes([
        payload[3], payload[4], payload[5], payload[6], payload[7], payload[8], payload[9], payload[10],
    ]);
    if name_len == 0 || name_len > MAX_METRIC_NAME_LEN || labels_len > MAX_LABELS_LEN {
        return Err(DecodeError::OverLimit);
    }
    if payload.len() != 11 + name_len + labels_len {
        return Err(DecodeError::Malformed);
    }
    let name = &payload[11..11 + name_len];
    let labels = &payload[11 + name_len..];
    match op {
        OP_COUNTER_INC => {
            if value < 0 {
                return Err(DecodeError::Malformed);
            }
            Ok(Request::CounterInc { nonce, name, labels, delta: value as u64 })
        }
        OP_GAUGE_SET => Ok(Request::GaugeSet { nonce, name, labels, value }),
        OP_HIST_OBSERVE => {
            if value < 0 {
                return Err(DecodeError::Malformed);
            }
            Ok(Request::HistObserve { nonce, name, labels, value: value as u64 })
        }
        _ => Err(DecodeError::Unsupported),
    }
}

fn decode_span_start<'a>(nonce: u32, payload: &'a [u8]) -> Result<Request<'a>, DecodeError> {
    if payload.len() < 8 + 8 + 8 + 8 + 1 + 2 {
        return Err(DecodeError::Malformed);
    }
    let span_id = SpanId(u64::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5], payload[6], payload[7],
    ]));
    let trace_id = TraceId(u64::from_le_bytes([
        payload[8], payload[9], payload[10], payload[11], payload[12], payload[13], payload[14], payload[15],
    ]));
    let parent_span_id = SpanId(u64::from_le_bytes([
        payload[16], payload[17], payload[18], payload[19], payload[20], payload[21], payload[22], payload[23],
    ]));
    let start_ns = u64::from_le_bytes([
        payload[24], payload[25], payload[26], payload[27], payload[28], payload[29], payload[30], payload[31],
    ]);
    let name_len = payload[32] as usize;
    let attrs_len = u16::from_le_bytes([payload[33], payload[34]]) as usize;
    if name_len == 0 || name_len > MAX_SPAN_NAME_LEN || attrs_len > MAX_ATTRS_LEN {
        return Err(DecodeError::OverLimit);
    }
    if payload.len() != 35 + name_len + attrs_len {
        return Err(DecodeError::Malformed);
    }
    let name = &payload[35..35 + name_len];
    let attrs = &payload[35 + name_len..];
    Ok(Request::SpanStart { nonce, span_id, trace_id, parent_span_id, start_ns, name, attrs })
}

fn decode_span_end<'a>(nonce: u32, payload: &'a [u8]) -> Result<Request<'a>, DecodeError> {
    if payload.len() < 8 + 8 + 1 + 2 {
        return Err(DecodeError::Malformed);
    }
    let span_id = SpanId(u64::from_le_bytes([
        payload[0], payload[1], payload[2], payload[3], payload[4], payload[5], payload[6], payload[7],
    ]));
    let end_ns = u64::from_le_bytes([
        payload[8], payload[9], payload[10], payload[11], payload[12], payload[13], payload[14], payload[15],
    ]);
    let status = payload[16];
    let attrs_len = u16::from_le_bytes([payload[17], payload[18]]) as usize;
    if attrs_len > MAX_ATTRS_LEN {
        return Err(DecodeError::OverLimit);
    }
    if payload.len() != 19 + attrs_len {
        return Err(DecodeError::Malformed);
    }
    let attrs = &payload[19..];
    Ok(Request::SpanEnd { nonce, span_id, end_ns, status, attrs })
}

/// Encodes a status-only response frame.
pub fn encode_status_response(op: u8, nonce: u32, status: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, op | 0x80, status]);
    out.extend_from_slice(&nonce.to_le_bytes());
    out
}

/// Decodes a status-only response and validates nonce/opcode.
pub fn decode_status_response(frame: &[u8], expected_op: u8, expected_nonce: u32) -> Result<u8, DecodeError> {
    if frame.len() != 9 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return Err(DecodeError::Malformed);
    }
    if frame[3] != (expected_op | 0x80) {
        return Err(DecodeError::Unsupported);
    }
    let nonce = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
    if nonce != expected_nonce {
        return Err(DecodeError::Malformed);
    }
    Ok(frame[4])
}

/// Client-side metrics IPC errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "metrics client errors must be handled"]
pub enum ClientError {
    /// Encoding rejected due to invalid/bounded input.
    Encode(EncodeError),
    /// Transport failure.
    Transport,
    /// Response decode failure.
    Decode(DecodeError),
}

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub mod client {
    use super::*;
    use core::sync::atomic::{AtomicU32, Ordering};
    use core::time::Duration;
    use nexus_ipc::{Client as _, KernelClient, Wait};

    /// OS metrics/tracing client over kernel IPC.
    pub struct MetricsClient {
        ipc: KernelClient,
        next_nonce: AtomicU32,
    }

    impl MetricsClient {
        /// Creates a client routed to `metricsd`.
        pub fn new() -> Result<Self, ClientError> {
            Self::new_for("metricsd")
        }

        /// Creates a client for an explicit service name.
        pub fn new_for(service_name: &str) -> Result<Self, ClientError> {
            let ipc = KernelClient::new_for(service_name).map_err(|_| ClientError::Transport)?;
            Ok(Self { ipc, next_nonce: AtomicU32::new(1) })
        }

        fn nonce(&self) -> u32 {
            self.next_nonce.fetch_add(1, Ordering::Relaxed)
        }

        /// Sends a counter increment.
        pub fn counter_inc(&self, name: &str, labels: &[u8], delta: u64) -> Result<u8, ClientError> {
            let nonce = self.nonce();
            let frame = encode_counter_inc(
                nonce,
                MetricName::new(name.as_bytes()).map_err(ClientError::Encode)?,
                BoundedFields::labels(labels).map_err(ClientError::Encode)?,
                delta,
            )
            .map_err(ClientError::Encode)?;
            self.send_and_parse(OP_COUNTER_INC, nonce, &frame)
        }

        /// Sends a gauge set.
        pub fn gauge_set(&self, name: &str, labels: &[u8], value: i64) -> Result<u8, ClientError> {
            let nonce = self.nonce();
            let frame = encode_gauge_set(
                nonce,
                MetricName::new(name.as_bytes()).map_err(ClientError::Encode)?,
                BoundedFields::labels(labels).map_err(ClientError::Encode)?,
                value,
            )
            .map_err(ClientError::Encode)?;
            self.send_and_parse(OP_GAUGE_SET, nonce, &frame)
        }

        /// Sends a histogram observation.
        pub fn hist_observe(&self, name: &str, labels: &[u8], value: u64) -> Result<u8, ClientError> {
            let nonce = self.nonce();
            let frame = encode_hist_observe(
                nonce,
                MetricName::new(name.as_bytes()).map_err(ClientError::Encode)?,
                BoundedFields::labels(labels).map_err(ClientError::Encode)?,
                value,
            )
            .map_err(ClientError::Encode)?;
            self.send_and_parse(OP_HIST_OBSERVE, nonce, &frame)
        }

        /// Sends a span start event.
        pub fn span_start(
            &self,
            span_id: SpanId,
            trace_id: TraceId,
            parent_span_id: SpanId,
            start_ns: u64,
            name: &str,
            attrs: &[u8],
        ) -> Result<u8, ClientError> {
            let nonce = self.nonce();
            let frame = encode_span_start(
                nonce,
                span_id,
                trace_id,
                parent_span_id,
                start_ns,
                SpanName::new(name.as_bytes()).map_err(ClientError::Encode)?,
                BoundedFields::attrs(attrs).map_err(ClientError::Encode)?,
            )
            .map_err(ClientError::Encode)?;
            self.send_and_parse(OP_SPAN_START, nonce, &frame)
        }

        /// Starts a span and returns an end-on-drop guard.
        pub fn span_guard(
            &self,
            ids: &mut DeterministicIdSource,
            parent_span_id: SpanId,
            start_ns: u64,
            name: &str,
            attrs: &[u8],
        ) -> Result<SpanGuard<'_, Self>, ClientError> {
            let span_id = ids.next_span_id();
            let trace_id = ids.next_trace_id();
            let status = self.span_start(span_id, trace_id, parent_span_id, start_ns, name, attrs)?;
            if status != STATUS_OK {
                return Err(ClientError::Decode(DecodeError::Malformed));
            }
            Ok(SpanGuard::new(self, span_id, default_guard_end_ns))
        }

        /// Sends a span end event.
        pub fn span_end(
            &self,
            span_id: SpanId,
            end_ns: u64,
            status: u8,
            attrs: &[u8],
        ) -> Result<u8, ClientError> {
            let nonce = self.nonce();
            let frame = encode_span_end(
                nonce,
                span_id,
                end_ns,
                status,
                BoundedFields::attrs(attrs).map_err(ClientError::Encode)?,
            )
            .map_err(ClientError::Encode)?;
            self.send_and_parse(OP_SPAN_END, nonce, &frame)
        }

        /// Sends a liveness ping.
        pub fn ping(&self) -> Result<u8, ClientError> {
            let nonce = self.nonce();
            let frame = encode_ping(nonce);
            self.send_and_parse(OP_PING, nonce, &frame)
        }

        fn send_and_parse(&self, op: u8, nonce: u32, frame: &[u8]) -> Result<u8, ClientError> {
            self.ipc
                .send(frame, Wait::Timeout(Duration::from_millis(500)))
                .map_err(|_| ClientError::Transport)?;
            let rsp = self
                .ipc
                .recv(Wait::Timeout(Duration::from_millis(500)))
                .map_err(|_| ClientError::Transport)?;
            decode_status_response(&rsp, op, nonce).map_err(ClientError::Decode)
        }
    }

    impl SpanEndClient for MetricsClient {
        type Error = ClientError;

        fn end_span(&self, span_id: SpanId, end_ns: u64, status: u8, attrs: &[u8]) -> Result<u8, Self::Error> {
            MetricsClient::span_end(self, span_id, end_ns, status, attrs)
        }
    }
}

/// Host-only deterministic backend for tests.
#[cfg(not(all(feature = "os-lite", nexus_env = "os")))]
pub mod host {
    use super::*;

    /// Minimal host event model.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Event {
        Counter { name: Vec<u8>, labels: Vec<u8>, delta: u64 },
        Gauge { name: Vec<u8>, labels: Vec<u8>, value: i64 },
        Hist { name: Vec<u8>, labels: Vec<u8>, value: u64 },
        SpanStart { span_id: SpanId, trace_id: TraceId, name: Vec<u8> },
        SpanEnd { span_id: SpanId, status: u8 },
    }

    /// In-memory host backend used by host tests.
    pub struct HostBackend {
        events: Vec<Event>,
    }

    impl HostBackend {
        /// Creates an empty backend.
        pub fn new() -> Self {
            Self { events: Vec::new() }
        }

        /// Records a counter event.
        pub fn counter_inc(&mut self, name: &str, labels: &[u8], delta: u64) {
            self.events.push(Event::Counter {
                name: name.as_bytes().to_vec(),
                labels: labels.to_vec(),
                delta,
            });
        }

        /// Records a gauge event.
        pub fn gauge_set(&mut self, name: &str, labels: &[u8], value: i64) {
            self.events.push(Event::Gauge {
                name: name.as_bytes().to_vec(),
                labels: labels.to_vec(),
                value,
            });
        }

        /// Records a histogram event.
        pub fn hist_observe(&mut self, name: &str, labels: &[u8], value: u64) {
            self.events.push(Event::Hist {
                name: name.as_bytes().to_vec(),
                labels: labels.to_vec(),
                value,
            });
        }

        /// Records a span start event.
        pub fn span_start(&mut self, span_id: SpanId, trace_id: TraceId, name: &str) {
            self.events.push(Event::SpanStart { span_id, trace_id, name: name.as_bytes().to_vec() });
        }

        /// Records a span end event.
        pub fn span_end(&mut self, span_id: SpanId, status: u8) {
            self.events.push(Event::SpanEnd { span_id, status });
        }

        /// Returns immutable events.
        pub fn events(&self) -> &[Event] {
            &self.events
        }
    }

    impl Default for HostBackend {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// Best-effort counter macro.
#[macro_export]
macro_rules! metrics_counter_inc {
    ($client:expr, $name:expr, $delta:expr) => {{
        let _ = $client.counter_inc($name, b"", $delta);
    }};
    ($client:expr, $name:expr, $labels:expr, $delta:expr) => {{
        let _ = $client.counter_inc($name, $labels, $delta);
    }};
}

/// Best-effort gauge macro.
#[macro_export]
macro_rules! metrics_gauge_set {
    ($client:expr, $name:expr, $value:expr) => {{
        let _ = $client.gauge_set($name, b"", $value);
    }};
    ($client:expr, $name:expr, $labels:expr, $value:expr) => {{
        let _ = $client.gauge_set($name, $labels, $value);
    }};
}

/// Best-effort histogram macro.
#[macro_export]
macro_rules! metrics_hist_observe {
    ($client:expr, $name:expr, $value:expr) => {{
        let _ = $client.hist_observe($name, b"", $value);
    }};
    ($client:expr, $name:expr, $labels:expr, $value:expr) => {{
        let _ = $client.hist_observe($name, $labels, $value);
    }};
}

/// Starts an end-on-drop span guard on OS metrics clients.
#[macro_export]
macro_rules! metrics_span_guard_start {
    ($client:expr, $ids:expr, $start_ns:expr, $name:expr) => {{
        $client.span_guard($ids, $crate::SpanId(0), $start_ns, $name, b"")
    }};
    ($client:expr, $ids:expr, $parent_span_id:expr, $start_ns:expr, $name:expr, $attrs:expr) => {{
        $client.span_guard($ids, $parent_span_id, $start_ns, $name, $attrs)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_reject_metric_name_over_limit() {
        let name = [b'a'; MAX_METRIC_NAME_LEN + 1];
        assert_eq!(MetricName::new(&name), Err(EncodeError::InvalidArgs));
    }

    #[test]
    fn test_reject_attrs_over_limit() {
        let attrs = [b'k'; MAX_ATTRS_LEN + 1];
        assert_eq!(BoundedFields::attrs(&attrs), Err(EncodeError::OverLimit));
    }

    #[test]
    fn test_deterministic_ids_are_monotonic() {
        let mut ids = DeterministicIdSource::new(0xAA55);
        let s1 = ids.next_span_id().0;
        let s2 = ids.next_span_id().0;
        let t1 = ids.next_trace_id().0;
        assert!(s2 > s1);
        assert_ne!(s1, t1);
    }

    #[test]
    fn test_send_sync_boundaries() {
        assert_send_sync::<DeterministicIdSource>();
        assert_send_sync::<SeriesId>();
        assert_send_sync::<SpanId>();
        assert_send_sync::<TraceId>();
        assert_send_sync::<EncodeError>();
        assert_send_sync::<DecodeError>();
        assert_send_sync::<ClientError>();
    }

    #[test]
    fn test_counter_wire_roundtrip() {
        let frame = encode_counter_inc(
            7,
            MetricName::new(b"sched.wakeups").unwrap(),
            BoundedFields::labels(b"svc=timed\n").unwrap(),
            3,
        )
        .unwrap();
        let req = decode_request(&frame).unwrap();
        match req {
            Request::CounterInc { nonce, name, labels, delta } => {
                assert_eq!(nonce, 7);
                assert_eq!(name, b"sched.wakeups");
                assert_eq!(labels, b"svc=timed\n");
                assert_eq!(delta, 3);
            }
            _ => panic!("wrong request variant"),
        }
    }

    #[test]
    fn test_best_effort_macros_record_events_on_host_backend() {
        let mut backend = host::HostBackend::new();
        metrics_counter_inc!(backend, "boot.events", 1);
        metrics_gauge_set!(backend, "sched.depth", -2);
        metrics_hist_observe!(backend, "timed.latency", 77);
        assert_eq!(backend.events().len(), 3);
    }

    struct FakeSpanClient {
        calls: AtomicUsize,
        span: AtomicU64,
        end_ns: AtomicU64,
        status: AtomicU8,
    }

    impl FakeSpanClient {
        const fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                span: AtomicU64::new(0),
                end_ns: AtomicU64::new(0),
                status: AtomicU8::new(0),
            }
        }
    }

    impl SpanEndClient for FakeSpanClient {
        type Error = ();

        fn end_span(&self, span_id: SpanId, end_ns: u64, status: u8, _attrs: &[u8]) -> Result<u8, Self::Error> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.span.store(span_id.0, Ordering::Relaxed);
            self.end_ns.store(end_ns, Ordering::Relaxed);
            self.status.store(status, Ordering::Relaxed);
            Ok(STATUS_OK)
        }
    }

    fn fixed_end_now() -> u64 {
        1234
    }

    #[test]
    fn test_span_guard_drop_sends_end_once() {
        let fake = FakeSpanClient::new();
        {
            let _guard = SpanGuard::new(&fake, SpanId(77), fixed_end_now);
        }
        assert_eq!(fake.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fake.span.load(Ordering::Relaxed), 77);
        assert_eq!(fake.end_ns.load(Ordering::Relaxed), 1234);
        assert_eq!(fake.status.load(Ordering::Relaxed), STATUS_OK);
    }

    #[test]
    fn test_span_guard_manual_end_disarms_drop() {
        let fake = FakeSpanClient::new();
        let guard = SpanGuard::new(&fake, SpanId(99), fixed_end_now);
        let _ = guard.end(777, 3, b"result=ok\n");
        assert_eq!(fake.calls.load(Ordering::Relaxed), 1);
        assert_eq!(fake.span.load(Ordering::Relaxed), 99);
        assert_eq!(fake.end_ns.load(Ordering::Relaxed), 777);
        assert_eq!(fake.status.load(Ordering::Relaxed), 3);
    }
}
