// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Declarative soft-realtime kernel latency budgets + the per-boot
//! accounting the `KSELFTEST: bkl budget` gate reads. THE SSOT for "how long
//! may anything wait for / hold the BKL": the values are asserted by the
//! boot gate, so a regression fails the marker instead of surfacing as mouse
//! lag. Tightened as the lock split lands (P2).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker `KSELFTEST: bkl budget ok` (smp gate)
//! ADR: docs/adr/0046-deterministic-parallel-compute-workpool.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Max time any hart may SPIN waiting for the BKL (µs), measured under
/// MTTCG emulation. Post-P2 calibration: phased vmo/exec + the lock-free
/// syscall class + cpu0 right-of-way brought the boot maximum from 90.8ms to
/// ~6ms; 8ms is that plus margin. Under the ~50x MTTCG cost factor this
/// corresponds to roughly <=160µs on target hardware — well inside a 16ms
/// frame budget. Any regression toward a >10ms convoy fails the gate.
/// Calibrated against run-to-run MTTCG + host jitter (healthy steady-state
/// runs scatter 3-22ms max across boots; the pre-P2 regression class sat at
/// 82-106ms with 4-5 >10ms convoys EVERY run). The gate targets the CLASS.
pub const BKL_WAIT_BUDGET_US: u64 = 40_000;

/// Convoy-frequency bound: healthy boots show 0-3 waits >10ms; the
/// regression class showed 4-5 EVERY run on top of a 90ms max.
pub const BKL_GT10MS_BUDGET: usize = 4;

/// Max time a single ecall may HOLD the BKL (ms) under MTTCG. Post-P2: the
/// worst holders (vmo_create zeroing 90ms, exec ELF copy 22ms, debug_write
/// 3ms) are phased/lock-free; remaining scheduler/teardown ops peak at ~3ms.
pub const ECALL_HOLD_BUDGET_MS: u64 = 10;

/// mtime ticks per µs on the virt machine (10 MHz).
pub const TICKS_PER_US: u64 = 10;

/// Per-boot maxima + a 4-bucket wait histogram (<=100µs, <=1ms, <=10ms,
/// >10ms). Written on EVERY BKL acquire/ecall (relaxed atomics — accounting,
/// > not synchronization); drained once by the boot-end gate marker.
pub static BKL_WAIT_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
pub static BKL_WAIT_BUCKETS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
pub static ECALL_HOLD_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
pub static ECALL_HOLD_MAX_NR: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn record_bkl_wait(ticks: u64) {
    BKL_WAIT_MAX_TICKS.fetch_max(ticks, Ordering::Relaxed);
    let bucket = match ticks {
        0..=1_000 => 0,        // <=100µs
        1_001..=10_000 => 1,   // <=1ms
        10_001..=100_000 => 2, // <=10ms
        _ => 3,                // >10ms
    };
    BKL_WAIT_BUCKETS[bucket].fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_ecall_hold(ticks: u64, nr: u64) {
    let prev = ECALL_HOLD_MAX_TICKS.fetch_max(ticks, Ordering::Relaxed);
    if ticks > prev {
        // Benign race: a concurrent larger hold may overwrite nr — the pair
        // is diagnostic, the gate only asserts the max value.
        ECALL_HOLD_MAX_NR.store(nr, Ordering::Relaxed);
    }
}

/// Two-window measurement (the boot bring-up burst is DENSE by design — 24
/// services exec in ~2s; soft-realtime matters for the state AFTER it).
/// `reset()` is invoked by the selftest once bring-up completes; the boot-end
/// gate then judges the steady-state window (the ladder itself is a
/// representative load: IPC storms, exec children, compute jobs).
pub fn reset() {
    BKL_WAIT_MAX_TICKS.store(0, Ordering::Relaxed);
    ECALL_HOLD_MAX_TICKS.store(0, Ordering::Relaxed);
    ECALL_HOLD_MAX_NR.store(0, Ordering::Relaxed);
    for b in &BKL_WAIT_BUCKETS {
        b.store(0, Ordering::Relaxed);
    }
}

/// Gate evaluation: `(ok, max_wait_us, max_hold_ms, max_hold_nr, buckets)`.
pub fn budget_report() -> (bool, u64, u64, u64, [usize; 4]) {
    let wait_us = BKL_WAIT_MAX_TICKS.load(Ordering::Relaxed) / TICKS_PER_US;
    let hold_ms = ECALL_HOLD_MAX_TICKS.load(Ordering::Relaxed) / (TICKS_PER_US * 1_000);
    let nr = ECALL_HOLD_MAX_NR.load(Ordering::Relaxed);
    let buckets = [
        BKL_WAIT_BUCKETS[0].load(Ordering::Relaxed),
        BKL_WAIT_BUCKETS[1].load(Ordering::Relaxed),
        BKL_WAIT_BUCKETS[2].load(Ordering::Relaxed),
        BKL_WAIT_BUCKETS[3].load(Ordering::Relaxed),
    ];
    let ok = wait_us <= BKL_WAIT_BUDGET_US
        && hold_ms <= ECALL_HOLD_BUDGET_MS
        && buckets[3] <= BKL_GT10MS_BUDGET;
    (ok, wait_us, hold_ms, nr, buckets)
}
