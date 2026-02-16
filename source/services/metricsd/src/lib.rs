// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: metricsd bounded registry and span table for observability v2
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host unit tests in this crate
//! ADR: docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md
//!
//! INVARIANTS:
//! - Bounded series cardinality and bounded live span state
//! - Deterministic reject categories (invalid_args / over_limit / rate_limited)
//! - Sender identity binding for span IDs (no payload-only trust)

#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use nexus_metrics::{MAX_ATTRS_LEN, MAX_LABELS_LEN, MAX_METRIC_NAME_LEN, MAX_SPAN_NAME_LEN};

pub const MAX_SERIES_TOTAL: usize = 64;
pub const MAX_SERIES_PER_METRIC: usize = 16;
pub const MAX_LIVE_SPANS: usize = 64;
pub const RATE_WINDOW_NS: u64 = 1_000_000_000;
pub const RATE_MAX_EVENTS_PER_WINDOW: u32 = 64;
pub const RATE_MAX_SUBJECTS: usize = 64;

const HIST_BUCKETS_NS: [u64; 4] = [1_000_000, 5_000_000, 20_000_000, 100_000_000];

/// Runtime config for metrics/tracing bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLimits {
    pub max_series_total: usize,
    pub max_series_per_metric: usize,
    pub max_live_spans: usize,
    pub rate_window_ns: u64,
    pub rate_max_events_per_window: u32,
    pub rate_max_subjects: usize,
    pub max_metric_name_len: usize,
    pub max_labels_len: usize,
    pub max_span_name_len: usize,
    pub max_attrs_len: usize,
    pub retention_enabled: bool,
    pub retention_max_segments: u32,
    pub retention_max_records_per_segment: u32,
    pub retention_rollup_every: u32,
    pub retention_best_effort_retries: u32,
    pub retention_critical_retries: u32,
    pub retention_ttl_windows: u32,
    pub retention_gc_batch: u32,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            max_series_total: MAX_SERIES_TOTAL,
            max_series_per_metric: MAX_SERIES_PER_METRIC,
            max_live_spans: MAX_LIVE_SPANS,
            rate_window_ns: RATE_WINDOW_NS,
            rate_max_events_per_window: RATE_MAX_EVENTS_PER_WINDOW,
            rate_max_subjects: RATE_MAX_SUBJECTS,
            max_metric_name_len: MAX_METRIC_NAME_LEN,
            max_labels_len: MAX_LABELS_LEN,
            max_span_name_len: MAX_SPAN_NAME_LEN,
            max_attrs_len: MAX_ATTRS_LEN,
            retention_enabled: true,
            retention_max_segments: 8,
            retention_max_records_per_segment: 64,
            retention_rollup_every: 16,
            retention_best_effort_retries: 1,
            retention_critical_retries: 2,
            retention_ttl_windows: 12,
            retention_gc_batch: 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    InvalidValue,
    UnknownKey,
}

