// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the user page fault's entry before the kernel lock (TASK-0330 P3). A missing lazy
//! page is the common fault — every stack, every `.bss` tail, every fresh heap page — and it
//! touches one address space and one memory object: the thread's own space is held to look and
//! to install, the frame comes from the hart's magazine and is zeroed with no lock held
//! (`mm::populate::fill_free`). Everything else stays with the locked path, which starts the
//! fault from the beginning: a real violation (reported, killed), a squeezed or swapped page,
//! an exhausted pool, an order waiting for the thread, a pressure level to deliver.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every boot (`KINIT: mm faults (free= locked= …)` shows which path the
//!   faults took), the rank-check lanes (no ranked lock is held past a fault)
//! INVARIANTS: `false` leaves nothing behind the locked path must undo; the trap frame is not
//!   touched — a filled fault returns to the same instruction

use super::*;
use crate::lazy::Access;
use crate::mm::{AddressSpaceManager, AsHandle};
use crate::task::Pid;
use core::sync::atomic::{AtomicU64, Ordering};

/// The address spaces: `kmain`'s, for the kernel's whole life.
struct Spaces(NonNull<AddressSpaceManager>);

// SAFETY: reached through a shared reference only (the kernel lock's holder gets the same),
// every space behind its own lock, the registry behind its own; `kmain` never drops it.
unsafe impl Send for Spaces {}
// SAFETY: as above.
unsafe impl Sync for Spaces {}

static SPACES: spin::Once<Spaces> = spin::Once::new();
/// Faults filled here, faults handed to the locked path, and the longest fill here (ticks) in
/// the window.
static FREE: AtomicU64 = AtomicU64::new(0);
static LOCKED: AtomicU64 = AtomicU64::new(0);
static LONGEST: AtomicU64 = AtomicU64::new(0);

/// Names the address spaces, once, when the trap runtime is installed.
pub(super) fn install(spaces: &AddressSpaceManager) {
    SPACES.call_once(|| Spaces(NonNull::from(spaces)));
}

/// Fills the page at `va` for the user thread this hart runs, without the kernel lock. `true`:
/// the page is accessible, the trap returns to the instruction. `false`: untouched, the locked
/// path takes the fault whole.
pub(super) fn serve(va: usize, access: Access) -> bool {
    let filled = fill(va, access);
    if filled { &FREE } else { &LOCKED }.fetch_add(1, Ordering::Relaxed);
    filled
}

fn fill(va: usize, access: Access) -> bool {
    // A hart before its scheduler loop runs the boot's own flow; a pressure level waits for a
    // holder of the kernel lock to deliver it — the locked path is one.
    let me = crate::sched::runs_here();
    if me == Pid::KERNEL
        || !crate::cpu_main::sched_loop_entered(crate::smp::cpu_current_id())
        || crate::mm::pressure_endpoint::pending()
    {
        return false;
    }
    let (Some(spaces), Some(rec)) = (SPACES.get(), crate::sched::record::get(me)) else {
        return false;
    };
    // ADR-0082: an order left for this thread (a kill, a freeze) is the locked path's.
    if rec.pending().is_some() {
        return false;
    }
    let Some(handle) = rec.space().and_then(|view| AsHandle::from_raw(view.raw)) else {
        return false;
    };
    let t0 = crate::arch::riscv::read_time();
    // SAFETY: `install` named a live manager that `kmain` never drops; a shared reference.
    let filled = crate::mm::populate::fill_free(unsafe { spaces.0.as_ref() }, handle, va, access);
    assert!(crate::sync::ranked::holds_none(), "ranked lock held past a page fault");
    LONGEST.fetch_max(crate::arch::riscv::read_time().wrapping_sub(t0), Ordering::Relaxed);
    filled
}

/// `(faults filled without the kernel lock, faults the locked path took, the longest fill
/// without it in ticks)` since the last call.
pub(super) fn take_window() -> (u64, u64, u64) {
    (
        FREE.swap(0, Ordering::Relaxed),
        LOCKED.swap(0, Ordering::Relaxed),
        LONGEST.swap(0, Ordering::Relaxed),
    )
}
