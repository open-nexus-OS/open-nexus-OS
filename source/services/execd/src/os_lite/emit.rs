// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! execd's console lines and best-effort metrics — the ADR-0056 exit-truth and crash lines and the
//! metrics client calls that never block a spawn; split out of `os_lite.rs` under the structure
//! ratchet (TASK-0333 6a). Every line goes out WHOLE: routine lines through `debug_println` (the
//! verdict folding), the rest as one `nexus_abi::DebugLine` write (TASK-0333 6f-3 — the per-byte
//! emitters let another process's output tear a marker).

use nexus_abi::{nsec, DebugLine, ExitReason};
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
    DebugLine::new()
        .put(b"execd: exit pid=")
        .u64(u64::from(pid))
        .put(b" reason=")
        .str(reason.label())
        .put(b" code=")
        .i64(i64::from(code))
        .emit();
}

pub(super) fn emit_crash_marker(pid: u32, code: i32, name: &str) {
    DebugLine::new()
        .put(b"execd: crash report pid=")
        .u64(u64::from(pid))
        .put(b" code=")
        .i64(i64::from(code))
        .put(b" name=")
        .str(name)
        .emit();
}

/// The console name of an IPC error (the crash-log sink's failure line).
pub(super) fn ipc_error_label(err: nexus_abi::IpcError) -> &'static str {
    match err {
        nexus_abi::IpcError::PermissionDenied => "PermissionDenied",
        nexus_abi::IpcError::NoSuchEndpoint => "NoSuchEndpoint",
        nexus_abi::IpcError::QueueFull => "QueueFull",
        nexus_abi::IpcError::QueueEmpty => "QueueEmpty",
        nexus_abi::IpcError::NoSpace => "NoSpace",
        nexus_abi::IpcError::TooBig => "TooBig",
        nexus_abi::IpcError::TimedOut => "TimedOut",
        nexus_abi::IpcError::PeerClosed => "PeerClosed",
        nexus_abi::IpcError::Frozen => "Frozen",
        nexus_abi::IpcError::Unsupported => "Unsupported",
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
