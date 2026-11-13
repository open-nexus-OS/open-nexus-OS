// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the running task's class as the trap exit sees it (ADR-0096): whether a task of a
//! class strictly above it waits in this hart's queues — the question every wake's trap exit
//! asks before it returns to the interrupted task — and the running task's class set in place.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `sched/run_queue_tests.rs` (host) + QEMU lanes + the board's
//!   `gpud: gpu pass trace (… late_us=)`
//! ADR: docs/adr/0096-a-wake-preempts-a-task-of-a-higher-class-runs-at-the-trap-exit-the-soft-realtime-chain-is-interactive.md

use super::*;

impl Scheduler {
    /// Whether a task of a class strictly above the running task's waits in this hart's queues
    /// (ADR-0096): the wake that put it there preempts the running task at the trap exit, so
    /// the display and input chain runs within interrupt latency, not within a tick. `false`
    /// when no task runs here (the idle loop rescans its queues itself) and for a wake of the
    /// running task's own class or below (those wait their turn, as before).
    pub fn ready_above_current(&self) -> bool {
        with(Self::current_cpu_index(), |q| q.ready_above_current()) == Some(true)
    }

    /// Updates the currently running task QoS in-place.
    pub fn set_current_qos(&mut self, qos: QosClass) -> bool {
        with(Self::current_cpu_index(), |q| q.set_current_qos(qos)) == Some(true)
    }
}
