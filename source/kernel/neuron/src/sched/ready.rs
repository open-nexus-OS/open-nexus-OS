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

#[inline]
fn now_ticks() -> u64 {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        riscv::register::time::read() as u64
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        0
    }
}

impl Task {
    /// A task entering a run queue now, on a hart running `current`.
    #[inline]
    pub(super) fn ready(id: TaskId, qos: QosClass, current: Option<&Task>) -> Self {
        let behind =
            crate::ready_stats::behind(current.map(|task| task.qos as usize), qos as usize);
        Self { id, qos, ready_at: now_ticks(), behind }
    }

    /// The task leaves its queue for a hart: its wait is recorded for its class.
    #[inline]
    pub(super) fn dispatched(&self) {
        crate::ready_stats::record(
            self.qos as usize,
            self.ready_at,
            now_ticks(),
            crate::hal::platform::ticks_per_us(),
            self.behind,
        );
    }
}
