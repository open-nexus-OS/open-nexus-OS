// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SMP runtime coordination flags — runtime-ready release, WFI wake
//! hints, spawn placement, steal rate gate, per-hart tick/dispatch bookkeeping
//! (A3/A4/A7/A8). The per-hart fields live in `hart_state` (one record per CPU).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (smp exec cpuN ok, per-hart ticks ok)
//! INVARIANTS: hints/flags are per-CPU atomics consumed only by the owning
//!   hart's cpu_main; bounded, no allocation.
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::types::CpuId;

use super::{cpu_current_id, cpu_online, hart_state};

// ——— A3/A4/A8 runtime state ———

/// Set by the boot hart once kernel selftests + init spawn are complete;
/// secondaries must not touch the scheduler before this (the selftest phase
/// mutates kernel state through kmain's direct borrows, not the BKL).
static RUNTIME_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn mark_runtime_ready() {
    RUNTIME_READY.store(true, Ordering::Release);
}

#[inline]
pub fn runtime_ready() -> bool {
    RUNTIME_READY.load(Ordering::Acquire)
}

/// Counts a user-task dispatch `cpu_main` performed on `cpu` (A3 proof: written only by the
/// owning hart with tp-derived identity).
#[inline]
pub fn record_user_dispatch(cpu: CpuId) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.user_dispatches.fetch_add(1, Ordering::AcqRel);
    }
}

/// Total user dispatches across every hart. The liveness watchdog's progress epoch is bumped by
/// EVERY trap (a timer tick included), so it cannot tell "the fleet is wedged" from "the fleet is
/// quietly waiting". This counter only moves when a task actually ran (TASK-0324 P5-a).
#[inline]
pub fn user_dispatch_total() -> usize {
    hart_state::all().fold(0usize, |total, (_, hart)| {
        total.saturating_add(hart.user_dispatches.load(Ordering::Acquire))
    })
}

/// Records a supervisor timer tick for `cpu` and returns the previous count (A7 proof:
/// written by the owning hart's S_TIMER trap; the first tick on a secondary hart emits the
/// per-hart-ticks marker — every online hart has a live preemption tick).
#[inline]
pub fn record_timer_tick(cpu: CpuId) -> usize {
    hart_state::of(cpu).map_or(0, |hart| hart.timer_ticks.fetch_add(1, Ordering::AcqRel))
}

/// Consumes this hart's wake hint; `cpu_main` skips WFI when it was set.
///
/// WFI wake hints (A4) are set by `request_resched` and consumed ONLY by the target's
/// `cpu_main` idle path. The resched-pending flag cannot serve this purpose — the S_SOFT
/// handler consumes it for the ack evidence chain, which races a hart into WFI with a freshly
/// filled queue (lost wakeup).
#[inline]
pub fn take_wake_hint(cpu: CpuId) -> bool {
    hart_state::of(cpu).is_some_and(|hart| hart.wake_hint.swap(0, Ordering::AcqRel) != 0)
}

/// Tells `cpu`'s idle loop not to sleep: something it must look at arrived without a
/// reschedule request (a device line its own external-interrupt trap claimed).
#[inline]
pub fn set_wake_hint(cpu: CpuId) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.wake_hint.store(1, Ordering::Release);
    }
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
    let online = cpu_online();
    if online.count() <= 1 {
        return cpu_current_id();
    }
    let turn = SPAWN_RR.fetch_add(1, Ordering::AcqRel) % online.count();
    online.iter().nth(turn).map_or(CpuId::BOOT, |cpu| CpuId::from_raw(cpu as u16))
}

/// A8 rate gate: allow at most one steal attempt per `min_interval_ns` per CPU.
pub fn steal_rate_gate(cpu: CpuId, now_ns: u64, min_interval_ns: u64) -> bool {
    let Some(hart) = hart_state::of(cpu) else {
        return false;
    };
    let last = hart.last_steal_ns.load(Ordering::Acquire);
    if now_ns.saturating_sub(last) < min_interval_ns {
        return false;
    }
    hart.last_steal_ns.store(now_ns, Ordering::Release);
    true
}
