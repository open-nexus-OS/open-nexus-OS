// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: waking a blocked task. Split out of `task/mod.rs` when TASK-0054C
//! P4c-2 instrumented the runqueue half of a wake: the module sat at its
//! structure-gate ceiling, and waking is its own concern — every IPC send,
//! every timer fire and every child exit ends here.
//!
//! The measured part is `timed_wake_enqueue`: the purge + enqueue that D3's
//! direct handoff proposed to remove. It costs 0.40 µs (peak 0.9), which is
//! 0.41 % of an exchange, against the 5.6–7.7 % that merging the two traps
//! already bought — which is why the handoff was withdrawn rather than built
//! (RFC-0096 §Amendment 2026-09-17).
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the scheduler/IPC ladders in QEMU

use super::affinity::clamp_home_to_affinity;
use super::{Pid, Scheduler, TaskTable, WakeOutcome};
use crate::sched::EnqueueOutcome;

impl TaskTable {
    /// Wakes a blocked task and enqueues it for execution on its home CPU.
    ///
    /// Uses per-CPU runqueues to prevent cross-CPU migration during IPC wakeups,
    /// which would otherwise cause starvation under SMP.
    pub fn wake(&mut self, pid: Pid, scheduler: &mut Scheduler) -> WakeOutcome {
        // Bounded bring-up diagnostic (A3/A4): callers ignore wake outcomes;
        // a silently failing wake stalls IPC for a full heartbeat. Surface
        // the first few anomalies.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        fn log_wake_anomaly(pid: Pid, what: &str) {
            static WAKE_ANOMALY_LOGGED: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            if WAKE_ANOMALY_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 6 {
                log_info!(target: "smp", "KINIT: wake pid={} {}", pid.as_raw(), what);
            }
        }
        let Some(task) = self.task(pid) else {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            log_wake_anomaly(pid, "not-found");
            return WakeOutcome::TaskNotFound;
        };
        if !task.blocked {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            log_wake_anomaly(pid, "not-blocked");
            return WakeOutcome::TaskNotBlocked;
        }
        // Selftest dummy tasks intentionally carry a zero frame and no AS.
        // Waking them should validate unblock bookkeeping, but must not
        // make them runnable for the real scheduler path.
        if task.address_space.is_none() && task.frame.sepc == 0 {
            if let Some(task) = self.task_mut(pid) {
                task.clear_blocked();
            }
            return WakeOutcome::WokenNoopSelftest;
        }
        let qos = task.qos;
        // A4/B: route to the task's home CPU, clamped into its affinity mask
        // and the online set.
        let home_cpu = clamp_home_to_affinity(
            task.affinity_mask,
            task.home_cpu,
            crate::smp::cpu_online_mask(),
        );
        // No duplicates, then enqueue with the stored QoS. MEASURED: the runqueue half of a
        // wake, what D3's handoff would have removed (TASK-0054C P4c-2).
        if crate::trap::budgets::timed_wake_enqueue(|| {
            scheduler.purge(pid);
            matches!(scheduler.enqueue_on_cpu(home_cpu, pid, qos), EnqueueOutcome::Rejected(_))
        }) {
            return WakeOutcome::EnqueueRejected;
        }
        if let Some(task) = self.task_mut(pid) {
            task.clear_blocked();
            // Cross-core wake: kick the home CPU out of WFI/user so the task
            // runs promptly (best-effort IPI; the evidence chain records it).
            if home_cpu != crate::smp::cpu_current_id() {
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                let ipi_t0 = riscv::register::time::read() as u64;
                let _ = crate::smp::request_resched(home_cpu);
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                crate::trap::budgets::record_wake_ipi(
                    (riscv::register::time::read() as u64).saturating_sub(ipi_t0),
                );
            }
            return WakeOutcome::Woken;
        }
        WakeOutcome::TaskNotFound
    }
}
