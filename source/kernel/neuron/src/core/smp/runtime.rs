// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SMP runtime coordination flags — runtime-ready release, WFI wake
//! hints, lazy TLB flush flags, spawn placement, steal rate gate, per-hart
//! tick/dispatch bookkeeping (A3/A4/A7/A8).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (smp exec cpuN ok, per-hart ticks ok)
//! INVARIANTS: hints/flags are per-CPU atomics consumed only by the owning
//!   hart's cpu_main; bounded, no allocation.
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::types::CpuId;

use super::{cpu_current_id, cpu_online_mask, MAX_CPUS};

// ——— A3/A4/A8 runtime state ———

/// Set by the boot hart once kernel selftests + init spawn are complete;
/// secondaries must not touch the scheduler before this (the selftest phase
/// mutates kernel state through kmain's direct borrows, not the BKL).
static RUNTIME_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Per-CPU count of user-task dispatches performed by `cpu_main` (A3 proof:
/// written only by the owning hart with tp-derived identity).
static USER_DISPATCHES: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

/// Work-steal rate gate (A8): last steal attempt timestamp per CPU.
static LAST_STEAL_NS: [core::sync::atomic::AtomicU64; MAX_CPUS] =
    [const { core::sync::atomic::AtomicU64::new(0) }; MAX_CPUS];

pub fn mark_runtime_ready() {
    RUNTIME_READY.store(true, Ordering::Release);
}

#[inline]
pub fn runtime_ready() -> bool {
    RUNTIME_READY.load(Ordering::Acquire)
}

#[inline]
pub fn record_user_dispatch(cpu: CpuId) {
    let idx = cpu.as_index();
    if idx < MAX_CPUS {
        USER_DISPATCHES[idx].fetch_add(1, Ordering::AcqRel);
    }
}

/// Per-hart supervisor timer tick counters (A7): written by the owning
/// hart's S_TIMER trap; proves every online hart has a live preemption tick.
static TIMER_TICKS: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

/// Records a timer tick for `cpu` and returns the previous count (A7 proof:
/// the first tick on a secondary hart emits the per-hart-ticks marker).
#[inline]
pub fn record_timer_tick(cpu: CpuId) -> usize {
    let idx = cpu.as_index();
    if idx < MAX_CPUS {
        TIMER_TICKS[idx].fetch_add(1, Ordering::AcqRel)
    } else {
        0
    }
}

/// WFI wake hints (A4): set by `request_resched`, consumed ONLY by the
/// target's `cpu_main` idle path. The RESCHED_PENDING flag cannot serve this
/// purpose — the S_SOFT handler consumes it for the ack evidence chain, which
/// races a hart into WFI with a freshly filled queue (lost wakeup).
static WAKE_HINT: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

/// Marks a wake hint for `cpu_idx` (called by `request_resched` before the IPI).
#[inline]
pub(super) fn set_wake_hint(cpu_idx: usize) {
    if cpu_idx < MAX_CPUS {
        WAKE_HINT[cpu_idx].store(1, Ordering::Release);
    }
}

/// Consumes this hart's wake hint; `cpu_main` skips WFI when it was set.
#[inline]
pub fn take_wake_hint(cpu: CpuId) -> bool {
    let idx = cpu.as_index();
    idx < MAX_CPUS && WAKE_HINT[idx].swap(0, Ordering::AcqRel) != 0
}

/// Deterministic round-robin cursor for initial task placement (A4).
static SPAWN_RR: AtomicUsize = AtomicUsize::new(0);

/// Initial-placement policy v1 (A4): before the runtime is released, every
/// spawn stays on the spawning hart (selftest children must run immediately
/// on the boot hart). Afterwards, spawns round-robin across online CPUs —
/// deterministic given the deterministic init spawn order, and identical to
/// the pre-SMP behavior under SMP=1. Phase B replaces this with affinity
/// masks + QoS budgets (TASK-0042).
pub fn assign_spawn_cpu() -> CpuId {
    if !runtime_ready() {
        return cpu_current_id();
    }
    let mask = cpu_online_mask();
    let count = mask.count_ones() as usize;
    if count <= 1 {
        return cpu_current_id();
    }
    let n = SPAWN_RR.fetch_add(1, Ordering::AcqRel);
    let mut k = n % count;
    for idx in 0..MAX_CPUS {
        if mask & (1 << idx) != 0 {
            if k == 0 {
                return CpuId::from_raw(idx as u16);
            }
            k -= 1;
        }
    }
    CpuId::BOOT
}

/// A8 rate gate: allow at most one steal attempt per `min_interval_ns` per CPU.
pub fn steal_rate_gate(cpu: CpuId, now_ns: u64, min_interval_ns: u64) -> bool {
    let idx = cpu.as_index();
    if idx >= MAX_CPUS {
        return false;
    }
    let last = LAST_STEAL_NS[idx].load(Ordering::Acquire);
    if now_ns.saturating_sub(last) < min_interval_ns {
        return false;
    }
    LAST_STEAL_NS[idx].store(now_ns, Ordering::Release);
    true
}
