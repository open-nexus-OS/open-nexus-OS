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
        // ADR-0082: a frozen task's wake only ends its block — the thaw enqueues it.
        if task.frozen {
            if let Some(task) = self.task_mut(pid) {
                task.clear_blocked();
            }
            return WakeOutcome::Woken;
        }
        // Selftest dummy tasks intentionally carry a zero frame and no AS.
        // Waking them should validate unblock bookkeeping, but must not
        // make them runnable for the real scheduler path.
        if task.address_space.is_none() && task.frame().sepc == 0 {
            if let Some(task) = self.task_mut(pid) {
                task.clear_blocked();
            }
            return WakeOutcome::WokenNoopSelftest;
        }
        let qos = task.qos;
        // A4/B: the task's home CPU, clamped into its affinity mask and the online set; then
        // TASK-0363 P5: the hart where it runs soonest (`placement`).
        let online = crate::smp::cpu_online();
        let home_cpu = clamp_home_to_affinity(task.affinity, task.home_cpu, online);
        // TASK-0363 P3: a reservation's holder runs on the hart its reservation is admitted on.
        let target = match crate::sched::reserve::hart_of(pid) {
            Some(hart) => crate::types::CpuId::from_raw(hart as u16),
            None => self.placement(home_cpu, task.affinity.intersection(online), qos),
        };
        // No duplicates, then enqueue with the stored QoS. MEASURED: the runqueue half of a
        // wake, what D3's handoff would have removed (TASK-0054C P4c-2).
        if crate::trap::budgets::timed_wake_enqueue(|| {
            scheduler.purge(pid);
            matches!(scheduler.enqueue_on_cpu(target, pid, qos), EnqueueOutcome::Rejected(_))
        }) {
            return WakeOutcome::EnqueueRejected;
        }
        if target != home_cpu {
            // An explicit migration, as a steal's: its home and its device lines follow.
            self.set_home_cpu(pid, target);
            crate::smp::record_wake_placement();
        }
        if let Some(task) = self.task_mut(pid) {
            task.clear_blocked();
            // Cross-core wake: kick the target CPU out of WFI/user so the task
            // runs promptly (best-effort IPI; the evidence chain records it).
            if target != crate::smp::cpu_current_id() {
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                let ipi_t0 = riscv::register::time::read() as u64;
                let _ = crate::smp::request_resched(target);
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                crate::trap::budgets::record_wake_ipi(
                    (riscv::register::time::read() as u64).saturating_sub(ipi_t0),
                );
            }
            return WakeOutcome::Woken;
        }
        WakeOutcome::TaskNotFound
    }

    /// TASK-0363 P5: where a task homed on `home`, allowed on `allowed`, of class `qos` runs
    /// soonest — the rule is `placement::place` (host-tested), fed here with what the machine
    /// shows now: the free harts, the home's cluster, its queue and the class it runs.
    fn placement(
        &self,
        home: crate::types::CpuId,
        allowed: nexus_sched::CpuSet,
        qos: crate::sched::QosClass,
    ) -> crate::types::CpuId {
        let h = home.as_index();
        let running = crate::sched::runs_on(h);
        let home_runs_lower = running.as_raw() != 0
            && self.task(running).is_some_and(|t| (t.qos as u8) < (qos as u8));
        let wake = crate::placement::Wake {
            home: h,
            waker: crate::smp::cpu_current_id().as_index(),
            allowed,
            free: crate::smp::free_harts(),
            near: crate::topology::cluster_of(h),
            backlog: crate::sched::queued_at_or_above(h, qos),
            home_runs_lower,
        };
        crate::types::CpuId::from_raw(crate::placement::place(&wake) as u16)
    }
}
