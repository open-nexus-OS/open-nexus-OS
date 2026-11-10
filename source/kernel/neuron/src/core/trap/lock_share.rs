// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: who holds the kernel lock, in sum — the time between taking and releasing it,
//! per holder (a syscall number, or a trap kind that is no syscall) and per measurement
//! window. `lock_stats` names the LONGEST holds (the budget); this names where the lock's
//! time goes, which is what the order of its decomposition follows (TASK-0330 P0). Pure
//! atomics and arithmetic, NOT target-gated, so the window rule and the grouping run on the
//! host; `trap::runtime::KernelGuard` feeds it at release.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::a_hold_counts_for_its_holder_and_the_window_it_began_in`,
//!   `tests::every_syscall_number_has_one_group`)
//! INVARIANTS: a hold belongs to the window it BEGAN in (the rule of `lock_stats`); a holder
//!   outside the table counts as `OTHER`, never dropped.
//! ADR: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Syscall numbers with a slot of their own (the width of `lock_stats::HOLD_TABLE`).
pub const SYSCALLS: usize = 80;
/// The timer interrupt's delivery and preemption.
pub const TIMER: usize = SYSCALLS;
/// A resched IPI's preemption.
pub const IPI: usize = SYSCALLS + 1;
/// An external interrupt's delivery to its endpoint.
pub const EXTERNAL: usize = SYSCALLS + 2;
/// A page fault (commit on touch, swap-in, the fault's verdict).
pub const FAULT: usize = SYSCALLS + 3;
/// A hart's scheduling loop picking its next task.
pub const IDLE: usize = SYSCALLS + 4;
/// Every other taker (a phased syscall's re-take, a pending order, a cache right).
pub const OTHER: usize = SYSCALLS + 5;
const HOLDERS: usize = SYSCALLS + 6;

/// The subsystems the lock's time is split by — the candidates of its decomposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Yield, wait, QoS and placement, timers: the run queues and the task states.
    Sched = 0,
    /// Send, receive, call, endpoints, waitsets, fences, interrupt binding.
    Ipc = 1,
    /// Memory objects, mappings, address spaces, swap.
    Mm = 2,
    /// Spawn, exec, exit, the process orders.
    Task = 3,
    /// Capability transfer, clone, close, query.
    Caps = 4,
    /// The console and what no group above names.
    Misc = 5,
    Timer = 6,
    Ipi = 7,
    External = 8,
    Fault = 9,
    Idle = 10,
}

/// Groups in the report's order.
pub const GROUPS: usize = 11;

/// The group of holder `id` (a syscall number below [`SYSCALLS`], else a trap kind).
pub const fn group_of(id: usize) -> Group {
    match id {
        0 | 12 | 15 | 25 | 32..=35 | 48 | 52 => Group::Sched,
        2 | 3 | 14 | 18 | 19 | 21..=23 | 26 | 36..=43 | 58 | 59 | 62 | 67 => Group::Ipc,
        5 | 6 | 46 | 47 | 49 | 51 | 53..=55 | 60 | 61 | 64 | 68..=74 => Group::Mm,
        7 | 11 | 13 | 17 | 29 | 63 | 65 | 66 => Group::Task,
        8 | 20 | 24 | 28 | 30 | 31 => Group::Caps,
        TIMER => Group::Timer,
        IPI => Group::Ipi,
        EXTERNAL => Group::External,
        FAULT => Group::Fault,
        IDLE => Group::Idle,
        _ => Group::Misc,
    }
}

static WINDOW_START_TICKS: AtomicU64 = AtomicU64::new(0);
static SUM_TICKS: [AtomicU64; HOLDERS] = [const { AtomicU64::new(0) }; HOLDERS];
static COUNT: [AtomicUsize; HOLDERS] = [const { AtomicUsize::new(0) }; HOLDERS];

/// One hold of `holder` that began at tick `t0` and lasted `ticks`.
#[inline]
pub fn record(holder: usize, t0: u64, ticks: u64) {
    if t0 < WINDOW_START_TICKS.load(Ordering::Relaxed) {
        return;
    }
    let slot = if holder < HOLDERS { holder } else { OTHER };
    SUM_TICKS[slot].fetch_add(ticks, Ordering::Relaxed);
    COUNT[slot].fetch_add(1, Ordering::Relaxed);
}

