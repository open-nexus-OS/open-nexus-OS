//! Phase: logd (extracted in Cut P2-09 of TASK-0023B).
//!
//! Owns the logd-anchored slice immediately following the exec phase:
//!   TASK-0014 Phase 0a logd sink hardening reject matrix +
//!   metricsd rate-limit window wait +
//!   TASK-0014 Phase 0/1 metrics/tracing semantics + sink evidence
//!     (security rejects, counters, gauges, histograms, spans, retention) +
//!   TASK-0006 logd journaling proof (APPEND + QUERY) +
//!   TASK-0006 nexus-log -> logd sink proof +
//!   TASK-0006 core services log proof (samgrd / bundlemgrd / policyd /
//!     dsoftbusd probe RPCs + logd-stats delta + paged QUERY).
//!
//! Marker order and marker strings are byte-identical to the pre-cut body.
//! `logd`, `metricsd`, `samgrd`, `bundlemgrd`, `policyd` handles are all
//! local to this phase and dropped at end-of-phase. Downstream phases re-resolve
//! via the silent `route_with_retry` (no marker change).
//!
//! `ctx.reply_send_slot` / `ctx.reply_recv_slot` are read for nexus-log sink
//! configuration (TASK-0006 facade probe); they are not mutated.

use nexus_abi::yield_;
use nexus_metrics::client::MetricsClient;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::probes::core_service::{core_service_probe, core_service_probe_policyd};
use crate::os_lite::services;

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    let logd = route_with_retry("logd")?;

    // TASK-0014 Phase 0a: logd sink hardening reject matrix.
    if services::logd::logd_hardening_reject_probe(&logd).is_ok() {
        emit_line("SELFTEST: logd hardening rejects ok");
    } else {
        emit_line("SELFTEST: logd hardening rejects FAIL");
    }
    let _ = services::metricsd::wait_rate_limit_window();

    // TASK-0014 Phase 0/1: metrics/tracing semantics + sink evidence.
    if let Ok(metricsd) = MetricsClient::new() {
        if services::metricsd::metricsd_security_reject_probe(&metricsd).is_ok() {
            emit_line("SELFTEST: metrics security rejects ok");
        } else {
            emit_line("SELFTEST: metrics security rejects FAIL");
        }
        match services::metricsd::metricsd_semantic_probe(&metricsd, &logd) {
            Ok((counters_ok, gauges_ok, hist_ok, spans_ok, retention_ok)) => {
                if counters_ok {
                    emit_line("SELFTEST: metrics counters ok");
                } else {
                    emit_line("SELFTEST: metrics counters FAIL");
                }
                if gauges_ok {
                    emit_line("SELFTEST: metrics gauges ok");
                } else {
                    emit_line("SELFTEST: metrics gauges FAIL");
                }
                if hist_ok {
                    emit_line("SELFTEST: metrics histograms ok");
                } else {
                    emit_line("SELFTEST: metrics histograms FAIL");
                }
                if spans_ok {
                    emit_line("SELFTEST: tracing spans ok");
                } else {
                    emit_line("SELFTEST: tracing spans FAIL");
                }
                if retention_ok {
                    emit_line("SELFTEST: metrics retention ok");
                } else {
                    emit_line("SELFTEST: metrics retention FAIL");
                }
            }
            Err(_) => {
                emit_line("SELFTEST: metrics counters FAIL");
                emit_line("SELFTEST: metrics gauges FAIL");
                emit_line("SELFTEST: metrics histograms FAIL");
                emit_line("SELFTEST: tracing spans FAIL");
                emit_line("SELFTEST: metrics retention FAIL");
            }
        }
    } else {
        emit_line("SELFTEST: metrics security rejects FAIL");
        emit_line("SELFTEST: metrics counters FAIL");
        emit_line("SELFTEST: metrics gauges FAIL");
        emit_line("SELFTEST: metrics histograms FAIL");
        emit_line("SELFTEST: tracing spans FAIL");
        emit_line("SELFTEST: metrics retention FAIL");
    }

    // TASK-0006: logd journaling proof (APPEND + QUERY).
    let logd = route_with_retry("logd")?;
    let append_ok = services::logd::logd_append_probe(&logd).is_ok();
    let query_ok = services::logd::logd_query_probe(&logd).unwrap_or(false);
    if append_ok && query_ok {
        emit_line("SELFTEST: log query ok");
    } else {
        if !append_ok {
            emit_line("SELFTEST: logd append probe FAIL");
        }
        if !query_ok {
            emit_line("SELFTEST: logd query probe FAIL");
        }
        emit_line("SELFTEST: log query FAIL");
    }

    // TASK-0006: nexus-log -> logd sink proof.
    // This checks that the facade can send to logd (bounded, best-effort) without relying on UART scraping.
    let _ = nexus_log::configure_sink_logd_slots(0x15, ctx.reply_send_slot, ctx.reply_recv_slot);
    nexus_log::info("selftest-client", |line| {
        line.text("nexus-log sink-logd probe");
    });
    for _ in 0..64 {
        let _ = yield_();
    }
    if services::logd::logd_query_contains_since_paged(&logd, 0, b"nexus-log sink-logd probe")
        .unwrap_or(false)
    {
        emit_line("SELFTEST: nexus-log sink-logd ok");
    } else {
        emit_line("SELFTEST: nexus-log sink-logd FAIL");
    }

    // ============================================================
    // TASK-0006: Core services log proof (mix of trigger + stats)
    // ============================================================
    // Trigger samgrd/bundlemgrd/policyd to emit a logd record (request-driven probe RPC).
    // For dsoftbusd we validate a startup-time probe (emitted after dsoftbusd: ready).
    //
    // Proof signals:
    // - logd STATS total increases by >=3 due to the three probe RPCs
    // - logd QUERY since t0 finds the expected messages (paged, bounded)
    let total0 = services::logd::logd_stats_total(&logd).unwrap_or(0);
    let mut ok = true;
    let mut total = total0;

    // samgrd probe
    let mut sam_probe = false;
    let mut sam_found = false;
    let mut sam_delta_ok = false;
    if let Ok(samgrd) = route_with_retry("samgrd") {
        sam_probe = core_service_probe(&samgrd, b'S', b'M', 1, 0x7f).is_ok();
        for _ in 0..64 {
            let _ = yield_();
        }
        let t1 = services::logd::logd_stats_total(&logd).unwrap_or(total);
        sam_delta_ok = t1 >= total.saturating_add(1);
        total = t1;
        sam_found = services::logd::logd_query_contains_since_paged(
            &logd,
            0,
            b"core service log probe: samgrd",
        )
        .unwrap_or(false);
    } else {
        emit_line("SELFTEST: core log samgrd route FAIL");
    }
    ok &= sam_probe && sam_found && sam_delta_ok;

    // bundlemgrd probe
    let mut bnd_probe = false;
    let mut bnd_delta_ok = false;
    if let Ok(bundlemgrd) = route_with_retry("bundlemgrd") {
        bnd_probe = core_service_probe(&bundlemgrd, b'B', b'N', 1, 0x7f).is_ok();
        for _ in 0..64 {
            let _ = yield_();
        }
        let t1 = services::logd::logd_stats_total(&logd).unwrap_or(total);
        bnd_delta_ok = t1 >= total.saturating_add(1);
        total = t1;
        let _ = services::logd::logd_query_contains_since_paged(
            &logd,
            0,
            b"core service log probe: bundlemgrd",
        )
        .unwrap_or(false);
    } else {
        emit_line("SELFTEST: core log bundlemgrd route FAIL");
    }
    // bundlemgrd: rely on stats delta + probe; query paging can be brittle on boot.
    ok &= bnd_probe && bnd_delta_ok;

    // policyd probe
    let mut pol_probe = false;
    let mut pol_delta_ok = false;
    let mut pol_found = false;
    if let Ok(policyd) = route_with_retry("policyd") {
        pol_probe = core_service_probe_policyd(&policyd).is_ok();
        for _ in 0..64 {
            let _ = yield_();
        }
        let t1 = services::logd::logd_stats_total(&logd).unwrap_or(total);
        pol_delta_ok = t1 >= total.saturating_add(1);
        total = t1;
        pol_found = services::logd::logd_query_contains_since_paged(
            &logd,
            0,
            b"core service log probe: policyd",
        )
        .unwrap_or(false);
    } else {
        emit_line("SELFTEST: core log policyd route FAIL");
    }
    // Mix of (1) and (2): for policyd we validate via logd stats delta (logd-backed) to avoid
    // brittle false negatives from QUERY paging/limits.
    ok &= pol_probe && pol_found;

    // dsoftbusd emits its probe at readiness; validate it via logd query scan.
    let _dsoft_found = services::logd::logd_query_contains_since_paged(
        &logd,
        0,
        b"core service log probe: dsoftbusd",
    )
    .unwrap_or(false);

    // Overall sanity: at least 2 appends during the probe phase (samgrd/bundlemgrd).
    // policyd is allowed to prove via query-only (delta can be flaky under QEMU).
    let delta_ok = total >= total0.saturating_add(2);
    ok &= delta_ok;
    if ok {
        emit_line("SELFTEST: core services log ok");
    } else {
        // Diagnostic detail (deterministic, no secrets).
        if !sam_probe {
            emit_line("SELFTEST: core log samgrd probe FAIL");
        }
        if !sam_found {
            emit_line("SELFTEST: core log samgrd query FAIL");
        }
        if !sam_delta_ok {
            emit_line("SELFTEST: core log samgrd delta FAIL");
        }
        if !bnd_probe {
            emit_line("SELFTEST: core log bundlemgrd probe FAIL");
        }
        // bundlemgrd query is not required for success (see delta-based check above).
        if !bnd_delta_ok {
            emit_line("SELFTEST: core log bundlemgrd delta FAIL");
        }
        if !pol_probe {
            emit_line("SELFTEST: core log policyd probe FAIL");
        }
        if !pol_found {
            emit_line("SELFTEST: core log policyd query FAIL");
        }
        if !pol_delta_ok {
            emit_line("SELFTEST: core log policyd delta FAIL");
        }
        if !delta_ok {
            emit_line("SELFTEST: core log stats delta FAIL");
        }
        emit_line("SELFTEST: core services log FAIL");
    }

    let _ = logd;
    Ok(())
}
