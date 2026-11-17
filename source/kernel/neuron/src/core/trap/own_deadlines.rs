// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: each hart's own deadlines (TASK-0363 P8): the earliest deadline of a task that
//! blocked on the hart, and of a timer cap armed on it. The hart that blocked a task (or armed a
//! cap) armed its timer for it (`arm_wakeup`, ADR-0052), so that hart wakes for it, and only that
//! hart: a sleeping hart arms its own floor (`deadline_rule::sleep_arm`), a hart that delivered
//! re-arms to its own floor — no hart arms another's deadline as a backstop (two harts woke for
//! one deadline, the second for nothing: board, up to 29 a hart in 10 s).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the arm decision in `deadline_rule` (host); `SELFTEST: wait deadline ok`,
//!   `KINIT: idle timer wakes` (QEMU, board)
//! INVARIANTS: a floor is a lower bound of the hart's pending deadlines — lowered under the
//!   kernel lock by the block or the arm, set exact by the walk under it. A walk computes into
//!   `scratch` and publishes after, so an unlocked reader never sees a floor above a pending
//!   deadline. A deadline belongs to the hart it was armed on, wherever its task runs later.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::hart_table::{HartTable, SizeError};

/// One hart's floor of one kind.
pub struct Floor {
    published: AtomicU64,
    scratch: AtomicU64,
}

impl Floor {
    const fn new() -> Self {
        Self { published: AtomicU64::new(u64::MAX), scratch: AtomicU64::new(u64::MAX) }
    }
}

/// What a deadline is of.
#[derive(Clone, Copy)]
pub enum Kind {
    /// A task blocked with a deadline (receive, send, waitset, fence).
    Blocked,
    /// A timer cap.
    Cap,
}

static BLOCKED: HartTable<Floor> = HartTable::new(Floor::new());
static CAPS: HartTable<Floor> = HartTable::new(Floor::new());

/// Sizes the floors for a machine of `harts` CPUs (`smp::size_for_machine`).
pub fn size(harts: usize) -> Result<(), SizeError> {
    BLOCKED.size(harts, Floor::new)?;
    CAPS.size(harts, Floor::new)
}

fn table(kind: Kind) -> &'static HartTable<Floor> {
    match kind {
        Kind::Blocked => &BLOCKED,
        Kind::Cap => &CAPS,
    }
}

fn slot(kind: Kind, cpu: usize) -> &'static Floor {
    let t = table(kind);
    t.get(cpu).unwrap_or(t.boot())
}

/// A deadline `ns` was armed on `cpu` (0: none). Under the kernel lock.
pub fn lower(kind: Kind, cpu: usize, ns: u64) {
    if ns != 0 {
        slot(kind, cpu).published.fetch_min(ns, Ordering::Relaxed);
    }
}

/// A walk over every pending deadline of `kind` starts: [`note`] each, then [`publish`]. Under
/// the kernel lock, which also orders it against [`lower`].
pub fn begin(kind: Kind) {
    for f in table(kind).iter() {
        f.scratch.store(u64::MAX, Ordering::Relaxed);
    }
}

/// The walk found `ns` pending on `cpu`.
pub fn note(kind: Kind, cpu: usize, ns: u64) {
    slot(kind, cpu).scratch.fetch_min(ns, Ordering::Relaxed);
}

/// The walk is complete: its exact floors become the published ones.
pub fn publish(kind: Kind) {
    for f in table(kind).iter() {
        f.published.store(f.scratch.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

/// `cpu`'s own floor: the earliest of its blocked tasks' and its timer caps' deadlines
/// (`u64::MAX`: none). Read without the kernel lock.
pub fn floor(cpu: usize) -> u64 {
    let blocked = slot(Kind::Blocked, cpu).published.load(Ordering::Relaxed);
    blocked.min(slot(Kind::Cap, cpu).published.load(Ordering::Relaxed))
}
