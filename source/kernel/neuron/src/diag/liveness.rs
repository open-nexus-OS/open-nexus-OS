// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Lightweight progress watchdog for bring-up and debugging
//! OWNERS: @kernel-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (diagnostic helper; exercised manually when enabled)
//! PUBLIC API: bump(), last_bump_ticks(), check(deadline_ticks)
//! DEPENDS_ON: riscv time CSR (OS), log
//! INVARIANTS: Only emits panic on prolonged stalls; cheap in steady state
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
fn read_time() -> u64 {
    riscv::register::time::read() as u64
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
#[inline(always)]
fn read_time() -> u64 {
    0
}

/// Global epoch incremented on meaningful progress (traps, yields, schedule).
static PROGRESS_EPOCH: AtomicU64 = AtomicU64::new(0);
/// Last tick timestamp a progress bump was observed.
static LAST_BUMP_TICKS: AtomicU64 = AtomicU64::new(0);

/// Bump the global progress epoch and update the last-tick snapshot.
#[inline]
pub fn bump() {
    PROGRESS_EPOCH.fetch_add(1, Ordering::SeqCst);
    LAST_BUMP_TICKS.store(read_time(), Ordering::SeqCst);
}

/// Returns the last observed progress tick timestamp.
#[inline]
pub fn last_bump_ticks() -> u64 {
    LAST_BUMP_TICKS.load(Ordering::SeqCst)
}

/// Checks whether progress advanced in the last `deadline_ticks`. If not,
/// triggers a panic to capture a diagnostic snapshot rather than silently stalling.
#[inline]
pub fn check(deadline_ticks: u64) {
    let last = LAST_BUMP_TICKS.load(Ordering::SeqCst);
    if last == 0 {
        return;
    }
    let now = read_time();
    if now.wrapping_sub(last) > deadline_ticks {
        log_error!(target: "watchdog", "PANIC: watchdog: no progress");
        panic!("watchdog: no progress");
    }
}
