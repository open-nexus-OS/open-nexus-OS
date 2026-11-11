// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: who yields — the `yield` syscalls of the measurement window per task, so a loop
//! that polls by yielding is named instead of showing up as the scheduler's share of the
//! kernel lock (TASK-0363 P0: 70 000 yields a second on the board, 149 000 on one emulated
//! hart, their task unknown). Pure atomics, NOT target-gated; the yield syscall feeds it, the
//! boot-end fence prints the three largest (`KINIT: yield top`).
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::the_three_tasks_that_yield_most_are_named`)
//! INVARIANTS: a task beyond the table shares its last entry — counted, never dropped.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Tasks counted apart; a pid at or above this shares the last entry.
pub const TASKS: usize = 256;

static YIELDS: [AtomicU32; TASKS] = [const { AtomicU32::new(0) }; TASKS];
/// The service id the kernel holds for the task that yielded last from each entry.
static SERVICE: [AtomicU64; TASKS] = [const { AtomicU64::new(0) }; TASKS];

/// One of the largest yielders: the task (its table entry), its service id, its yields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Yielder {
    pub pid: u32,
    pub service_id: u64,
    pub yields: u32,
}

/// Task `pid` of service `service_id` yielded.
#[inline]
pub fn record(pid: u32, service_id: u64) {
    let entry = (pid as usize).min(TASKS - 1);
    YIELDS[entry].fetch_add(1, Ordering::Relaxed);
    SERVICE[entry].store(service_id, Ordering::Relaxed);
}

/// Begins a new window.
pub fn reset() {
    for counter in &YIELDS {
        counter.store(0, Ordering::Relaxed);
    }
}

/// The three tasks with the most yields in the window, largest first, and the window's total.
pub fn top() -> ([Yielder; 3], u64) {
    let mut top = [Yielder::default(); 3];
    let mut total = 0u64;
    for (entry, counter) in YIELDS.iter().enumerate() {
        let yields = counter.load(Ordering::Relaxed);
        total += u64::from(yields);
        let mut candidate = Yielder {
            pid: entry as u32,
            service_id: SERVICE[entry].load(Ordering::Relaxed),
            yields,
        };
        for slot in top.iter_mut() {
            if candidate.yields > slot.yields {
                core::mem::swap(slot, &mut candidate);
            }
        }
    }
    (top, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test for the statics: they are process-wide.
    #[test]
    fn the_three_tasks_that_yield_most_are_named() {
        reset();
        assert_eq!(top(), ([Yielder::default(); 3], 0));
        for _ in 0..5 {
            record(7, 0x77);
        }
        for _ in 0..9 {
            record(40, 0x40);
        }
        record(3, 0x3);
        record(3, 0x3);
        record(12, 0x12);
        // Two tasks beyond the table share its last entry.
        record(TASKS as u32, 0xaa);
        record(5_000, 0xbb);
        record(5_000, 0xbb);
        let (top3, total) = top();
        assert_eq!(total, 20);
        assert_eq!(
            top3,
            [
                Yielder { pid: 40, service_id: 0x40, yields: 9 },
                Yielder { pid: 7, service_id: 0x77, yields: 5 },
                Yielder { pid: TASKS as u32 - 1, service_id: 0xbb, yields: 3 },
            ]
        );
        reset();
        assert_eq!(top().1, 0);
    }
}
