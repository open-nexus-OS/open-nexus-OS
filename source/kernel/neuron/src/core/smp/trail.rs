// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where each hart last entered an address space and where it last stopped running a
//! task — evidence, not control. A space a hart still names is never freed
//! (`mm::hart_spaces`); the refused free (`KSELFTEST: address space free FAIL`) prints the two
//! places of the hart that held it and which came later, so the failure names the path that
//! left the hart there instead of asking for a reproduction.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: read by the reap's refusal line only (QEMU, board)
//! INVARIANTS: a hart writes only its own trail; two words per mark, no lock

use core::fmt;
use core::panic::Location;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

/// One place a hart passed, and the hart's count of marks when it did.
pub(crate) struct Mark {
    at: AtomicPtr<Location<'static>>,
    nth: AtomicU64,
}

impl Mark {
    const fn new() -> Self {
        Self { at: AtomicPtr::new(core::ptr::null_mut()), nth: AtomicU64::new(0) }
    }

    fn set(&self, at: &'static Location<'static>, nth: u64) {
        self.nth.store(nth, Ordering::Relaxed);
        self.at.store(core::ptr::from_ref(at).cast_mut(), Ordering::Release);
    }
}

impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: a non-null pointer is a `&'static Location` stored by `set`.
        match unsafe { self.at.load(Ordering::Acquire).as_ref() } {
            Some(at) => {
                let file = at.file();
                let file = file.rsplit_once("src/").map_or(file, |(_, rest)| rest);
                write!(f, "{}:{}#{}", file, at.line(), self.nth.load(Ordering::Relaxed))
            }
            None => f.write_str("never"),
        }
    }
}

/// A hart's two marks; the higher count is the later one.
pub(crate) struct Trail {
    marks: AtomicU64,
    /// Where its `satp` was last written.
    pub(crate) entered: Mark,
    /// Where it last stopped running a task.
    pub(crate) left: Mark,
}

impl Trail {
    pub(super) const fn new() -> Self {
        Self { marks: AtomicU64::new(0), entered: Mark::new(), left: Mark::new() }
    }

    fn next(&self) -> u64 {
        self.marks.fetch_add(1, Ordering::Relaxed) + 1
    }
}

impl fmt::Display for Trail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "entered={} left={}", self.entered, self.left)
    }
}

fn here() -> Option<&'static Trail> {
    super::hart_state::of(super::cpu_current_id()).map(|hart| &hart.trail)
}

/// This hart's `satp` is written at the caller.
#[track_caller]
pub(crate) fn entered_space() {
    if let Some(trail) = here() {
        trail.entered.set(Location::caller(), trail.next());
    }
}

/// This hart stops running a task at the caller.
#[track_caller]
pub(crate) fn left_task() {
    if let Some(trail) = here() {
        trail.left.set(Location::caller(), trail.next());
    }
}

/// Hart `cpu`'s trail.
pub(crate) fn of(cpu: usize) -> Option<&'static Trail> {
    super::hart_state::at(cpu).map(|hart| &hart.trail)
}
