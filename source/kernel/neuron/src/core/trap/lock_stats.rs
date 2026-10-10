// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel lock's accounting — how long a hart waited for it and how long one
//! ecall held it, per measurement window: maxima, a four-bucket histogram each, and the
//! longest hold per syscall number. Pure atomics and arithmetic, NOT target-gated, so the
//! window rule and the units run on the host; `trap::budgets` feeds it the platform's ticks
//! and judges the result.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::a_sample_counts_for_the_window_it_began_in`,
//!   `tests::the_buckets_are_microseconds_on_every_timebase`)
//! INVARIANTS: a sample belongs to the window it BEGAN in — the call that prints the
//!   bring-up lines and resets, and every hart queued behind it, began before the reset;
//!   counted into the fresh window they were its first and worst entries (22 ms of console
//!   output read as the steady state's longest hold on the board, found 2026-10-10).
//! ADR: docs/adr/0046-deterministic-parallel-compute-workpool.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Histogram buckets: <=100 µs, <=1 ms, <=10 ms, above.
pub const BUCKETS: usize = 4;
/// Syscall numbers with their own slot in the hold table; higher numbers share the last.
pub const HOLD_TABLE: usize = 80;
/// Holders named by the report, longest first.
pub const TOP_HOLDERS: usize = 3;

static WINDOW_START_TICKS: AtomicU64 = AtomicU64::new(0);
static WAIT_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static WAIT_BUCKETS: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];
static HOLD_MAX_TICKS: AtomicU64 = AtomicU64::new(0);
static HOLD_MAX_NR: AtomicU64 = AtomicU64::new(0);
static HOLD_BUCKETS: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];
static HOLD_MAX_BY_NR: [AtomicU64; HOLD_TABLE] = [const { AtomicU64::new(0) }; HOLD_TABLE];

/// The bucket of `ticks` at `ticks_per_us` (literal tick bounds were QEMU's 10 per µs — the
/// board's 24 made ">10 ms" mean ">4.2 ms").
#[inline]
fn bucket_of(ticks: u64, ticks_per_us: u64) -> usize {
    match ticks / ticks_per_us.max(1) {
        0..=100 => 0,
        101..=1_000 => 1,
        1_001..=10_000 => 2,
        _ => 3,
    }
}

#[inline]
fn in_window(t0: u64) -> bool {
    t0 >= WINDOW_START_TICKS.load(Ordering::Relaxed)
}

/// One wait for the lock that began at tick `t0` and lasted `ticks`.
#[inline]
pub fn record_wait(t0: u64, ticks: u64, ticks_per_us: u64) {
    if in_window(t0) {
        WAIT_MAX_TICKS.fetch_max(ticks, Ordering::Relaxed);
        WAIT_BUCKETS[bucket_of(ticks, ticks_per_us)].fetch_add(1, Ordering::Relaxed);
    }
}

/// One ecall `nr` that began at tick `t0` and held the lock for `ticks`.
#[inline]
pub fn record_hold(t0: u64, ticks: u64, nr: u64, ticks_per_us: u64) {
    if !in_window(t0) {
        return;
    }
    HOLD_BUCKETS[bucket_of(ticks, ticks_per_us)].fetch_add(1, Ordering::Relaxed);
    HOLD_MAX_BY_NR[(nr as usize).min(HOLD_TABLE - 1)].fetch_max(ticks, Ordering::Relaxed);
    if ticks > HOLD_MAX_TICKS.fetch_max(ticks, Ordering::Relaxed) {
        // Benign race: a concurrent larger hold may overwrite nr — the pair is diagnostic,
        // the gate asserts the maximum only.
        HOLD_MAX_NR.store(nr, Ordering::Relaxed);
    }
}

