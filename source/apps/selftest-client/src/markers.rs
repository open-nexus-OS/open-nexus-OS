// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: UART marker helpers for selftest-client (no-alloc, deterministic output)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Marker-driven via just test-os
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#![allow(clippy::missing_docs_in_private_items)]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use nexus_abi::debug_putc;

// ─── Verdict aggregation (the minimal-but-powerful console) ──────────────────────────────────
//
// In INTERACTIVE boots the per-marker ladder is noise — a human wants one aggregated line per
// group: `[ts]  OK  selftest:bringup  12/12  11ms`, expanding only on a problem. So in verdict
// mode `emit_line` TALLIES each marker instead of printing it, and the phase wrapper emits a
// single `emit_verdict`. A failing marker (`FAIL`) is ALWAYS printed — a problem is never hidden.
// In the proof harness verdict mode is OFF: markers print in full so `verify-uart` stays
// deterministic against the proof-manifest SSOT. Same markers, two views.

/// When true, routine markers are tallied (not printed); the group verdict is the console output.
static CONSOLE_VERDICT: AtomicBool = AtomicBool::new(false);
/// Running count of markers emitted (the aggregation numerator).
static MARKER_TALLY: AtomicU32 = AtomicU32::new(0);
/// Running count of markers that signalled a failure (`FAIL` literal).
static MARKER_FAILS: AtomicU32 = AtomicU32::new(0);

/// Soft-real-time budget per group (ms). A verdict slower than this is flagged `WARN … slow`, so
/// a sluggish group (e.g. a 12 s display bring-up) jumps out of an otherwise quiet `OK` column.
pub const GROUP_SLOW_BUDGET_MS: u64 = 250;

/// Enable/disable verdict mode (on for interactive boots; off for the proof harness).
pub fn set_console_verdict_mode(on: bool) {
    CONSOLE_VERDICT.store(on, Ordering::Relaxed);
}

/// Snapshot of (markers, failures) so far — the phase wrapper diffs this around each group.
pub fn marker_counts() -> (u32, u32) {
    (MARKER_TALLY.load(Ordering::Relaxed), MARKER_FAILS.load(Ordering::Relaxed))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

// P4-04: marker literals live in the sibling `markers_generated` module
// (built unconditionally so the host pfad reuses the same SSOT). Re-export
// here so `crate::markers::M_*` is the canonical reference path from emit
// sites under `os_lite/**`.
#[allow(unused_imports)]
pub(crate) use crate::markers_generated::*;

/// Emit a single byte over UART.
pub fn emit_byte(byte: u8) {
    let _ = debug_putc(byte);
}

/// Emit raw bytes over UART.
pub fn emit_bytes(bytes: &[u8]) {
    for &b in bytes {
        emit_byte(b);
    }
}

/// Emit a newline-terminated marker line over UART. Always tallied; in verdict mode a routine
/// marker is suppressed (folded into its group verdict) while a `FAIL` is always shown.
pub fn emit_line(message: &str) {
    let bytes = message.as_bytes();
    MARKER_TALLY.fetch_add(1, Ordering::Relaxed);
    let failed = contains(bytes, b"FAIL");
    if failed {
        MARKER_FAILS.fetch_add(1, Ordering::Relaxed);
    }
    if CONSOLE_VERDICT.load(Ordering::Relaxed) && !failed {
        return;
    }
    emit_bytes(bytes);
    emit_byte(b'\n');
}

/// Emit `value` right-padded with leading zeros to `width` digits (no alloc).
fn emit_zero_padded(value: u64, width: usize) {
    let mut buf = [b'0'; 20];
    let mut v = value;
    let mut n = 0usize;
    loop {
        buf[buf.len() - 1 - n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
        if v == 0 {
            break;
        }
    }
    let digits = n.max(width);
    emit_bytes(&buf[buf.len() - digits..]);
}

/// Emit `s` then spaces up to `width` (left-aligned column, no alloc).
fn emit_col(s: &[u8], width: usize) {
    emit_bytes(s);
    for _ in s.len()..width {
        emit_byte(b' ');
    }
}

/// Emit the boot-relative timestamp as `[ssss.uuuuuu]` (seconds + microseconds) — the leading
/// column of every console line, so the whole boot timeline (and any gap) is visible.
fn emit_timestamp() {
    let ns = nexus_abi::nsec().unwrap_or(0);
    let secs = ns / 1_000_000_000;
    emit_byte(b'[');
    // RFC-0068: match `nexus_event::render_verdict_line`'s `[{:>5}.{:06}]` so the kernel, service
    // and selftest verdict timestamps share ONE column-aligned format (no padded-vs-unpadded split).
    // Right-align the seconds in a 5-wide field.
    let digits = {
        let mut d = 1u64;
        let mut v = secs;
        while v >= 10 {
            v /= 10;
            d += 1;
        }
        d
    };
    for _ in digits..5 {
        emit_byte(b' ');
    }
    emit_u64(secs);
    emit_byte(b'.');
    emit_zero_padded((ns % 1_000_000_000) / 1000, 6);
    emit_byte(b']');
}

/// Emit one aggregated group verdict in the unified console grid:
/// `[ts]  TAG    group              n/n      Xms  [slow]`. `TAG` is `OK` (pass, fast), `WARN`
/// (pass but over the soft-real-time budget) or `ERROR` (any failure) — so problems are loud and
/// a slow group is visible by both its tag and its right-aligned duration.
pub fn emit_verdict(group: &str, passed: u32, total: u32, ms: u64) {
    let ok = passed == total;
    let slow = ms >= GROUP_SLOW_BUDGET_MS;
    let tag: &[u8] = if !ok {
        b"ERROR"
    } else if slow {
        b"WARN"
    } else {
        b"OK"
    };
    emit_timestamp();
    emit_bytes(b"  ");
    emit_col(tag, 7);
    emit_col(group.as_bytes(), 22);
    emit_u64(passed as u64);
    emit_byte(b'/');
    emit_u64(total as u64);
    emit_bytes(b"   ");
    emit_u64(ms);
    emit_bytes(b"ms");
    if slow && ok {
        emit_bytes(b"  slow");
    }
    emit_byte(b'\n');
}

/// Emit `prefix` followed by `value` as decimal u8 and a newline.
// NOTE: keep this module minimal and only include helpers currently used in
// the marker-driven OS tests (RFC-0003: avoid drifting, duplicated helper sets).

/// Emit an unsigned decimal u64 (no allocation).
pub fn emit_u64(mut value: u64) {
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
    emit_bytes(&buf[idx..]);
}

/// Emit a signed decimal i64 (no allocation).
pub fn emit_i64(value: i64) {
    if value < 0 {
        emit_byte(b'-');
        emit_u64((-value) as u64);
    } else {
        emit_u64(value as u64);
    }
}

/// Emit a fixed-width 16-nybble hex u64 (no allocation).
pub fn emit_hex_u64(mut value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [0u8; 16];
    for idx in (0..buf.len()).rev() {
        buf[idx] = HEX[(value & 0xF) as usize];
        value >>= 4;
    }
    emit_bytes(&buf);
}
