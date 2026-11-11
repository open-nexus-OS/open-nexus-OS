// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how long a runnable task waits for a hart — the time from entering a run queue
//! (a wake, a spawn, a yield, a preemption) to being dispatched, per QoS class and per
//! measurement window: the count, the sum, the longest and a four-bucket histogram. The
//! number the scheduler's end form is judged by (TASK-0363: wake-to-run p99), measured
//! before it is built (TASK-0330 P0). Pure atomics and arithmetic, NOT target-gated, so the
//! window rule and the units run on the host; the scheduler feeds it at dispatch.
//!
//! A wait above one millisecond is attributed to what the task's hart ran when the task
//! became runnable (TASK-0363 P0): no task (the hart slept or was between tasks — the wake
//! did not reach it in time), a task of a lower class (a preemption that did not happen), or
//! a peer of its class or higher (the hart was rightly busy: placement, not preemption).
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::a_wait_counts_for_its_class_and_the_window_it_began_in`,
//!   `tests::a_task_waits_behind_what_its_hart_ran`)
//! INVARIANTS: a wait belongs to the window it BEGAN in (the rule of `lock_stats`); a
//!   dispatch stamped before its enqueue (two harts' clocks read apart) counts as zero; the
//!   attributed waits of a class sum to its two upper buckets.
//!
//! ADR: docs/adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// QoS classes (the discriminants of `sched::QosClass`: idle, normal, interactive, burst).
pub const CLASSES: usize = 4;
/// Histogram buckets: <=100 µs, <=1 ms, <=10 ms, above.
pub const BUCKETS: usize = 4;
/// What a late task waited behind (the variants of [`Behind`]).
pub const CAUSES: usize = 3;
/// A wait above this is late: it is attributed (the two upper buckets).
const LATE_US: u64 = 1_000;

/// What the hart a task was queued on ran when the task became runnable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Behind {
    /// No task: the hart slept or was between two tasks.
    Idle = 0,
    /// A task of a lower class — the newcomer should have preempted it.
    Lower = 1,
    /// A task of its own class or a higher one.
    Peer = 2,
}

/// What a task of `class` waits behind on a hart whose current task has `current`'s class.
pub fn behind(current: Option<usize>, class: usize) -> Behind {
    match current {
        None => Behind::Idle,
        Some(running) if running < class => Behind::Lower,
        Some(_) => Behind::Peer,
    }
}

struct Class {
    count: AtomicUsize,
    sum_ticks: AtomicU64,
    max_ticks: AtomicU64,
    buckets: [AtomicUsize; BUCKETS],
    late: [AtomicUsize; CAUSES],
}

impl Class {
    const fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            sum_ticks: AtomicU64::new(0),
            max_ticks: AtomicU64::new(0),
            buckets: [const { AtomicUsize::new(0) }; BUCKETS],
            late: [const { AtomicUsize::new(0) }; CAUSES],
        }
    }
}

static WINDOW_START_TICKS: AtomicU64 = AtomicU64::new(0);
static CLASS: [Class; CLASSES] = [const { Class::new() }; CLASSES];

/// One class's window: dispatches, mean and longest wait in µs, the histogram, and the late
/// waits by what they waited behind (indexed by [`Behind`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub count: usize,
    pub mean_us: u64,
    pub max_us: u64,
    pub buckets: [usize; BUCKETS],
    pub late: [usize; CAUSES],
}

/// A task of `class` that became runnable at tick `ready_at` — its hart running `waited`
/// then — is dispatched at tick `now`.
#[inline]
pub fn record(class: usize, ready_at: u64, now: u64, ticks_per_us: u64, waited: Behind) {
    if ready_at < WINDOW_START_TICKS.load(Ordering::Relaxed) {
        return;
    }
    let Some(slot) = CLASS.get(class) else { return };
    let ticks = now.saturating_sub(ready_at);
    let us = ticks / ticks_per_us.max(1);
    let bucket = match us {
        0..=100 => 0,
        101..=LATE_US => 1,
        1_001..=10_000 => 2,
        _ => 3,
    };
    slot.count.fetch_add(1, Ordering::Relaxed);
    slot.sum_ticks.fetch_add(ticks, Ordering::Relaxed);
    slot.max_ticks.fetch_max(ticks, Ordering::Relaxed);
    slot.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    if us > LATE_US {
        slot.late[waited as usize].fetch_add(1, Ordering::Relaxed);
    }
}

/// Begins a new window at tick `now`.
pub fn reset(now: u64) {
    WINDOW_START_TICKS.store(now, Ordering::Relaxed);
    for slot in &CLASS {
        slot.count.store(0, Ordering::Relaxed);
        slot.sum_ticks.store(0, Ordering::Relaxed);
        slot.max_ticks.store(0, Ordering::Relaxed);
        for counter in slot.buckets.iter().chain(&slot.late) {
            counter.store(0, Ordering::Relaxed);
        }
    }
}

/// The window of `class`.
pub fn report(class: usize, ticks_per_us: u64) -> Report {
    let per_us = ticks_per_us.max(1);
    let Some(slot) = CLASS.get(class) else {
        return Report {
            count: 0,
            mean_us: 0,
            max_us: 0,
            buckets: [0; BUCKETS],
            late: [0; CAUSES],
        };
    };
    let count = slot.count.load(Ordering::Relaxed);
    Report {
        count,
        mean_us: slot.sum_ticks.load(Ordering::Relaxed) / per_us / (count.max(1) as u64),
        max_us: slot.max_ticks.load(Ordering::Relaxed) / per_us,
        buckets: core::array::from_fn(|i| slot.buckets[i].load(Ordering::Relaxed)),
        late: core::array::from_fn(|i| slot.late[i].load(Ordering::Relaxed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test for the statics: they are process-wide.
    #[test]
    fn a_wait_counts_for_its_class_and_the_window_it_began_in() {
        reset(1_000);
        record(2, 900, 5_000, 1, Behind::Idle); // runnable before the window
        assert_eq!(report(2, 1).count, 0);

        record(2, 1_000, 1_050, 1, Behind::Lower); // 50 µs: in time, not attributed
        record(2, 1_000, 3_300, 1, Behind::Lower); // 2.3 ms
        record(1, 2_000, 14_500, 1, Behind::Peer); // 12.5 ms
        record(2, 5_000, 4_990, 1, Behind::Idle); // dispatched "before" it was enqueued: zero
        record(9, 1_000, 2_000, 1, Behind::Idle); // no such class: ignored
        assert_eq!(
            report(2, 1),
            Report {
                count: 3,
                mean_us: 783,
                max_us: 2_300,
                buckets: [2, 0, 1, 0],
                late: [0, 1, 0]
            }
        );
        assert_eq!(report(1, 1).buckets, [0, 0, 0, 1]);
        assert_eq!(report(1, 1).late, [0, 0, 1]);
        assert_eq!(report(0, 1).count, 0);
        // The board's 24 ticks per microsecond: 2 400 ticks are 100 µs, the first bucket.
        record(3, 6_000, 8_400, 24, Behind::Peer);
        assert_eq!(
            report(3, 24),
            Report { count: 1, mean_us: 100, max_us: 100, buckets: [1, 0, 0, 0], late: [0; 3] }
        );
        // Exactly one millisecond is in time; one tick more is late. The late waits of a
        // class sum to its two upper buckets.
        record(3, 6_000, 6_000 + 24_000, 24, Behind::Idle);
        record(3, 6_000, 6_000 + 24_024, 24, Behind::Idle);
        record(3, 6_000, 6_000 + 480_000, 24, Behind::Peer);
        let burst = report(3, 24);
        assert_eq!(burst.late, [1, 0, 1]);
        assert_eq!(burst.late.iter().sum::<usize>(), burst.buckets[2] + burst.buckets[3]);

        reset(9_000);
        assert_eq!(report(2, 1).count, 0);
        assert_eq!(report(3, 1).late, [0; 3]);
    }

    #[test]
    fn a_task_waits_behind_what_its_hart_ran() {
        assert_eq!(behind(None, 2), Behind::Idle);
        assert_eq!(behind(Some(1), 2), Behind::Lower);
        assert_eq!(behind(Some(0), 1), Behind::Lower);
        assert_eq!(behind(Some(2), 2), Behind::Peer);
        assert_eq!(behind(Some(3), 2), Behind::Peer);
        // The lowest class has nothing below it.
        assert_eq!(behind(Some(0), 0), Behind::Peer);
    }
}
