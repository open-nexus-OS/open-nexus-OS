use nexus_abi::yield_;
use nexus_ipc::KernelClient;
use nexus_metrics::client::MetricsClient;
use nexus_metrics::{
    DeterministicIdSource, SpanId, TraceId, STATUS_INVALID_ARGS as METRICS_STATUS_INVALID_ARGS,
    STATUS_NOT_FOUND as METRICS_STATUS_NOT_FOUND, STATUS_OK as METRICS_STATUS_OK,
    STATUS_OVER_LIMIT as METRICS_STATUS_OVER_LIMIT,
    STATUS_RATE_LIMITED as METRICS_STATUS_RATE_LIMITED,
};

pub(crate) fn metricsd_security_reject_probe(
    metricsd: &MetricsClient,
) -> core::result::Result<(), ()> {
    let sender = super::samgrd::fetch_sender_service_id_from_samgrd()
        .unwrap_or_else(|_| nexus_abi::service_id_from_name(b"selftest-client"));

    // Invalid args: span id must be sender-bound.
    let invalid = metricsd
        .span_start(
            SpanId((0xdead_beefu64 << 32) | 1),
            TraceId(1),
            SpanId(0),
            1,
            "selftest.invalid",
            b"",
        )
        .map_err(|_| ())?;
    if invalid != METRICS_STATUS_INVALID_ARGS {
        return Err(());
    }

    // Over limit: exceed per-metric series cap with unique labels.
    let mut over_limit_seen = false;
    for idx in 0..32u8 {
        let mut labels = [0u8; 8];
        labels[0] = b'i';
        labels[1] = b'd';
        labels[2] = b'=';
        labels[3] = b'0' + ((idx / 10) % 10);
        labels[4] = b'0' + (idx % 10);
        labels[5] = b'\n';
        let st = metricsd.counter_inc("selftest.cap", &labels[..6], 1).map_err(|_| ())?;
        if st == METRICS_STATUS_OVER_LIMIT {
            over_limit_seen = true;
            break;
        }
    }
    if !over_limit_seen {
        return Err(());
    }

    // Rate-limited: burst above sender budget.
    let mut rate_limited_seen = false;
    for idx in 0..96u64 {
        // Use a mutating op that is expected to return NOT_FOUND before budget exhaustion.
        // This keeps the reject proof deterministic without flooding logd with snapshot exports.
        let span_id = SpanId(((sender & 0xffff_ffff) << 32) | (0x1000 + idx));
        let st = metricsd.span_end(span_id, idx, 0, b"").map_err(|_| ())?;
        if st == METRICS_STATUS_RATE_LIMITED {
            rate_limited_seen = true;
            break;
        }
        if st != METRICS_STATUS_NOT_FOUND {
            return Err(());
        }
    }
    if !rate_limited_seen {
        return Err(());
    }

    // Allow sender budget window to elapse before validating a clean follow-up request.
    wait_rate_limit_window().map_err(|_| ())?;

    // Ensure sender-bound deterministic IDs would be accepted (sanity).
    let mut ids = DeterministicIdSource::new(sender);
    let span_id = ids.next_span_id();
    let trace_id = ids.next_trace_id();
    let start_status = metricsd
        .span_start(span_id, trace_id, SpanId(0), 10, "selftest.sanity", b"")
        .map_err(|_| ())?;
    if start_status != METRICS_STATUS_OK {
        return Err(());
    }
    let end_status = metricsd.span_end(span_id, 20, 0, b"").map_err(|_| ())?;
    if end_status != METRICS_STATUS_OK {
        return Err(());
    }
    Ok(())
}

pub(crate) fn wait_rate_limit_window() -> core::result::Result<(), ()> {
    const RATE_WINDOW_NS: u64 = 1_000_000_000;
    const MAX_SPINS: usize = 1_000_000;

    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(RATE_WINDOW_NS);
    for spin in 0..MAX_SPINS {
        let now = nexus_abi::nsec().map_err(|_| ())?;
        if now >= deadline {
            return Ok(());
        }
        if (spin & 0x3ff) == 0 {
            let _ = yield_();
        }
    }
    Err(())
}

pub(crate) fn metricsd_semantic_probe(
    metricsd: &MetricsClient,
    logd: &KernelClient,
) -> core::result::Result<(bool, bool, bool, bool, bool), ()> {
    let total_before = super::logd::logd_stats_total(logd).unwrap_or(0);
    let c0 =
        metricsd.counter_inc("selftest.counter", b"svc=selftest-client\n", 3).map_err(|_| ())?;
    let c1 =
        metricsd.counter_inc("selftest.counter", b"svc=selftest-client\n", 4).map_err(|_| ())?;
    for _ in 0..64 {
        let _ = yield_();
    }
    let mut counters_ok = c0 == METRICS_STATUS_OK && c1 == METRICS_STATUS_OK;

    let g0 = metricsd.gauge_set("selftest.gauge", b"svc=selftest-client\n", 7).map_err(|_| ())?;
    let g1 = metricsd.gauge_set("selftest.gauge", b"svc=selftest-client\n", -3).map_err(|_| ())?;
    for _ in 0..64 {
        let _ = yield_();
    }
    let mut gauges_ok = g0 == METRICS_STATUS_OK && g1 == METRICS_STATUS_OK;

    let h0 =
        metricsd.hist_observe("selftest.hist", b"svc=selftest-client\n", 1_000).map_err(|_| ())?;
    let h1 =
        metricsd.hist_observe("selftest.hist", b"svc=selftest-client\n", 12_000).map_err(|_| ())?;
    for _ in 0..64 {
        let _ = yield_();
    }
    let mut hist_ok = h0 == METRICS_STATUS_OK && h1 == METRICS_STATUS_OK;

    let sender = super::samgrd::fetch_sender_service_id_from_samgrd()
        .unwrap_or_else(|_| nexus_abi::service_id_from_name(b"selftest-client"));
    let mut ids = DeterministicIdSource::new(sender);
    let span_id = ids.next_span_id();
    let trace_id = ids.next_trace_id();
    let s0 = metricsd
        .span_start(span_id, trace_id, SpanId(0), 100, "selftest.span", b"phase=selftest\n")
        .map_err(|_| ())?;
    let s1 = metricsd.span_end(span_id, 180, 0, b"result=ok\n").map_err(|_| ())?;
    for _ in 0..64 {
        let _ = yield_();
    }
    let mut spans_ok = s0 == METRICS_STATUS_OK && s1 == METRICS_STATUS_OK;
    let retention_ok =
        super::logd::logd_query_contains_since_paged(logd, 0, b"retention wal verified")
            .unwrap_or(false);

    let total_after = super::logd::logd_stats_total(logd).unwrap_or(0);
    if total_after <= total_before {
        counters_ok = false;
        gauges_ok = false;
        hist_ok = false;
        spans_ok = false;
    }

    Ok((counters_ok, gauges_ok, hist_ok, spans_ok, retention_ok))
}
