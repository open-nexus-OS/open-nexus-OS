// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//!
//! CONTEXT: `sys_sched` telemetry ops — the read-only BKL / deadline-sweep /
//!          wake-IPI reports the selftest ladder asks for by op number. Split
//!          out of `sched_task.rs` (TASK-0306) so the scheduling syscalls stay
//!          scheduling syscalls and the reporting can grow with the
//!          instrumentation without pushing that file over the size ratchet.
//! OWNERS: @kernel
//! STATUS: Functional
//! API_STABILITY: Unstable — diagnostic surface, not an ABI promise
//! TEST_COVERAGE: QEMU marker ladder (`KSELFTEST: bkl budget ok`)
//! ADR: docs/rfcs/RFC-0033-soft-real-time-spine.md

use super::{Args, SysResult};

/// Handles the telemetry ops of `sys_sched`. `Some(_)` = this op was a report
/// and is fully handled; `None` = not a telemetry op, fall through to the real
/// scheduling path.
///
/// OP 5 (two-window): log the bring-up burst maxima, then RESET the accounting
/// so the boot-end gate judges the steady-state window on its own numbers.
/// OP 4: emit the boot-end gate line. Read-only, and called late by the ladder
/// so the report covers the whole service bring-up contention window.
pub(super) fn sched_telemetry_op(args: &Args) -> Option<SysResult<usize>> {
    // OP 4 (P0, declarative budgets SSOT in core/trap/budgets.rs): emit the
    // boot-end BKL budget gate line. Read-only; callable late by the selftest
    // ladder so the report COVERS the service bring-up contention window.
    // OP 5 (P0 two-window): log the bring-up burst maxima, then reset the
    // accounting so the boot-end gate judges the steady-state window.
    if args.get(0) == 5 {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let (_, wait_us, hold_ms, nr, b) = crate::trap::budgets::budget_report();
            log_info!(
                target: "smp",
                "KINIT: bkl bring-up burst max_wait={}us max_hold={}ms nr={} gt10ms={}",
                wait_us,
                hold_ms,
                nr,
                b[3]
            );
            let (
                sweep_us,
                sweep_tasks,
                sweep_calls,
                sweep_mean_us,
                sweep_skipped,
                sweep_wakes,
                sweep_wake_us,
            ) = crate::trap::budgets::sweep_report();
            log_info!(
                target: "smp",
                "KINIT: sweep bring-up max={}us tasks={} calls={} mean={}us skipped={} wakes={} wakeus={}",
                sweep_us,
                sweep_tasks,
                sweep_calls,
                sweep_mean_us,
                sweep_skipped,
                sweep_wakes,
                sweep_wake_us
            );
            let (ipi_max_us, ipi_mean_us, ipi_n) = crate::trap::budgets::wake_ipi_report();
            log_info!(
                target: "smp",
                "KINIT: wake ipi max={}us mean={}us n={}",
                ipi_max_us,
                ipi_mean_us,
                ipi_n
            );
            crate::trap::budgets::reset();
        }
        return Some(Ok(0));
    }
    if args.get(0) == 4 {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let (ok, wait_us, hold_ms, nr, b) = crate::trap::budgets::budget_report();
            log_info!(
                target: "smp",
                "KINIT: bkl histogram le100us={} le1ms={} le10ms={} gt10ms={}",
                b[0],
                b[1],
                b[2],
                b[3]
            );
            let (
                sweep_us,
                sweep_tasks,
                sweep_calls,
                sweep_mean_us,
                sweep_skipped,
                sweep_wakes,
                sweep_wake_us,
            ) = crate::trap::budgets::sweep_report();
            log_info!(
                target: "smp",
                "KINIT: sweep steady max={}us tasks={} calls={} mean={}us skipped={} wakes={} wakeus={}",
                sweep_us,
                sweep_tasks,
                sweep_calls,
                sweep_mean_us,
                sweep_skipped,
                sweep_wakes,
                sweep_wake_us
            );
            let (ipi_max_us, ipi_mean_us, ipi_n) = crate::trap::budgets::wake_ipi_report();
            log_info!(
                target: "smp",
                "KINIT: wake ipi max={}us mean={}us n={}",
                ipi_max_us,
                ipi_mean_us,
                ipi_n
            );
            if ok {
                log_info!(
                    target: "smp",
                    "KSELFTEST: bkl budget ok (max_wait={}us max_hold={}ms nr={})",
                    wait_us,
                    hold_ms,
                    nr
                );
            } else {
                log_error!(
                    target: "smp",
                    "KSELFTEST: bkl budget FAIL max_wait={}us max_hold={}ms nr={}",
                    wait_us,
                    hold_ms,
                    nr
                );
            }
        }
        return Some(Ok(0));
    }
    None
}
