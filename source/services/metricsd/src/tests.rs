// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests for the metrics registry, the span checks and the snapshot cadence.

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
    assert_eq!(reg.counter_inc(1, b"boot.events", b"id=overflow", 1), Err(RejectReason::OverLimit));
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
    let limits =
        RuntimeLimits { max_series_total: 1, max_series_per_metric: 1, ..RuntimeLimits::default() };
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
        let kind = if i % 2 == 0 { RetentionEventKind::Metric } else { RetentionEventKind::Span };
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

/// The keystroke privacy rule: a counter a user action bumps (a registry listing per keystroke
/// of a live search) must not print a line per increment — only on reaching or crossing a power
/// of two, whatever the step.
#[test]
fn snapshots_print_when_a_power_of_two_is_reached_or_crossed() {
    let printed: Vec<u64> = (1..=40u64).filter(|v| snapshot_due(v - 1, *v)).collect();
    assert_eq!(printed, [1, 2, 4, 8, 16, 32], "one at a time");
    assert!(snapshot_due(0, 3), "the first step prints");
    assert!(snapshot_due(3, 7), "3 → 7 crosses 4 (the selftest's +3, +4)");
    assert!(!snapshot_due(4, 7), "within one power of two: silent");
}

#[test]
fn test_reject_a_snapshot_line_per_increment() {
    let lines = (1..=1000u64).filter(|v| snapshot_due(v - 1, *v)).count();
    assert_eq!(lines, 10, "a thousand increments print ten lines, not a thousand");
    assert!(!snapshot_due(5, 5), "no change, no line");
    assert!(!snapshot_due(0, 0));
}
