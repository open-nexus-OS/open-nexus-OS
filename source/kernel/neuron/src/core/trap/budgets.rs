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
    SWEEP_MAX_TICKS.store(0, Ordering::Relaxed);
    SWEEP_MAX_TASKS.store(0, Ordering::Relaxed);
    SWEEP_CALLS.store(0, Ordering::Relaxed);
    SWEEP_TICKS_TOTAL.store(0, Ordering::Relaxed);
    SWEEP_SKIPPED.store(0, Ordering::Relaxed);
    // NEXT_DEADLINE_NS is LIVE state, not accounting — never reset it here.
}

/// Earliest pending block deadline across all tasks, or `u64::MAX` when none
/// is armed — the O(1) gate in front of the O(tasks) deadline sweep.
///
/// The sweep used to run on EVERY scheduling transition: 242k–389k calls per
/// 4-hart interactive boot at ~6–7 µs each (measured 2026-07-25 with
/// `record_sweep`), i.e. **~1.7–2.3 s of BKL held per boot** scanning for
/// deadlines that had overwhelmingly not expired, plus 6–13 ms outliers that
/// were the worst BKL holds in the run.
///
/// SAFETY OF THE SHORTCUT: skipping the scan while `now < min` can never miss
/// an expiry, because (a) every task that blocks WITH a deadline lowers this
/// bound via [`note_block_deadline`] before it can expire, and (b) the scan
/// itself recomputes the exact minimum of the deadlines still pending, so the
/// bound is never stale-high. A bound that is too LOW only costs one
/// unnecessary scan; a bound that is too high would lose a wakeup, which the
/// two rules above prevent.
pub static NEXT_DEADLINE_NS: AtomicU64 = AtomicU64::new(u64::MAX);

/// Lowers the sweep gate when a task blocks with a deadline.
#[inline]
pub fn note_block_deadline(deadline_ns: u64) {
    if deadline_ns != 0 {
        NEXT_DEADLINE_NS.fetch_min(deadline_ns, Ordering::Relaxed);
    }
}

/// True if the deadline sweep can be skipped entirely at `now_ns`.
#[inline]
pub fn sweep_can_skip(now_ns: u64) -> bool {
    now_ns < NEXT_DEADLINE_NS.load(Ordering::Relaxed)
}

/// Publishes the exact minimum the sweep just observed (`u64::MAX` = none).
#[inline]
pub fn set_next_deadline(min_ns: u64) {
    NEXT_DEADLINE_NS.store(min_ns, Ordering::Relaxed);
}

/// Sweeps actually performed vs skipped by the gate (proof the gate works).
pub static SWEEP_SKIPPED: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn record_sweep_skipped() {
    SWEEP_SKIPPED.fetch_add(1, Ordering::Relaxed);
}

/// Deadline-sweep attribution (`syscall::api::wake_expired_blocked`): the one
/// body shared by every >10 ms BKL holder observed so far (`yield`,
/// `ipc_recv_v1/v2`, `waitset_wait`). Diagnostic only — it answers "is the
/// hold the O(tasks) sweep, or something else in the syscall?" before anyone
/// optimises the wrong half.
pub static SWEEP_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
pub static SWEEP_MAX_TASKS: AtomicUsize = AtomicUsize::new(0);
pub static SWEEP_CALLS: AtomicUsize = AtomicUsize::new(0);
pub static SWEEP_TICKS_TOTAL: AtomicU64 = AtomicU64::new(0);
/// Second-order attribution for the sweep's worst call: how many expiries it
/// actually processed, and how many of its ticks went into the DEREGISTER +
/// WAKE half (each wake can IPI another hart) rather than the O(tasks) scan.
///
/// This exists because the first-order numbers are contradictory on their own:
/// `mean=101us` over 4527 calls with `max=17341us` at `tasks=54` is 171x the
/// mean for the same task count, so the scan length cannot be the explanation.
/// Only splitting scan from wake tells you which half to fix.
pub static SWEEP_MAX_WAKES: AtomicUsize = AtomicUsize::new(0);
pub static SWEEP_MAX_WAKE_TICKS: AtomicU64 = AtomicU64::new(0);
/// Cross-core wake IPI cost (`smp::request_resched` -> `sbi::send_ipi`), which
/// today runs INSIDE the held BKL from `Tasks::wake`. Split out because the
/// two candidates for an expensive wake — `Scheduler::purge` (a scan of 4 CPUs
/// x 4 short queues over an 8-byte element) and a firmware-mediated IPI — have
/// opposite fixes, and only the second justifies restructuring the wake path.
pub static WAKE_IPI_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
pub static WAKE_IPI_TICKS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static WAKE_IPI_COUNT: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn record_wake_ipi(ticks: u64) {
    WAKE_IPI_MAX_TICKS.fetch_max(ticks, Ordering::Relaxed);
    WAKE_IPI_TICKS_TOTAL.fetch_add(ticks, Ordering::Relaxed);
    WAKE_IPI_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// `(max_us, mean_us, count)` for the cross-core wake IPI.
pub fn wake_ipi_report() -> (u64, u64, usize) {
    let n = WAKE_IPI_COUNT.load(Ordering::Relaxed);
    let total = WAKE_IPI_TICKS_TOTAL.load(Ordering::Relaxed);
    (
        WAKE_IPI_MAX_TICKS.load(Ordering::Relaxed) / TICKS_PER_US,
        if n == 0 { 0 } else { total / (n as u64) / TICKS_PER_US },
        n,
    )
}

#[inline]
pub fn record_sweep(ticks: u64, tasks: usize, wakes: usize, wake_ticks: u64) {
    let prev = SWEEP_MAX_TICKS.fetch_max(ticks, Ordering::Relaxed);
    if ticks > prev {
        SWEEP_MAX_TASKS.store(tasks, Ordering::Relaxed);
        SWEEP_MAX_WAKES.store(wakes, Ordering::Relaxed);
        SWEEP_MAX_WAKE_TICKS.store(wake_ticks, Ordering::Relaxed);
    }
    SWEEP_CALLS.fetch_add(1, Ordering::Relaxed);
    SWEEP_TICKS_TOTAL.fetch_add(ticks, Ordering::Relaxed);
}

/// `(max_us, tasks_at_max, calls, mean_us, skipped, wakes_at_max,
/// wake_us_at_max)` for the sweep since the last [`reset`]. `skipped` is the
/// O(1)-gate win; the last two split the worst call into its wake half and
/// (by subtraction) its scan half.
pub fn sweep_report() -> (u64, usize, usize, u64, usize, usize, u64) {
    let calls = SWEEP_CALLS.load(Ordering::Relaxed);
    let total = SWEEP_TICKS_TOTAL.load(Ordering::Relaxed);
    let mean_us = if calls == 0 { 0 } else { total / (calls as u64) / TICKS_PER_US };
    (
        SWEEP_MAX_TICKS.load(Ordering::Relaxed) / TICKS_PER_US,
        SWEEP_MAX_TASKS.load(Ordering::Relaxed),
        calls,
        mean_us,
        SWEEP_SKIPPED.load(Ordering::Relaxed),
        SWEEP_MAX_WAKES.load(Ordering::Relaxed),
        SWEEP_MAX_WAKE_TICKS.load(Ordering::Relaxed) / TICKS_PER_US,
    )
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
