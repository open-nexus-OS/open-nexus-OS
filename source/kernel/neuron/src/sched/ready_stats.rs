// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how long a runnable task waits for a hart — the time from entering a run queue
//! (a wake, a spawn, a yield, a preemption) to being dispatched, per QoS class and per
//! measurement window: the count, the sum, the longest and a four-bucket histogram. The
//! number the scheduler's end form is judged by (TASK-0363: wake-to-run p99), measured
//! before it is built (TASK-0330 P0). Pure atomics and arithmetic, NOT target-gated, so the
//! window rule and the units run on the host; the scheduler feeds it at dispatch.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::a_wait_counts_for_its_class_and_the_window_it_began_in`)
//! INVARIANTS: a wait belongs to the window it BEGAN in (the rule of `lock_stats`); a
//!   dispatch stamped before its enqueue (two harts' clocks read apart) counts as zero.
//! ADR: docs/adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// QoS classes (the discriminants of `sched::QosClass`: idle, normal, interactive, burst).
pub const CLASSES: usize = 4;
/// Histogram buckets: <=100 µs, <=1 ms, <=10 ms, above.
pub const BUCKETS: usize = 4;

struct Class {
    count: AtomicUsize,
    sum_ticks: AtomicU64,
    max_ticks: AtomicU64,
    buckets: [AtomicUsize; BUCKETS],
}

impl Class {
    const fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            sum_ticks: AtomicU64::new(0),
            max_ticks: AtomicU64::new(0),
            buckets: [const { AtomicUsize::new(0) }; BUCKETS],
        }
    }
}

static WINDOW_START_TICKS: AtomicU64 = AtomicU64::new(0);
static CLASS: [Class; CLASSES] = [const { Class::new() }; CLASSES];

/// One class's window: dispatches, mean and longest wait in µs, the histogram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub count: usize,
    pub mean_us: u64,
    pub max_us: u64,
    pub buckets: [usize; BUCKETS],
}

/// A task of `class` that became runnable at tick `ready_at` is dispatched at tick `now`.
#[inline]
pub fn record(class: usize, ready_at: u64, now: u64, ticks_per_us: u64) {
    if ready_at < WINDOW_START_TICKS.load(Ordering::Relaxed) {
        return;
    }
    let Some(slot) = CLASS.get(class) else { return };
    let ticks = now.saturating_sub(ready_at);
    let bucket = match ticks / ticks_per_us.max(1) {
        0..=100 => 0,
        101..=1_000 => 1,
        1_001..=10_000 => 2,
        _ => 3,
    };
    slot.count.fetch_add(1, Ordering::Relaxed);
    slot.sum_ticks.fetch_add(ticks, Ordering::Relaxed);
    slot.max_ticks.fetch_max(ticks, Ordering::Relaxed);
    slot.buckets[bucket].fetch_add(1, Ordering::Relaxed);
}

/// Begins a new window at tick `now`.
pub fn reset(now: u64) {
    WINDOW_START_TICKS.store(now, Ordering::Relaxed);
    for slot in &CLASS {
        slot.count.store(0, Ordering::Relaxed);
        slot.sum_ticks.store(0, Ordering::Relaxed);
        slot.max_ticks.store(0, Ordering::Relaxed);
        for bucket in &slot.buckets {
            bucket.store(0, Ordering::Relaxed);
        }
    }
}

/// The window of `class`.
pub fn report(class: usize, ticks_per_us: u64) -> Report {
    let per_us = ticks_per_us.max(1);
    let Some(slot) = CLASS.get(class) else {
        return Report { count: 0, mean_us: 0, max_us: 0, buckets: [0; BUCKETS] };
    };
    let count = slot.count.load(Ordering::Relaxed);
    Report {
        count,
        mean_us: slot.sum_ticks.load(Ordering::Relaxed) / per_us / (count.max(1) as u64),
        max_us: slot.max_ticks.load(Ordering::Relaxed) / per_us,
        buckets: core::array::from_fn(|i| slot.buckets[i].load(Ordering::Relaxed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test for the statics: they are process-wide.
    #[test]
    fn a_wait_counts_for_its_class_and_the_window_it_began_in() {
        reset(1_000);
        record(2, 900, 5_000, 1); // runnable before the window
        assert_eq!(report(2, 1).count, 0);

        record(2, 1_000, 1_050, 1); // 50 µs
        record(2, 1_000, 3_300, 1); // 2.3 ms
        record(1, 2_000, 14_500, 1); // 12.5 ms
        record(2, 5_000, 4_990, 1); // dispatched "before" it was enqueued: zero
        record(9, 1_000, 2_000, 1); // no such class: ignored
        assert_eq!(
            report(2, 1),
            Report { count: 3, mean_us: 783, max_us: 2_300, buckets: [2, 0, 1, 0] }
        );
        assert_eq!(report(1, 1).buckets, [0, 0, 0, 1]);
        assert_eq!(report(0, 1).count, 0);
        // The board's 24 ticks per microsecond: 2 400 ticks are 100 µs, the first bucket.
        record(3, 6_000, 8_400, 24);
        assert_eq!(
            report(3, 24),
            Report { count: 1, mean_us: 100, max_us: 100, buckets: [1, 0, 0, 0] }
        );

        reset(9_000);
        assert_eq!(report(2, 1).count, 0);
    }
}
