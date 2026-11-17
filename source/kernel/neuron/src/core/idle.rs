// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a hart without a task (TASK-0363 P4/P8): whether its idle path needs the kernel lock
//! (read without it), how it sleeps — its timer at its own next deadline or the safety net, never
//! the last task's preemption tick, never another hart's deadline — and the timer interrupt that
//! wakes it in the kernel, counted (`KINIT: idle timer wakes`). The loop around it is `cpu_main`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the arm decision `deadline_rule::sleep_arm` (host); `SELFTEST: wait deadline
//!   ok`, `KINIT: idle timer wakes` (QEMU, board)
//! INVARIANTS: the timer is written with this hart's interrupts masked; writing it never drops a
//!   deadline that is due (`sleep_arm` arms "now" then); a wake hint set before WFI skips it.
//! ADR: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md

use core::sync::atomic::{AtomicU64, Ordering};

use crate::types::CpuId;

/// The boot hart's idle delivery runs at least this often even when nothing is due — a safety
/// net (the liveness witness samples, a line nobody stashed is drained); what it serves wakes it
/// as an event (a deadline, a timer cap, a stashed line, memory pressure).
const HEARTBEAT_NS: u64 = crate::trap::IDLE_SAFETY_NS;

/// When the boot hart last ran its idle delivery (ns).
static LAST_DELIVERY_NS: AtomicU64 = AtomicU64::new(0);

/// The boot hart runs its idle delivery at `now`.
pub(crate) fn note_delivery(now: u64) {
    LAST_DELIVERY_NS.store(now, Ordering::Relaxed);
}

/// The monotonic clock, read without the kernel lock (the platform timer is stateless).
pub(crate) fn now_ns() -> u64 {
    use crate::hal::Timer as _;
    crate::hal::platform::PlatformTimer.now()
}

/// Whether a blocked task's deadline or a timer cap is reached at `now` (both gates are lower
/// bounds: one too low costs a locked look, never a lost expiry).
pub(crate) fn timed_due(now: u64) -> bool {
    now >= crate::trap::budgets::NEXT_DEADLINE_NS.load(Ordering::Relaxed)
        || now >= crate::timer::next_cap_ns()
}

/// Whether this hart's idle path needs the kernel lock now — read without it (TASK-0363 P6): a
/// deadline or a timer cap reached, or a line stashed; on the boot hart also memory pressure, the
/// heartbeat, and a stall the watchdog must name under the lock.
pub(crate) fn lock_due(cpu: CpuId) -> bool {
    let now = now_ns();
    timed_due(now)
        || crate::irq::has_stash(cpu)
        || (cpu.is_boot()
            && (crate::mm::pressure_endpoint::pending()
                || now.saturating_sub(LAST_DELIVERY_NS.load(Ordering::Relaxed)) >= HEARTBEAT_NS
                || crate::liveness::is_stalled(crate::trap::default_tick_cycles() * 3)))
}

/// A walk of the idle path woke `woke` tasks while a deadline read as due: none is a dry look
/// (a gate left low by a wait an event ended), counted with the empty timer wakes.
pub(crate) fn note_walk(cpu: CpuId, due: bool, woke: usize) {
    if due && woke == 0 {
        crate::smp::record_idle_dry(cpu);
    }
}

/// Sleeps until an interrupt: mask, arm the timer, WFI unless a wake hint arrived, unmask.
/// The hint (not RESCHED_PENDING, which the S_SOFT ack path consumes) closes the lost-wakeup
/// window: `request_resched` sets it before the IPI, the external-interrupt trap when it stashed a
/// line, and only this loop consumes it; a pending-but-masked interrupt still wakes WFI. Marked
/// idle, a shootdown skips this hart and it flushes before its next dispatch.
///
/// The timer holds what this hart waits for (TASK-0363 P4/P8): its own next deadline — of a task
/// that blocked on it, of a timer cap armed on it — else the safety net; the boot hart's net is
/// its heartbeat. Another hart's deadline is that hart's: a backstop here woke two harts for one
/// deadline, the second for nothing.
pub(crate) fn sleep(cpu: CpuId) {
    crate::smp::mark_cpu_idle(cpu, true);
    // SAFETY: SIE toggling around wfi; interrupts are handled right after set_sie re-enables them.
    unsafe {
        riscv::register::sie::set_ssoft();
        riscv::register::sie::set_stimer();
        riscv::register::sie::set_sext();
        riscv::register::sstatus::clear_sie();
        let now = now_ns();
        let mut net = now.saturating_add(crate::trap::IDLE_SAFETY_NS);
        if cpu.is_boot() {
            net = net.min(LAST_DELIVERY_NS.load(Ordering::Relaxed).saturating_add(HEARTBEAT_NS));
        }
        let own = crate::trap::own_floor_ns();
        crate::trap::idle_arm(now, own, crate::deadline_rule::sleep_arm(now, own, net));
        if !crate::smp::take_wake_hint(cpu) {
            core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }
        riscv::register::sstatus::set_sie();
    }
    crate::smp::mark_cpu_idle(cpu, false);
}

/// The timer interrupted this hart in the kernel (idle): no tick (TASK-0363 P4) — its pending
/// deadline, else the safety net, is re-armed; the idle loop delivers what is due. Counted:
/// `empty` when nothing was due, a stale one when its armed deadline had passed.
#[cfg(feature = "timer_irq")]
pub(crate) fn on_timer() {
    let now = now_ns();
    let empty = !timed_due(now);
    let armed = crate::trap::armed_deadline_ns();
    let stale = armed != 0 && armed <= now;
    let net = now.saturating_add(crate::trap::IDLE_SAFETY_NS);
    crate::smp::record_idle_timer(crate::smp::cpu_current_id(), now, empty, stale, net);
    crate::trap::timer_arm(crate::hal::platform::ns_to_ticks(crate::trap::IDLE_SAFETY_NS));
}
