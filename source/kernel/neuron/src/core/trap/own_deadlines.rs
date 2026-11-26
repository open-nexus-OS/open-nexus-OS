// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the deadlines a hart wakes for, as lower bounds read without a lock. Each hart's
//! own (TASK-0363 P8): the earliest deadline of a task that blocked on the hart, and of a timer
//! cap armed on it. The hart that blocked a task (or armed a cap) armed its timer for it
//! (`arm_wakeup`, ADR-0052), so that hart wakes for it, and only that hart: a sleeping hart arms
//! its own floor (`deadline_rule::sleep_arm`), a hart that delivered re-arms to its own floor —
//! no hart arms another's deadline as a backstop (two harts woke for one deadline, the second
//! for nothing: board, up to 29 a hart in 10 s). And the gate of all blocked tasks: the O(1)
//! question in front of the walk over them (`syscall::api::expire_deadlines`), which ran on
//! every scheduling transition before it (2026-07-25: 242k-389k walks a boot, 1.7-2.3 s under
//! the kernel lock for deadlines that had not passed).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the bound's protocol in `sync::noted_bound` and the arm decision in
//!   `deadline_rule` (host); `SELFTEST: wait deadline ok`, `KINIT: idle timer wakes` (QEMU, board)
//! INVARIANTS: every bound here is a `noted_bound::Bound` — lowered by the block, inside the
//!   hold of the thread lock that makes the wait visible to the walk (`task::wake::
//!   prepare_block`, under no kernel lock since TASK-0330 P2), or by the arm; set exact by the
//!   walk, of which one runs at a time (the kernel lock's). A bound too low costs a look, one
//!   too high would lose an expiry: a read beside a walk waits for it, so a reader holds no lock
//!   the walk takes and never reads from inside the walk. A deadline belongs to
//!   the hart it was armed on, wherever its task runs later.

use crate::sync::hart_table::{HartTable, SizeError};
use crate::sync::noted_bound::{Bound, Walks};

/// What a deadline is of.
#[derive(Clone, Copy)]
pub enum Kind {
    /// A task blocked with a deadline (receive, send, waitset, fence).
    Blocked,
    /// A timer cap.
    Cap,
}

static BLOCKED: HartTable<Bound> = HartTable::new(Bound::new());
static CAPS: HartTable<Bound> = HartTable::new(Bound::new());
/// The earliest deadline of any blocked task: the walk's gate.
static ALL_BLOCKED: Bound = Bound::new();
static BLOCKED_WALKS: Walks = Walks::new();
static CAP_WALKS: Walks = Walks::new();

/// Sizes the floors for a machine of `harts` CPUs (`smp::size_for_machine`).
pub fn size(harts: usize) -> Result<(), SizeError> {
    BLOCKED.size(harts, Bound::new)?;
    CAPS.size(harts, Bound::new)
}

fn table(kind: Kind) -> &'static HartTable<Bound> {
    match kind {
        Kind::Blocked => &BLOCKED,
        Kind::Cap => &CAPS,
    }
}

fn walks(kind: Kind) -> &'static Walks {
    match kind {
        Kind::Blocked => &BLOCKED_WALKS,
        Kind::Cap => &CAP_WALKS,
    }
}

/// The bound of all harts, for the kind that has a walk of its own behind a gate.
fn all(kind: Kind) -> Option<&'static Bound> {
    matches!(kind, Kind::Blocked).then_some(&ALL_BLOCKED)
}

fn slot(kind: Kind, cpu: usize) -> &'static Bound {
    let t = table(kind);
    t.get(cpu).unwrap_or(t.boot())
}

/// A deadline `ns` was armed on `cpu` (0: none) — in the hold the module's invariant names.
pub fn lower(kind: Kind, cpu: usize, ns: u64) {
    if ns != 0 {
        slot(kind, cpu).lower(ns);
        if let Some(all) = all(kind) {
            all.lower(ns);
        }
    }
}

/// A walk over every pending deadline of `kind` starts: [`note`] each, then [`publish`]. One
/// walk of a kind at a time.
pub fn begin(kind: Kind) {
    walks(kind).begin();
    for f in table(kind).iter().chain(all(kind)) {
        f.clear();
    }
}

/// The walk found `ns` pending on `cpu`.
pub fn note(kind: Kind, cpu: usize, ns: u64) {
    slot(kind, cpu).note(ns);
    if let Some(all) = all(kind) {
        all.note(ns);
    }
}

/// The walk is complete: its exact bounds, and what waiters noted since it began, are the
/// bounds.
pub fn publish(kind: Kind) {
    for f in table(kind).iter().chain(all(kind)) {
        f.publish();
    }
    walks(kind).end();
}

/// `cpu`'s own floor: the earliest of its blocked tasks' and its timer caps' deadlines
/// (`u64::MAX`: none). Read without the kernel lock.
pub fn floor(cpu: usize) -> u64 {
    let blocked = BLOCKED_WALKS.stable(|| slot(Kind::Blocked, cpu).get());
    blocked.min(CAP_WALKS.stable(|| slot(Kind::Cap, cpu).get()))
}

/// The gate of the walk over the blocked tasks: the earliest deadline any of them waits with
/// (`u64::MAX`: none). The walk is skipped while the clock is before it.
pub fn blocked_gate() -> u64 {
    BLOCKED_WALKS.stable(|| ALL_BLOCKED.get())
}
