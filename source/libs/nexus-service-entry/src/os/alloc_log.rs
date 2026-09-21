// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Allocation/verbosity logging helpers of the OS service entry (bump-heap init,
//! alloc events, failures, corruption, zeroed allocs) in their `alloc-log` / plain cfg pairs.
//! Split out of `lib.rs` (module-size ratchet); behaviour unchanged.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Internal (`pub(super)`)
//! TEST_COVERAGE: Exercised by every OS service boot (`!alloc-*` probes)

use super::*;
use core::sync::atomic::{AtomicU8, Ordering};

pub(super) fn log_alloc_init_debug(base: usize, end: usize) {
    if !probe_logs_enabled() {
        return;
    }
    debug_write_bytes(b"alloc-init svc=");
    debug_write_str(service_name());
    debug_write_bytes(b" base=0x");
    debug_write_hex(base);
    debug_write_bytes(b" end=0x");
    debug_write_hex(end);
    debug_write_byte(b'\n');
}

pub(super) fn log_alloc_event_debug(
    size: usize,
    align: usize,
    cur_before: usize,
    aligned: usize,
    cur_after: usize,
    end: usize,
    result: usize,
) {
    if !probe_logs_enabled() {
        return;
    }
    debug_write_bytes(b"alloc svc=");
    debug_write_str(service_name());
    debug_write_bytes(b" size=0x");
    debug_write_hex(size);
    debug_write_bytes(b" align=0x");
    debug_write_hex(align);
    debug_write_bytes(b" cur_before=0x");
    debug_write_hex(cur_before);
    debug_write_bytes(b" aligned=0x");
    debug_write_hex(aligned);
    debug_write_bytes(b" cur_after=0x");
    debug_write_hex(cur_after);
    debug_write_bytes(b" end=0x");
    debug_write_hex(end);
    debug_write_bytes(b" result=0x");
    debug_write_hex(result);
    debug_write_byte(b'\n');
}

pub(super) fn log_alloc_failure(
    site: &str,
    size: usize,
    align: usize,
    heap_start: usize,
    heap_end: usize,
    cur_before: usize,
    cur_after: usize,
) {
    debug_write_bytes(b"alloc-fail svc=");
    debug_write_str(service_name());
    debug_write_bytes(b" site=");
    debug_write_str(site);
    debug_write_bytes(b" size=0x");
    debug_write_hex(size);
    debug_write_bytes(b" align=0x");
    debug_write_hex(align);
    debug_write_bytes(b" heap_start=0x");
    debug_write_hex(heap_start);
    debug_write_bytes(b" heap_end=0x");
    debug_write_hex(heap_end);
    debug_write_bytes(b" cur_before=0x");
    debug_write_hex(cur_before);
    debug_write_bytes(b" cur_after=0x");
    debug_write_hex(cur_after);
    debug_write_byte(b'\n');
    #[cfg(feature = "alloc-log")]
    {
        nexus_log::error("alloc", |line| {
            line.text("alloc-fail svc=");
            line.text(service_name());
            line.text(" site=");
            line.text(site);
            line.text(" size=");
            line.hex(size as u64);
            line.text(" align=");
            line.hex(align as u64);
            line.text(" heap_start=");
            line.hex(heap_start as u64);
            line.text(" heap_end=");
            line.hex(heap_end as u64);
            line.text(" cur_before=");
            line.hex(cur_before as u64);
            line.text(" cur_after=");
            line.hex(cur_after as u64);
        });
    }
}

pub(super) fn log_alloc_zero_debug(size: usize, align: usize, result: usize, phase: &str) {
    if !probe_logs_enabled() {
        return;
    }
    debug_write_bytes(b"alloc-zero svc=");
    debug_write_str(service_name());
    debug_write_bytes(b" ");
    debug_write_str(phase);
    debug_write_bytes(b" size=0x");
    debug_write_hex(size);
    debug_write_bytes(b" align=0x");
    debug_write_hex(align);
    debug_write_bytes(b" result=0x");
    debug_write_hex(result);
    debug_write_byte(b'\n');
}

