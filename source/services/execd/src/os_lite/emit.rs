// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! execd's console lines and best-effort metrics — the alloc-free number and marker emitters
//! (ADR-0056 exit-truth and crash lines) and the metrics client calls that never block a
//! spawn; split out of `os_lite.rs` under the structure ratchet (TASK-0333 6a).

use nexus_abi::{debug_putc, nsec, ExitReason};
use nexus_metrics::client::MetricsClient;
use nexus_metrics::SpanId;

pub(crate) fn emit_line(message: &str) {
    // Verdict folding: pre-`ready` markers tally into `execd N/N`; post-`ready` runtime lines fold
    // into recall-only detail (`NEXUS_LOG_EXPAND=execd`). Failures & proof boots print live & raw.
    // One atomic `debug_write` (via `debug_println`, which also owns the verdict
    // folding): the per-byte `debug_putc` fallback tears mid-line against the
    // kernel's locked log records and DROPS the tail — a torn ready marker is a
    // red ladder gate.
    let _ = nexus_abi::debug_println(message);
}

/// ADR-0056 exit-truth line — one per reaped child, exactly once:
/// `execd: exit pid=<p> reason=<label> code=<c>`.
pub(super) fn emit_exit_reason_marker(pid: u32, code: i32, reason: ExitReason) {
    emit_line_no_nl("execd: exit pid=");
    emit_u64(pid as u64);
    emit_line_no_nl(" reason=");
    emit_line_no_nl(reason.label());
    emit_line_no_nl(" code=");
    emit_i64(code as i64);
    let _ = debug_putc(b'\n');
}

pub(super) fn emit_crash_marker(pid: u32, code: i32, name: &str) {
    emit_line_no_nl("execd: crash report pid=");
    emit_u64(pid as u64);
    emit_line_no_nl(" code=");
    emit_i64(code as i64);
    emit_line_no_nl(" name=");
    for b in name.as_bytes() {
        let _ = debug_putc(*b);
    }
    let _ = debug_putc(b'\n');
}

pub(super) fn emit_line_no_nl(message: &str) {
    for byte in message.as_bytes().iter().copied() {
        let _ = debug_putc(byte);
    }
}

pub(super) fn metrics_counter_inc_best_effort(name: &str) {
    let Ok(metrics) = MetricsClient::new() else {
        return;
    };
    let _ = metrics.counter_inc(name, b"svc=execd\n", 1);
}

pub(super) fn metrics_span_start_best_effort(
    span_id: SpanId,
    trace_id: nexus_metrics::TraceId,
    name: &str,
    attrs: &[u8],
) {
    let Ok(metrics) = MetricsClient::new() else {
        return;
    };
    let _ = metrics.span_start(span_id, trace_id, SpanId(0), nsec().ok().unwrap_or(0), name, attrs);
}

pub(super) fn metrics_span_end_best_effort(span_id: SpanId, end_ns: u64, status: u8, attrs: &[u8]) {
    let Ok(metrics) = MetricsClient::new() else {
        return;
    };
    let _ = metrics.span_end(span_id, end_ns, status, attrs);
}

pub(super) fn emit_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut idx = buf.len();
    if value == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else {
        while value != 0 {
            idx -= 1;
            buf[idx] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    for &b in &buf[idx..] {
        let _ = debug_putc(b);
    }
}

pub(super) fn emit_i64(value: i64) {
    if value < 0 {
        let _ = debug_putc(b'-');
        emit_u64((-value) as u64);
    } else {
        emit_u64(value as u64);
    }
}
