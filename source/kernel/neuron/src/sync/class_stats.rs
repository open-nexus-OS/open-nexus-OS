// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what each lock class costs (TASK-0330 P2, RFC-0102 "Budgets") — per class the
//! takes, the takes that had to wait, the longest wait and the longest hold, in timer ticks.
//! Fed by the ranked lock on every take and release; read per window by the baseline lines and
//! at the boot-end fence by the budget gate. Pure atomics, so it runs on the host.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::a_window_keeps_the_longest_and_starts_again`)
//! INVARIANTS: a window's read takes the counts with it; no lock, no allocation.

use core::sync::atomic::{AtomicU64, Ordering};

use super::lock_rank::Class;

struct Cell {
    takes: AtomicU64,
    waited: AtomicU64,
    max_wait: AtomicU64,
    max_hold: AtomicU64,
}

impl Cell {
    const NEW: Cell = Cell {
        takes: AtomicU64::new(0),
        waited: AtomicU64::new(0),
        max_wait: AtomicU64::new(0),
        max_hold: AtomicU64::new(0),
    };
}

static CELLS: [Cell; Class::COUNT] = [Cell::NEW; Class::COUNT];

/// One class's numbers over a window, in timer ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Window {
    /// How often a lock of the class was taken.
    pub takes: u64,
    /// How many of those takes found the lock held.
    pub waited: u64,
    /// The longest wait for a lock of the class.
    pub max_wait: u64,
    /// The longest hold of a lock of the class.
    pub max_hold: u64,
}

/// A lock of `class` was taken; `waited`: the ticks it was waited for, if it was held.
#[inline]
pub fn took(class: Class, waited: Option<u64>) {
    let cell = &CELLS[class.index()];
    cell.takes.fetch_add(1, Ordering::Relaxed);
    if let Some(ticks) = waited {
        cell.waited.fetch_add(1, Ordering::Relaxed);
        cell.max_wait.fetch_max(ticks, Ordering::Relaxed);
    }
}

/// A lock of `class` was released after `held` ticks.
#[inline]
pub fn released(class: Class, held: u64) {
    let cell = &CELLS[class.index()];
    if held > cell.max_hold.load(Ordering::Relaxed) {
        cell.max_hold.fetch_max(held, Ordering::Relaxed);
    }
}

/// Reads `class`'s window and starts the next.
pub fn take_window(class: Class) -> Window {
    let cell = &CELLS[class.index()];
    Window {
        takes: cell.takes.swap(0, Ordering::Relaxed),
        waited: cell.waited.swap(0, Ordering::Relaxed),
        max_wait: cell.max_wait.swap(0, Ordering::Relaxed),
        max_hold: cell.max_hold.swap(0, Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_keeps_the_longest_and_starts_again() {
        // The registry class: no host test takes a registry lock beside this one.
        let class = Class::Registry;
        let _ = take_window(class);
        took(class, None);
        took(class, Some(40));
        took(class, Some(15));
        released(class, 7);
        released(class, 90);
        released(class, 30);
        assert_eq!(take_window(class), Window { takes: 3, waited: 2, max_wait: 40, max_hold: 90 });
        assert_eq!(take_window(class), Window::default());
    }
}