pub(super) fn log_alloc_corruption(
    addr: usize,
    size: usize,
    heap_start: usize,
    heap_end: usize,
    cur_before: usize,
    cur_after: usize,
) {
    debug_write_bytes(b"alloc-corrupt addr=0x");
    debug_write_hex(addr);
    debug_write_bytes(b" size=0x");
    debug_write_hex(size);
    debug_write_bytes(b" heap_start=0x");
    debug_write_hex(heap_start);
    debug_write_bytes(b" heap_end=0x");
    debug_write_hex(heap_end);
    debug_write_bytes(b" cur_before=0x");
    debug_write_hex(cur_before);
    debug_write_bytes(b" cur_after=0x");
    debug_write_hex(cur_after);
    debug_write_byte(b'\n');
    #[cfg(feature = "alloc-log")]
    {
        nexus_log::error("alloc", |line| {
            line.text("alloc-corrupt addr=");
            line.hex(addr as u64);
            line.text(" size=");
            line.hex(size as u64);
            line.text(" heap_start=");
            line.hex(heap_start as u64);
            line.text(" heap_end=");
            line.hex(heap_end as u64);
        });
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(super) fn probe_logs_enabled() -> bool {
    static STATE: AtomicU8 = AtomicU8::new(2);
    match STATE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => {
            let enabled = option_env!("INIT_LITE_LOG_TOPICS")
                .map(|spec| spec.split(',').any(|token| token.trim().eq_ignore_ascii_case("probe")))
                .unwrap_or(false);
            STATE.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
            enabled
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub(super) fn probe_logs_enabled() -> bool {
    false
}

/// One-time per-process verbosity setup from the build-time `INIT_LITE_LOG_TOPICS` knob.
/// A `trace` or `verbose` token re-enables developer breadcrumbs (`debug_trace`) and lifts
/// the `nexus_log` floor to Debug; without it the process stays quiet (Warn/Info). This is
/// the "flags" path: detail is one rebuild-time token away, never deleted.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(super) fn configure_verbosity_from_env() {
    if crate::verbose_build() {
        nexus_abi::set_debug_trace(true);
        #[cfg(feature = "alloc-log")]
        nexus_log::set_max_level(nexus_log::Level::Debug);
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub(super) fn configure_verbosity_from_env() {}

#[cfg(feature = "alloc-log")]
pub(super) fn log_alloc_init(base: usize, end: usize) {
    if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
        return;
    }
    log_alloc_init_debug(base, end);
    nexus_log::info("alloc", |line| {
        line.text("alloc-init base=");
        line.hex(base as u64);
        line.text(" end=");
        line.hex(end as u64);
    });
}

#[cfg(not(feature = "alloc-log"))]
pub(super) fn log_alloc_init(base: usize, end: usize) {
    if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
        return;
    }
    log_alloc_init_debug(base, end);
}

#[cfg(feature = "alloc-log")]
pub(super) fn log_alloc_event(
    size: usize,
    align: usize,
    cur_before: usize,
    aligned: usize,
    cur_after: usize,
    end: usize,
    result: usize,
) {
    if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
        return;
    }
    log_alloc_event_debug(size, align, cur_before, aligned, cur_after, end, result);
    nexus_log::info("alloc", |line| {
        line.text("alloc size=");
        line.hex(size as u64);
        line.text(" align=");
        line.hex(align as u64);
        line.text(" cur_before=");
        line.hex(cur_before as u64);
        line.text(" aligned=");
        line.hex(aligned as u64);
        line.text(" cur_after=");
        line.hex(cur_after as u64);
        line.text(" end=");
        line.hex(end as u64);
        line.text(" result=");
        line.hex(result as u64);
    });
}

#[cfg(not(feature = "alloc-log"))]
pub(super) fn log_alloc_event(
    size: usize,
    align: usize,
    cur_before: usize,
    aligned: usize,
    cur_after: usize,
    end: usize,
    result: usize,
) {
    if ALLOC_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_LOG_LIMIT {
        return;
    }
    log_alloc_event_debug(size, align, cur_before, aligned, cur_after, end, result);
}

#[cfg(feature = "alloc-log")]
pub(super) fn log_alloc_zeroed(size: usize, align: usize, result: usize) {
    if ALLOC_ZERO_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_ZERO_LOG_LIMIT {
        return;
    }
    log_alloc_zero_debug(size, align, result, "pre");
    nexus_log::info("alloc", |line| {
        line.text("alloc-zero size=");
        line.hex(size as u64);
        line.text(" align=");
        line.hex(align as u64);
        line.text(" result=");
        line.hex(result as u64);
    });
}

#[cfg(not(feature = "alloc-log"))]
pub(super) fn log_alloc_zeroed(size: usize, align: usize, result: usize) {
    if ALLOC_ZERO_LOG_COUNT.fetch_add(1, Ordering::Relaxed) >= ALLOC_ZERO_LOG_LIMIT {
        return;
    }
    log_alloc_zero_debug(size, align, result, "pre");
}
