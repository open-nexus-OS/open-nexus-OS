// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a run-queue entry knows when it became runnable and what its hart ran then:
//! stamped where it enters a queue, read where it is dispatched, the difference recorded per
//! class in `ready_stats` (TASK-0330 P0 — the number TASK-0363's scheduler is judged by,
//! measured before it is built; TASK-0363 P0 — a late dispatch is attributed to what it
//! waited behind). A stolen task keeps its stamp: the wait it had on its victim's queue
//! counts, behind what the victim ran.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `ready_stats` (host); the board's `KINIT: ready latency` lines
//! ADR: docs/adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md

use super::*;

/// The clock a task's wait is stamped with where it enters a run queue (`run_queue` takes
/// it as a number).
#[inline]
pub(super) fn now_ticks() -> u64 {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        riscv::register::time::read() as u64
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        0
    }
}

/// The task leaves its queue for a hart: its wait is recorded for its class.
#[inline]
pub(super) fn dispatched(task: &Task) {
    crate::ready_stats::record(
        task.qos as usize,
        task.ready_at,
        now_ticks(),
        crate::hal::platform::ticks_per_us(),
        task.behind,
    );
}