/// Begins a new window at tick `now`.
pub fn reset(now: u64) {
    WINDOW_START_TICKS.store(now, Ordering::Relaxed);
    for (sum, count) in SUM_TICKS.iter().zip(&COUNT) {
        sum.store(0, Ordering::Relaxed);
        count.store(0, Ordering::Relaxed);
    }
}

/// The window's start tick (the report divides the held time by the window's length).
pub fn window_start() -> u64 {
    WINDOW_START_TICKS.load(Ordering::Relaxed)
}

/// Microseconds the lock was held per group, and the takes per group.
pub fn by_group(ticks_per_us: u64) -> ([u64; GROUPS], [usize; GROUPS]) {
    let mut us = [0u64; GROUPS];
    let mut takes = [0usize; GROUPS];
    for id in 0..HOLDERS {
        let group = group_of(id) as usize;
        us[group] += SUM_TICKS[id].load(Ordering::Relaxed) / ticks_per_us.max(1);
        takes[group] += COUNT[id].load(Ordering::Relaxed);
    }
    (us, takes)
}

/// The three holders with the largest sum `(id, µs, takes)`, largest first.
pub fn top(ticks_per_us: u64) -> [(usize, u64, usize); 3] {
    let mut top = [(0usize, 0u64, 0usize); 3];
    for id in 0..HOLDERS {
        let us = SUM_TICKS[id].load(Ordering::Relaxed) / ticks_per_us.max(1);
        if let Some(at) = top.iter().position(|&(_, best, _)| us > best) {
            top[at..].rotate_right(1);
            top[at] = (id, us, COUNT[id].load(Ordering::Relaxed));
        }
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_syscall_number_has_one_group() {
        // The numbers of `syscall/mod.rs` the split is read by, one per group.
        assert_eq!(group_of(0), Group::Sched, "yield");
        assert_eq!(group_of(58), Group::Ipc, "call");
        assert_eq!(group_of(40), Group::Ipc, "waitset wait");
        assert_eq!(group_of(53), Group::Mm, "vm_map");
        assert_eq!(group_of(63), Group::Task, "exec");
        assert_eq!(group_of(24), Group::Caps, "cap clone");
        assert_eq!(group_of(44), Group::Misc, "console write");
        assert_eq!(group_of(FAULT), Group::Fault);
        assert_eq!(group_of(IDLE), Group::Idle);
        assert_eq!(group_of(OTHER), Group::Misc);
        assert_eq!(group_of(usize::MAX), Group::Misc);
    }

    // One test for the statics: they are process-wide. A tick is a microsecond here.
    #[test]
    fn a_hold_counts_for_its_holder_and_the_window_it_began_in() {
        reset(1_000);
        record(58, 900, 5_000); // began before the window
        assert_eq!(by_group(1), ([0; GROUPS], [0; GROUPS]));

        record(58, 1_000, 40);
        record(59, 1_100, 60);
        record(0, 1_200, 10);
        record(FAULT, 1_300, 700);
        record(9_999, 1_400, 3); // outside the table: counted as OTHER, never dropped
        let (us, takes) = by_group(1);
        assert_eq!(us[Group::Ipc as usize], 100);
        assert_eq!(takes[Group::Ipc as usize], 2);
        assert_eq!(us[Group::Sched as usize], 10);
        assert_eq!(us[Group::Fault as usize], 700);
        assert_eq!((us[Group::Misc as usize], takes[Group::Misc as usize]), (3, 1));
        assert_eq!(top(1), [(FAULT, 700, 1), (59, 60, 1), (58, 40, 1)]);
        // 24 ticks per microsecond (the board): sums are divided, takes are not.
        assert_eq!(by_group(24).0[Group::Fault as usize], 29);

        reset(2_000);
        assert_eq!(by_group(1), ([0; GROUPS], [0; GROUPS]));
        assert_eq!(window_start(), 2_000);
    }
}