/// Begins a new window at tick `now`.
pub fn reset(now: u64) {
    WINDOW_START_TICKS.store(now, Ordering::Relaxed);
    WAIT_MAX_TICKS.store(0, Ordering::Relaxed);
    HOLD_MAX_TICKS.store(0, Ordering::Relaxed);
    HOLD_MAX_NR.store(0, Ordering::Relaxed);
    for bucket in WAIT_BUCKETS.iter().chain(&HOLD_BUCKETS) {
        bucket.store(0, Ordering::Relaxed);
    }
    for slot in &HOLD_MAX_BY_NR {
        slot.store(0, Ordering::Relaxed);
    }
}

/// The window's longest wait in ticks and the wait histogram.
pub fn waits() -> (u64, [usize; BUCKETS]) {
    (
        WAIT_MAX_TICKS.load(Ordering::Relaxed),
        core::array::from_fn(|i| WAIT_BUCKETS[i].load(Ordering::Relaxed)),
    )
}

/// The window's longest hold in ticks and the syscall number that held it.
pub fn longest_hold() -> (u64, u64) {
    (HOLD_MAX_TICKS.load(Ordering::Relaxed), HOLD_MAX_NR.load(Ordering::Relaxed))
}

/// The hold histogram and the longest holders `(nr, µs)` of the window, longest first.
pub fn holds(ticks_per_us: u64) -> ([usize; BUCKETS], [(usize, u64); TOP_HOLDERS]) {
    let buckets = core::array::from_fn(|i| HOLD_BUCKETS[i].load(Ordering::Relaxed));
    let mut top = [(0usize, 0u64); TOP_HOLDERS];
    for (nr, slot) in HOLD_MAX_BY_NR.iter().enumerate() {
        let us = slot.load(Ordering::Relaxed) / ticks_per_us.max(1);
        if let Some(at) = top.iter().position(|&(_, best)| us > best) {
            top[at..].rotate_right(1);
            top[at] = (nr, us);
        }
    }
    (buckets, top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buckets_are_microseconds_on_every_timebase() {
        // 4.3 ms: 43 000 ticks at 10 per µs, 103 200 at the board's 24.
        assert_eq!(bucket_of(43_000, 10), 2);
        assert_eq!(bucket_of(103_200, 24), 2, "above 100 000 ticks, below 10 ms");
        assert_eq!(bucket_of(240_001 + 24, 24), 3);
        assert_eq!(bucket_of(2_400, 24), 0);
        assert_eq!(bucket_of(2_424, 24), 1);
        assert_eq!(bucket_of(7, 0), 0, "an unset timebase divides by one");
    }

    // One test for the statics: they are process-wide. A tick is a microsecond here.
    #[test]
    fn a_sample_counts_for_the_window_it_began_in() {
        reset(1_000);
        // The call that reset the window, and a hart that queued behind it: both began before.
        record_hold(900, 22_000, 48, 1);
        record_wait(950, 25_000, 1);
        assert_eq!(longest_hold(), (0, 0));
        assert_eq!(waits(), (0, [0; BUCKETS]));
        assert_eq!(holds(1), ([0; BUCKETS], [(0, 0); TOP_HOLDERS]));

        record_wait(1_000, 90, 1);
        record_wait(1_200, 4_300, 1);
        record_hold(1_000, 80, 3, 1);
        record_hold(1_100, 700, 12, 1);
        record_hold(1_200, 2_500, 3, 1);
        record_hold(1_300, 11_000, 500, 1);
        assert_eq!(waits(), (4_300, [1, 0, 1, 0]));
        assert_eq!(longest_hold(), (11_000, 500));
        let (histogram, top) = holds(1);
        assert_eq!(histogram, [1, 1, 1, 1]);
        assert_eq!(top, [(HOLD_TABLE - 1, 11_000), (3, 2_500), (12, 700)]);

        reset(2_000);
        assert_eq!(holds(1), ([0; BUCKETS], [(0, 0); TOP_HOLDERS]));
        assert_eq!(waits(), (0, [0; BUCKETS]));
    }
}
