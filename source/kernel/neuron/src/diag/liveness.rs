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
    if is_stalled(deadline_ticks) {
        log_error!(target: "watchdog", "PANIC: watchdog: no progress");
        panic!("watchdog: no progress");
    }
}

/// Non-panicking stall predicate: true when no progress bump occurred in the last
/// `deadline_ticks` (and the watchdog is armed). Lets the caller capture a labeled
/// diagnostic snapshot (blocked-task set) BEFORE panicking, instead of a bare panic.
#[inline]
#[must_use]
pub fn is_stalled(deadline_ticks: u64) -> bool {
    let last = LAST_BUMP_TICKS.load(Ordering::SeqCst);
    if last == 0 {
        return false;
    }
    read_time().wrapping_sub(last) > deadline_ticks
}

/// Watchdog diagnostic: dump every task's schedulable / blocked state so a
/// fleet-collapse "no progress" panic names WHO is blocked on WHAT (endpoint /
/// waitset / fence + deadline). Turns a silent early-boot park into a labeled
/// snapshot for root-causing the lost-wakeup. Bounded — one line per task.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn dump_snapshot(tasks: &crate::task::TaskTable, now_ns: u64) {
    let n = tasks.len();
    log_error!(target: "watchdog", "liveness snapshot: now_ns={} tasks={}", now_ns, n);
    for i in 0..n {
        let Some(t) = tasks.task(crate::task::Pid::from_raw(i as u32)) else {
            continue;
        };
        match t.block_reason() {
            Some(r) => {
                log_error!(target: "watchdog", "  pid={} state={:?} BLOCKED {:?}", i, t.state(), r)
            }
            None => log_error!(target: "watchdog", "  pid={} state={:?} runnable", i, t.state()),
        }
    }
}

/// Names a wedged fleet (TASK-0324 P5-a, ADR-0062): the progress epoch above is bumped by EVERY
/// trap — a timer tick included — so `is_stalled` cannot tell a wedged fleet from one that waits
/// quietly. This witness names the wedge: no task dispatched for [`QUIET_STALL_NS`], every online
/// hart idle, and at least one task blocked — nobody can progress and nobody will be woken. A
/// quiet but healthy system keeps dispatching (timers, IPC), so it never trips this. Latched once
/// per boot and FAIL-shaped, so the harness FAIL gate fails the lane instead of hiding the stall
/// behind a missing marker further down the ladder.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn quiet_stall_witness(cpu: crate::types::CpuId, tasks: &crate::task::TaskTable, now_ns: u64) {
    use core::sync::atomic::{AtomicBool, AtomicUsize};

    /// Long enough that a slow policy-gated grant round (~100 ms) never trips it.
    const QUIET_STALL_NS: u64 = 2_000_000_000;
    static LAST_DISPATCHES: AtomicUsize = AtomicUsize::new(0);
    static LAST_CHANGE_NS: AtomicU64 = AtomicU64::new(0);
    static REPORTED: AtomicBool = AtomicBool::new(false);

    if !crate::smp::runtime_ready() || REPORTED.load(Ordering::Relaxed) {
        return;
    }
    let dispatches = crate::smp::user_dispatch_total();
    if dispatches != LAST_DISPATCHES.swap(dispatches, Ordering::Relaxed)
        || LAST_CHANGE_NS.load(Ordering::Relaxed) == 0
    {
        LAST_CHANGE_NS.store(now_ns, Ordering::Relaxed);
        return;
    }
    if now_ns.saturating_sub(LAST_CHANGE_NS.load(Ordering::Relaxed)) < QUIET_STALL_NS {
        return;
    }
    // This hart is about to idle; every OTHER online hart must already be idle.
    let online = crate::smp::cpu_online_mask();
    let idle = crate::smp::cpu_idle_mask() | (1usize << cpu.as_index());
    if online & !idle != 0 {
        return;
    }
    let blocked = (0..tasks.len())
        .filter(|i| {
            tasks.task(crate::task::Pid::from_raw(*i as u32)).is_some_and(|t| t.is_blocked())
        })
        .count();
    if blocked == 0 || REPORTED.swap(true, Ordering::AcqRel) {
        return;
    }
    log_error!(
        target: "selftest",
        "KSELFTEST: liveness snapshot FAIL quiet-stall since_ms={} harts=0x{:x} blocked={}",
        now_ns.saturating_sub(LAST_CHANGE_NS.load(Ordering::Relaxed)) / 1_000_000,
        online,
        blocked
    );
    dump_snapshot(tasks, now_ns);
}