impl RuntimeLimits {
    /// Parses observability runtime limits from `recipes/observability/metrics.toml`.
    pub fn parse_toml(input: &str) -> Result<Self, ConfigError> {
        let mut cfg = Self::default();
        let mut section = "";
        for raw in input.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len().saturating_sub(1)];
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(ConfigError::InvalidValue);
            };
            let key = k.trim();
            let value_u64 = v.trim().parse::<u64>().map_err(|_| ConfigError::InvalidValue)?;
            match (section, key) {
                ("metrics", "max_series_total") => cfg.max_series_total = value_u64 as usize,
                ("metrics", "max_series_per_metric") => {
                    cfg.max_series_per_metric = value_u64 as usize
                }
                ("metrics", "max_live_spans") => cfg.max_live_spans = value_u64 as usize,
                ("ingest", "rate_window_ns") => cfg.rate_window_ns = value_u64,
                ("ingest", "max_events_per_window") => {
                    cfg.rate_max_events_per_window = value_u64 as u32
                }
                ("ingest", "max_subjects") => cfg.rate_max_subjects = value_u64 as usize,
                ("wire", "max_metric_name_len") => cfg.max_metric_name_len = value_u64 as usize,
                ("wire", "max_labels_len") => cfg.max_labels_len = value_u64 as usize,
                ("wire", "max_span_name_len") => cfg.max_span_name_len = value_u64 as usize,
                ("wire", "max_attrs_len") => cfg.max_attrs_len = value_u64 as usize,
                ("retention", "enabled") => cfg.retention_enabled = value_u64 != 0,
                ("retention", "max_segments") => cfg.retention_max_segments = value_u64 as u32,
                ("retention", "max_records_per_segment") => {
                    cfg.retention_max_records_per_segment = value_u64 as u32
                }
                ("retention", "rollup_every") => cfg.retention_rollup_every = value_u64 as u32,
                ("retention", "best_effort_retries") => {
                    cfg.retention_best_effort_retries = value_u64 as u32
                }
                ("retention", "critical_retries") => {
                    cfg.retention_critical_retries = value_u64 as u32
                }
                ("retention", "ttl_windows") => cfg.retention_ttl_windows = value_u64 as u32,
                ("retention", "gc_batch") => cfg.retention_gc_batch = value_u64 as u32,
                _ => return Err(ConfigError::UnknownKey),
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.max_series_total == 0
            || self.max_series_per_metric == 0
            || self.max_live_spans == 0
            || self.rate_window_ns == 0
            || self.rate_max_events_per_window == 0
            || self.rate_max_subjects == 0
            || self.max_metric_name_len == 0
            || self.max_labels_len == 0
            || self.max_span_name_len == 0
            || self.max_attrs_len == 0
            || self.retention_max_segments == 0
            || self.retention_max_records_per_segment == 0
            || self.retention_rollup_every == 0
            || self.retention_best_effort_retries == 0
            || self.retention_critical_retries == 0
            || self.retention_ttl_windows == 0
            || self.retention_gc_batch == 0
        {
            return Err(ConfigError::InvalidValue);
        }
        // Wire/runtime limits may only tighten, never exceed client wire contract.
        if self.max_metric_name_len > MAX_METRIC_NAME_LEN
            || self.max_labels_len > MAX_LABELS_LEN
            || self.max_span_name_len > MAX_SPAN_NAME_LEN
            || self.max_attrs_len > MAX_ATTRS_LEN
        {
            return Err(ConfigError::InvalidValue);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionEventKind {
    Metric,
    Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionUpdate {
    pub wal_slot: u32,
    pub wal_bytes: Vec<u8>,
    pub rollup_10s: Option<RollupFrame>,
    pub rollup_60s: Option<RollupFrame>,
    pub gc_rollup_10s: Vec<u64>,
    pub gc_rollup_60s: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollupFrame {
    pub window_id: u64,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RollupWindow {
    id: u64,
    metrics: u64,
    spans: u64,
}

/// Deterministic ring WAL planner used by OS-lite persistence writer.
pub struct RetentionEngine {
    limits: RuntimeLimits,
    active_segment: u32,
    active_records: u32,
    segments: Vec<Vec<u8>>,
    metrics_total: u64,
    spans_total: u64,
    pending_10s_metrics: u64,
    pending_10s_spans: u64,
    pending_60s_metrics: u64,
    pending_60s_spans: u64,
    pending_60s_count: u32,
    rollup_10s_windows: VecDeque<RollupWindow>,
    rollup_60s_windows: VecDeque<RollupWindow>,
}

impl RetentionEngine {
    pub fn new(limits: RuntimeLimits) -> Self {
        let mut segments = Vec::new();
        for _ in 0..limits.retention_max_segments {
            segments.push(Vec::new());
        }
        Self {
            limits,
            active_segment: 0,
            active_records: 0,
            segments,
            metrics_total: 0,
            spans_total: 0,
            pending_10s_metrics: 0,
            pending_10s_spans: 0,
            pending_60s_metrics: 0,
            pending_60s_spans: 0,
            pending_60s_count: 0,
            rollup_10s_windows: VecDeque::new(),
            rollup_60s_windows: VecDeque::new(),
        }
    }

    pub fn append(&mut self, kind: RetentionEventKind, record: &[u8]) -> Option<RetentionUpdate> {
        if !self.limits.retention_enabled {
            return None;
        }
        let slot = self.active_segment % self.limits.retention_max_segments;
        let slot_idx = slot as usize;
        if self.active_records == 0 {
            self.segments[slot_idx].clear();
        }
        self.segments[slot_idx].extend_from_slice(record);
        self.segments[slot_idx].push(b'\n');
        self.active_records = self.active_records.saturating_add(1);
        match kind {
            RetentionEventKind::Metric => {
                self.metrics_total = self.metrics_total.saturating_add(1);
                self.pending_10s_metrics = self.pending_10s_metrics.saturating_add(1);
            }
            RetentionEventKind::Span => {
                self.spans_total = self.spans_total.saturating_add(1);
                self.pending_10s_spans = self.pending_10s_spans.saturating_add(1);
            }
        }

        let total_events = self.metrics_total.saturating_add(self.spans_total);
        let mut rollup_10s = None;
        let mut rollup_60s = None;
        let mut gc_rollup_10s = Vec::new();
        let mut gc_rollup_60s = Vec::new();

        if total_events.checked_rem(self.limits.retention_rollup_every as u64) == Some(0) {
            let window_10s_id = total_events / self.limits.retention_rollup_every as u64;
            let window_10s = RollupWindow {
                id: window_10s_id,
                metrics: self.pending_10s_metrics,
                spans: self.pending_10s_spans,
            };
            self.pending_10s_metrics = 0;
            self.pending_10s_spans = 0;
            self.rollup_10s_windows.push_back(window_10s);
            rollup_10s = Some(RollupFrame {
                window_id: window_10s.id,
                bytes: encode_rollup_window("10s", window_10s),
            });
            gc_rollup_10s = trim_rollup_windows(
                &mut self.rollup_10s_windows,
                self.limits.retention_ttl_windows,
                self.limits.retention_gc_batch,
            );

            self.pending_60s_count = self.pending_60s_count.saturating_add(1);
            self.pending_60s_metrics = self.pending_60s_metrics.saturating_add(window_10s.metrics);
            self.pending_60s_spans = self.pending_60s_spans.saturating_add(window_10s.spans);
            if self.pending_60s_count >= 6 {
                let window_60s_id = window_10s_id / 6;
                let window_60s = RollupWindow {
                    id: window_60s_id,
                    metrics: self.pending_60s_metrics,
                    spans: self.pending_60s_spans,
                };
                self.pending_60s_count = 0;
                self.pending_60s_metrics = 0;
                self.pending_60s_spans = 0;
                self.rollup_60s_windows.push_back(window_60s);
                rollup_60s = Some(RollupFrame {
                    window_id: window_60s.id,
                    bytes: encode_rollup_window("60s", window_60s),
                });
                gc_rollup_60s = trim_rollup_windows(
                    &mut self.rollup_60s_windows,
                    self.limits.retention_ttl_windows,
                    self.limits.retention_gc_batch,
                );
            }
        }

        let update = RetentionUpdate {
            wal_slot: slot,
            wal_bytes: self.segments[slot_idx].clone(),
            rollup_10s,
            rollup_60s,
            gc_rollup_10s,
            gc_rollup_60s,
        };
        if self.active_records >= self.limits.retention_max_records_per_segment {
            self.active_records = 0;
            self.active_segment = self.active_segment.saturating_add(1);
        }
        Some(update)
    }
}

fn trim_rollup_windows(windows: &mut VecDeque<RollupWindow>, ttl: u32, gc_batch: u32) -> Vec<u64> {
    let mut gc_ids = Vec::new();
    while windows.len() > ttl as usize && gc_ids.len() < gc_batch as usize {
        if let Some(old) = windows.pop_front() {
            gc_ids.push(old.id);
        } else {
            break;
        }
    }
    gc_ids
}

fn encode_rollup_window(kind: &str, window: RollupWindow) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("kind=");
    out.push_str(kind);
    out.push('\n');
    out.push_str("window_id=");
    out.push_str(utoa(window.id).as_str());
    out.push('\n');
    out.push_str("metrics_total=");
    out.push_str(utoa(window.metrics).as_str());
    out.push('\n');
    out.push_str("spans_total=");
    out.push_str(utoa(window.spans).as_str());
    out.push('\n');
    out.into_bytes()
}

fn utoa(mut value: u64) -> String {
    let mut out = [0u8; 20];
    let mut idx = out.len();
    if value == 0 {
        idx = idx.saturating_sub(1);
        out[idx] = b'0';
    } else {
        while value != 0 {
            idx = idx.saturating_sub(1);
            out[idx] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    let mut s = String::new();
    for b in out[idx..].iter().copied() {
        s.push(b as char);
    }
    s
}

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;
#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "reject reasons must be handled"]
pub enum RejectReason {
    InvalidArgs,
    OverLimit,
    RateLimited,
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Clone, Debug)]
struct HistogramState {
    buckets: [u64; 5], // 4 configured buckets + overflow bucket
    count: u64,
    sum: u64,
}

impl HistogramState {
    fn new() -> Self {
        Self { buckets: [0; 5], count: 0, sum: 0 }
    }

    fn observe(&mut self, value: u64) {
        self.count = self.count.saturating_add(1);
        self.sum = self.sum.saturating_add(value);
        let mut idx = HIST_BUCKETS_NS.len();
        for (i, bound) in HIST_BUCKETS_NS.iter().enumerate() {
            if value <= *bound {
                idx = i;
                break;
            }
        }
        self.buckets[idx] = self.buckets[idx].saturating_add(1);
    }
}

#[derive(Clone, Debug)]
struct SeriesEntry {
    sender_service_id: u64,
    kind: MetricKind,
    name: Vec<u8>,
    labels: Vec<u8>,
    counter_value: u64,
    gauge_value: i64,
    histogram: HistogramState,
}

#[derive(Clone, Debug)]
struct LiveSpan {
    sender_service_id: u64,
    span_id: u64,
    trace_id: u64,
    parent_span_id: u64,
    name: Vec<u8>,
    start_attrs: Vec<u8>,
    start_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndedSpan {
    pub sender_service_id: u64,
    pub span_id: u64,
    pub trace_id: u64,
    pub parent_span_id: u64,
    pub name: Vec<u8>,
    pub start_attrs: Vec<u8>,
    pub end_attrs: Vec<u8>,
    pub duration_ns: u64,
    pub status: u8,
}

/// Span start request payload for bounded registry insertion.
pub struct SpanStartArgs<'a> {
    pub sender_service_id: u64,
    pub span_id: u64,
    pub trace_id: u64,
    pub parent_span_id: u64,
    pub start_ns: u64,
    pub name: &'a [u8],
    pub attrs: &'a [u8],
}

/// Bounded metrics and tracing state machine.
pub struct Registry {
    series: Vec<SeriesEntry>,
    live_spans: Vec<LiveSpan>,
    limits: RuntimeLimits,
}

impl Registry {
    pub fn new() -> Self {
        Self::new_with_limits(RuntimeLimits::default())
    }

    pub fn new_with_limits(limits: RuntimeLimits) -> Self {
        Self { series: Vec::new(), live_spans: Vec::new(), limits }
    }

    pub fn counter_inc(
        &mut self,
        sender_service_id: u64,
        name: &[u8],
        labels: &[u8],
        delta: u64,
    ) -> Result<u64, RejectReason> {
        let idx = self.ensure_series(sender_service_id, MetricKind::Counter, name, labels)?;
        let series = &mut self.series[idx];
        series.counter_value = series.counter_value.saturating_add(delta);
        Ok(series.counter_value)
    }

    pub fn gauge_set(
        &mut self,
        sender_service_id: u64,
        name: &[u8],
        labels: &[u8],
        value: i64,
    ) -> Result<i64, RejectReason> {
        let idx = self.ensure_series(sender_service_id, MetricKind::Gauge, name, labels)?;
        let series = &mut self.series[idx];
        series.gauge_value = value;
        Ok(series.gauge_value)
    }

    pub fn hist_observe(
        &mut self,
        sender_service_id: u64,
        name: &[u8],
        labels: &[u8],
        value: u64,
    ) -> Result<(u64, u64), RejectReason> {
        let idx = self.ensure_series(sender_service_id, MetricKind::Histogram, name, labels)?;
        let series = &mut self.series[idx];
        series.histogram.observe(value);
        Ok((series.histogram.count, series.histogram.sum))
    }

    pub fn span_start(&mut self, args: SpanStartArgs<'_>) -> Result<(), RejectReason> {
        let SpanStartArgs {
            sender_service_id,
            span_id,
            trace_id,
            parent_span_id,
            start_ns,
            name,
            attrs,
        } = args;
        if !span_id_matches_sender(sender_service_id, span_id) {
            return Err(RejectReason::InvalidArgs);
        }
        if name.is_empty() {
            return Err(RejectReason::InvalidArgs);
        }
        if name.len() > self.limits.max_span_name_len || attrs.len() > self.limits.max_attrs_len {
            return Err(RejectReason::OverLimit);
        }
        if self
            .live_spans
            .iter()
            .any(|span| span.sender_service_id == sender_service_id && span.span_id == span_id)
        {
            return Err(RejectReason::InvalidArgs);
        }
        if self.live_spans.len() >= self.limits.max_live_spans {
            return Err(RejectReason::OverLimit);
        }
        self.live_spans.push(LiveSpan {
            sender_service_id,
            span_id,
            trace_id,
            parent_span_id,
            name: name.to_vec(),
            start_attrs: attrs.to_vec(),
            start_ns,
        });
        Ok(())
    }

    pub fn span_end(
        &mut self,
        sender_service_id: u64,
        span_id: u64,
        end_ns: u64,
        status: u8,
        attrs: &[u8],
    ) -> Result<EndedSpan, RejectReason> {
        if attrs.len() > self.limits.max_attrs_len {
            return Err(RejectReason::OverLimit);
        }
        if let Some(pos) = self
            .live_spans
            .iter()
            .position(|span| span.sender_service_id == sender_service_id && span.span_id == span_id)
        {
            let span = self.live_spans.swap_remove(pos);
            let duration_ns = end_ns.saturating_sub(span.start_ns);
            return Ok(EndedSpan {
                sender_service_id,
                span_id,
                trace_id: span.trace_id,
                parent_span_id: span.parent_span_id,
                name: span.name,
                start_attrs: span.start_attrs,
                end_attrs: attrs.to_vec(),
                duration_ns,
                status,
            });
        }
        Err(RejectReason::NotFound)
    }

    fn ensure_series(
        &mut self,
        sender_service_id: u64,
        kind: MetricKind,
        name: &[u8],
        labels: &[u8],
    ) -> Result<usize, RejectReason> {
        if name.is_empty() {
            return Err(RejectReason::InvalidArgs);
        }
        if name.len() > self.limits.max_metric_name_len || labels.len() > self.limits.max_labels_len
        {
            return Err(RejectReason::OverLimit);
        }
        if let Some(pos) = self.series.iter().position(|entry| {
            entry.sender_service_id == sender_service_id
                && entry.kind == kind
                && entry.name.as_slice() == name
                && entry.labels.as_slice() == labels
        }) {
            return Ok(pos);
        }
        if self.series.len() >= self.limits.max_series_total {
            return Err(RejectReason::OverLimit);
        }
        let same_name_count = self
            .series
            .iter()
            .filter(|entry| entry.kind == kind && entry.name.as_slice() == name)
            .count();
        if same_name_count >= self.limits.max_series_per_metric {
            return Err(RejectReason::OverLimit);
        }
        self.series.push(SeriesEntry {
            sender_service_id,
            kind,
            name: name.to_vec(),
            labels: labels.to_vec(),
            counter_value: 0,
            gauge_value: 0,
            histogram: HistogramState::new(),
        });
        Ok(self.series.len().saturating_sub(1))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
struct RateWindow {
    sender_service_id: u64,
    window_start_ns: u64,
    used: u32,
}

/// Deterministic per-sender event limiter.
pub struct RateLimiter {
    windows: Vec<RateWindow>,
    limits: RuntimeLimits,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::new_with_limits(RuntimeLimits::default())
    }

    pub fn new_with_limits(limits: RuntimeLimits) -> Self {
        Self { windows: Vec::new(), limits }
    }

    pub fn is_limited(&mut self, sender_service_id: u64, now_ns: u64) -> bool {
        if let Some(pos) =
            self.windows.iter().position(|window| window.sender_service_id == sender_service_id)
        {
            let window = &mut self.windows[pos];
            if now_ns.saturating_sub(window.window_start_ns) >= self.limits.rate_window_ns {
                window.window_start_ns = now_ns;
                window.used = 0;
            }
            if window.used >= self.limits.rate_max_events_per_window {
                return true;
            }
            window.used = window.used.saturating_add(1);
            return false;
        }
        if self.windows.len() >= self.limits.rate_max_subjects {
            return true;
        }
        self.windows.push(RateWindow { sender_service_id, window_start_ns: now_ns, used: 1 });
        false
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn span_id_matches_sender(sender_service_id: u64, span_id: u64) -> bool {
    (span_id >> 32) == (sender_service_id & 0xffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_semantics_are_monotonic() {
        let mut reg = Registry::new();
        assert_eq!(reg.counter_inc(1, b"sched.wakeups", b"", 3), Ok(3));
        assert_eq!(reg.counter_inc(1, b"sched.wakeups", b"", 2), Ok(5));
    }

    #[test]
    fn gauge_semantics_are_replace() {
        let mut reg = Registry::new();
        assert_eq!(reg.gauge_set(1, b"sched.depth", b"", 9), Ok(9));
        assert_eq!(reg.gauge_set(1, b"sched.depth", b"", -2), Ok(-2));
    }

    #[test]
    fn histogram_bucket_boundaries_are_deterministic() {
        let mut reg = Registry::new();
        assert!(reg.hist_observe(1, b"timed.latency", b"", 1_000_000).is_ok());
        assert!(reg.hist_observe(1, b"timed.latency", b"", 5_000_000).is_ok());
        assert!(reg.hist_observe(1, b"timed.latency", b"", 500_000_000).is_ok());
        let idx = reg
            .series
            .iter()
            .position(|entry| entry.name.as_slice() == b"timed.latency")
            .unwrap_or(usize::MAX);
        assert_ne!(idx, usize::MAX);
        assert_eq!(reg.series[idx].histogram.count, 3);
    }

    #[test]
    fn span_lifecycle_start_end_is_deterministic() {
        let mut reg = Registry::new();
        let sender = 0x1234u64;
        let span_id = (sender << 32) | 1;
        assert!(reg
            .span_start(SpanStartArgs {
                sender_service_id: sender,
                span_id,
                trace_id: 77,
                parent_span_id: 0,
                start_ns: 100,
                name: b"exec.path",
                attrs: b"phase=run\n",
            })
            .is_ok());
        let ended = reg.span_end(sender, span_id, 180, 0, b"result=ok\n").unwrap_or(EndedSpan {
            sender_service_id: 0,
            span_id: 0,
            trace_id: 0,
            parent_span_id: 0,
            name: Vec::new(),
            start_attrs: Vec::new(),
            end_attrs: Vec::new(),
            duration_ns: 0,
            status: 255,
        });
        assert_eq!(ended.duration_ns, 80);
        assert_eq!(ended.status, 0);
        assert_eq!(ended.parent_span_id, 0);
        assert_eq!(ended.start_attrs.as_slice(), b"phase=run\n");
        assert_eq!(ended.end_attrs.as_slice(), b"result=ok\n");
    }

    #[test]
    fn test_reject_series_cap_exceeded() {
        let mut reg = Registry::new();
        for i in 0..MAX_SERIES_PER_METRIC {
            let mut labels = Vec::new();
            labels.extend_from_slice(b"id=");
            labels.push((i as u8).saturating_add(b'0'));
            assert!(reg.counter_inc(1, b"boot.events", &labels, 1).is_ok());
        }
        assert_eq!(
            reg.counter_inc(1, b"boot.events", b"id=overflow", 1),
            Err(RejectReason::OverLimit)
        );
    }

    #[test]
    fn test_reject_live_span_cap_exceeded() {
        let mut reg = Registry::new();
        let sender = 0x42u64;
        for i in 0..MAX_LIVE_SPANS {
            let span_id = ((sender & 0xffff_ffff) << 32) | (i as u64 + 1);
            assert!(reg
                .span_start(SpanStartArgs {
                    sender_service_id: sender,
                    span_id,
                    trace_id: i as u64,
                    parent_span_id: 0,
                    start_ns: i as u64,
                    name: b"s",
                    attrs: b"",
                })
                .is_ok());
        }
        let over = ((sender & 0xffff_ffff) << 32) | 0xffff;
        assert_eq!(
            reg.span_start(SpanStartArgs {
                sender_service_id: sender,
                span_id: over,
                trace_id: 999,
                parent_span_id: 0,
                start_ns: 999,
                name: b"s",
                attrs: b"",
            }),
            Err(RejectReason::OverLimit)
        );
    }

    #[test]
    fn test_reject_payload_identity_spoof() {
        let mut reg = Registry::new();
        let spoofed_span = (0x9999u64 << 32) | 1;
        assert_eq!(
            reg.span_start(SpanStartArgs {
                sender_service_id: 0x1111,
                span_id: spoofed_span,
                trace_id: 1,
                parent_span_id: 0,
                start_ns: 1,
                name: b"spoof",
                attrs: b"",
            }),
            Err(RejectReason::InvalidArgs)
        );
    }

    #[test]
    fn test_reject_rate_limit_exceeded() {
        let mut limiter = RateLimiter::new();
        let sender = 7u64;
        let mut limited = false;
        for _ in 0..(RATE_MAX_EVENTS_PER_WINDOW + 1) {
            if limiter.is_limited(sender, 10) {
                limited = true;
                break;
            }
        }
        assert!(limited);
    }

    #[test]
    fn test_reject_oversized_metric_fields() {
        let mut reg = Registry::new();
        let oversized_name = vec![b'n'; MAX_METRIC_NAME_LEN + 1];
        assert_eq!(
            reg.counter_inc(1, &oversized_name, b"svc=selftest-client\n", 1),
            Err(RejectReason::OverLimit)
        );

        let oversized_labels = vec![b'l'; MAX_LABELS_LEN + 1];
        assert_eq!(
            reg.counter_inc(1, b"selftest.counter", &oversized_labels, 1),
            Err(RejectReason::OverLimit)
        );
    }

    #[test]
    fn test_parse_runtime_limits_valid() {
        let toml = "\
[metrics]
max_series_total = 8
max_series_per_metric = 4
max_live_spans = 5

[ingest]
rate_window_ns = 2000
max_events_per_window = 3
max_subjects = 2

[wire]
max_metric_name_len = 32
max_labels_len = 64
max_span_name_len = 32
max_attrs_len = 64

[retention]
enabled = 1
max_segments = 2
max_records_per_segment = 2
rollup_every = 2
best_effort_retries = 1
critical_retries = 3
ttl_windows = 2
gc_batch = 1
";
        let limits = RuntimeLimits::parse_toml(toml).expect("valid limits parse");
        assert_eq!(limits.max_series_total, 8);
        assert_eq!(limits.rate_max_events_per_window, 3);
        assert_eq!(limits.max_attrs_len, 64);
        assert_eq!(limits.retention_max_segments, 2);
        assert_eq!(limits.retention_critical_retries, 3);
        assert_eq!(limits.retention_ttl_windows, 2);
    }

    #[test]
    fn test_parse_runtime_limits_rejects_invalid_values() {
        let toml = "\
[wire]
max_metric_name_len = 999
";
        assert_eq!(RuntimeLimits::parse_toml(toml), Err(ConfigError::InvalidValue));
    }

    #[test]
    fn test_runtime_limits_apply_series_cap() {
        let limits = RuntimeLimits {
            max_series_total: 1,
            max_series_per_metric: 1,
            ..RuntimeLimits::default()
        };
        let mut reg = Registry::new_with_limits(limits);
        assert!(reg.counter_inc(1, b"m.a", b"id=1", 1).is_ok());
        assert_eq!(reg.counter_inc(1, b"m.b", b"id=2", 1), Err(RejectReason::OverLimit));
    }

    #[test]
    fn test_retention_engine_rotates_ring_segments() {
        let limits = RuntimeLimits {
            retention_max_segments: 2,
            retention_max_records_per_segment: 2,
            retention_rollup_every: 10,
            ..RuntimeLimits::default()
        };
        let mut retention = RetentionEngine::new(limits);
        let a = retention.append(RetentionEventKind::Metric, b"r1").unwrap();
        let b = retention.append(RetentionEventKind::Metric, b"r2").unwrap();
        let c = retention.append(RetentionEventKind::Metric, b"r3").unwrap();
        assert_eq!(a.wal_slot, 0);
        assert_eq!(b.wal_slot, 0);
        assert_eq!(c.wal_slot, 1);
    }

    #[test]
    fn test_retention_engine_emits_rollup_deterministically() {
        let limits = RuntimeLimits { retention_rollup_every: 2, ..RuntimeLimits::default() };
        let mut retention = RetentionEngine::new(limits);
        let first = retention.append(RetentionEventKind::Metric, b"m1").unwrap();
        assert!(first.rollup_10s.is_none());
        let second = retention.append(RetentionEventKind::Span, b"s1").unwrap();
        let rollup = second.rollup_10s.map(|r| r.bytes).unwrap_or_default();
        assert!(rollup.starts_with(b"kind=10s\nwindow_id=1\nmetrics_total=1\nspans_total=1\n"));
        assert!(second.rollup_60s.is_none());
    }

    #[test]
    fn test_retention_engine_emits_60s_rollup_after_six_10s_windows() {
        let limits = RuntimeLimits { retention_rollup_every: 1, ..RuntimeLimits::default() };
        let mut retention = RetentionEngine::new(limits);
        let mut saw_60 = None;
        for i in 0..6 {
            let kind =
                if i % 2 == 0 { RetentionEventKind::Metric } else { RetentionEventKind::Span };
            let update = retention.append(kind, b"x").unwrap();
            if update.rollup_60s.is_some() {
                saw_60 = update.rollup_60s.map(|r| r.bytes);
            }
        }
        let rollup_60 = saw_60.unwrap_or_default();
        assert!(rollup_60.starts_with(b"kind=60s\nwindow_id=1\nmetrics_total=3\nspans_total=3\n"));
    }

    #[test]
    fn test_retention_engine_ttl_gc_is_bounded_deterministic() {
        let limits = RuntimeLimits {
            retention_rollup_every: 1,
            retention_ttl_windows: 2,
            retention_gc_batch: 1,
            ..RuntimeLimits::default()
        };
        let mut retention = RetentionEngine::new(limits);
        let _ = retention.append(RetentionEventKind::Metric, b"a").unwrap();
        let _ = retention.append(RetentionEventKind::Metric, b"b").unwrap();
        let c = retention.append(RetentionEventKind::Metric, b"c").unwrap();
        // ttl=2 and gc_batch=1 => exactly one stale rollup key per update.
        assert_eq!(c.gc_rollup_10s.len(), 1);
        assert_eq!(c.gc_rollup_10s[0], 1);
    }
}
