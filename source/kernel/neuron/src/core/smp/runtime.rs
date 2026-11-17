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

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

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

/// TASK-0363 P4: a timer interrupt woke `cpu` in the kernel (idle) at `now_ns`; `empty` when
/// nothing was due. An empty wake at or after the net `cpu` armed is its safety net; one before
/// is stray: `deadline_fired` when the hart's armed deadline had passed (a deadline nobody waited
/// for any more — its waiter woken by an event, or served by another hart), else blind (the
/// compare register held neither — a tick that slipped into the idle loop). The handler re-arms
/// the net after it: `next_net_ns`.
#[inline]
pub fn record_idle_timer(
    cpu: CpuId,
    now_ns: u64,
    empty: bool,
    deadline_fired: bool,
    next_net_ns: u64,
) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.idle_timer_wakes.fetch_add(1, Ordering::Relaxed);
        if empty {
            hart.idle_timer_empty.fetch_add(1, Ordering::Relaxed);
            let net_at = hart.idle_net_at_ns.load(Ordering::Relaxed);
            if net_at != 0 && now_ns >= net_at {
                hart.idle_timer_net.fetch_add(1, Ordering::Relaxed);
            } else if deadline_fired {
                hart.idle_timer_stale.fetch_add(1, Ordering::Relaxed);
            }
        }
        hart.idle_net_at_ns.store(next_net_ns, Ordering::Relaxed);
    }
}

/// TASK-0363 P8: an idle walk on `cpu` found a deadline due and woke nothing — a gate left low
/// by a wait an event ended; counted with the empty timer wakes.
#[inline]
pub fn record_idle_dry(cpu: CpuId) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.idle_timer_dry.fetch_add(1, Ordering::Relaxed);
    }
}

/// TASK-0363 P4: `cpu` goes to sleep with its compare register at its safety net due `at_ns`
/// (0: at a deadline) — what [`record_idle_timer`] tells a net from a stray wake by.
#[inline]
pub fn note_idle_net(cpu: CpuId, at_ns: u64) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.idle_net_at_ns.store(at_ns, Ordering::Relaxed);
    }
}

/// The window's idle timer wakes, summed over the harts; the empty ones and of them the safety
/// nets; the dry looks; the hart with the most empty ones (dry looks included), and the most
/// stray ones on any hart (empty or dry, not the net); the counts start again.
pub fn take_idle_timer_window() -> IdleTimerWindow {
    let mut w = IdleTimerWindow::default();
    for (cpu, hart) in hart_state::all() {
        w.wakes += hart.idle_timer_wakes.swap(0, Ordering::Relaxed);
        let d = hart.idle_timer_dry.swap(0, Ordering::Relaxed);
        let e = hart.idle_timer_empty.swap(0, Ordering::Relaxed) + d;
        let n = hart.idle_timer_net.swap(0, Ordering::Relaxed);
        w.stale += hart.idle_timer_stale.swap(0, Ordering::Relaxed);
        w.dry += d;
        w.empty += e;
        w.net += n;
        if e > w.max_empty {
            (w.max_empty, w.at) = (e, cpu);
        }
        w.max_stray = w.max_stray.max(e.saturating_sub(n));
    }
    w
}

/// One window of [`take_idle_timer_window`].
#[derive(Clone, Copy, Default)]
pub struct IdleTimerWindow {
    pub wakes: usize,
    pub empty: usize,
    pub net: usize,
    /// Stray wakes at a passed deadline.
    pub stale: usize,
    /// Idle walks that found a deadline due and woke nothing (in `empty`).
    pub dry: usize,
    pub max_empty: usize,
    pub at: usize,
    pub max_stray: usize,
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

/// TASK-0363 P5: the harts asleep with no wake already sent to them — where a woken task runs at
/// once. A hart a placement chose a moment ago carries its wake hint until it looks, so two wakes
/// in a row do not both pick it.
pub fn free_harts() -> nexus_sched::CpuSet {
    let mut free = super::cpu_idle();
    for (cpu, hart) in hart_state::all() {
        let woken = hart.wake_hint.load(Ordering::Acquire) != 0;
        if let Some(one) = nexus_sched::CpuSet::single(cpu).filter(|_| woken) {
            free = free.without(one);
        }
    }
    free
}

/// TASK-0363 P5: wakes placed on another hart than the task's home (`task::wake`).
static WAKE_PLACEMENTS: AtomicUsize = AtomicUsize::new(0);

/// A wake placed its task away from its home.
#[inline]
pub fn record_wake_placement() {
    WAKE_PLACEMENTS.fetch_add(1, Ordering::Relaxed);
}

/// Wakes placed away from their home since boot.
pub fn wake_placement_count() -> usize {
    WAKE_PLACEMENTS.load(Ordering::Relaxed)
}

/// TASK-0363 P5: each hart's time asleep (ns, `cpu_main::idle::sleep`).
pub fn record_idle_sleep(cpu: CpuId, ns: u64) {
    if let Some(hart) = hart_state::of(cpu) {
        hart.idle_sleep_ns.fetch_add(ns, Ordering::Relaxed);
    }
}

/// When the busy window started (ns; 0: at boot, where the sleep counts start too).
static BUSY_WINDOW_NS: AtomicU64 = AtomicU64::new(0);

/// The busy share per hart in permille since the last call, which ended at `now_ns` (the sleep
/// counts start again): `(window_ns, min, at_min, max, at_max, mean)`.
pub fn take_busy_window(now_ns: u64) -> (u64, u64, usize, u64, usize, u64) {
    let window_ns = now_ns.saturating_sub(BUSY_WINDOW_NS.swap(now_ns, Ordering::Relaxed));
    let (mut min, mut at_min, mut max, mut at_max, mut sum, mut n) = (u64::MAX, 0, 0, 0, 0, 0);
    for (cpu, hart) in hart_state::all() {
        let slept = hart.idle_sleep_ns.swap(0, Ordering::Relaxed).min(window_ns);
        let busy = (window_ns - slept).saturating_mul(1000) / window_ns.max(1);
        if busy < min {
            (min, at_min) = (busy, cpu);
        }
        if busy >= max {
            (max, at_max) = (busy, cpu);
        }
        sum += busy;
        n += 1;
    }
    (window_ns, min.min(max), at_min, max, at_max, sum / n.max(1))
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
