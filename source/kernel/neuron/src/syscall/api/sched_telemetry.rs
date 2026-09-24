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
use crate::mm::address_space::AddressSpaceManager;

/// Handles the telemetry ops of `sys_sched`. `Some(_)` = this op was a report
/// and is fully handled; `None` = not a telemetry op, fall through to the real
/// scheduling path.
///
/// OP 5 (two-window): log the bring-up burst maxima, then RESET the accounting
/// so the boot-end gate judges the steady-state window on its own numbers.
/// OP 4: emit the boot-end gate line. Read-only, and called late by the ladder
/// so the report covers the whole service bring-up contention window.
pub(super) fn sched_telemetry_op(
    args: &Args,
    spaces: &AddressSpaceManager,
) -> Option<SysResult<usize>> {
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
            let (ipi_sent, ipi_skipped) = crate::smp::resched_ipi_counts();
            log_info!(
                target: "smp",
                "KINIT: wake ipi max={}us mean={}us n={} sent={} elided={}",
                ipi_max_us,
                ipi_mean_us,
                ipi_n,
                ipi_sent,
                ipi_skipped
            );
            crate::trap::budgets::reset();
            crate::ipc_stats::reset();
        }
        return Some(Ok(0));
    }
    if args.get(0) == 4 {
        // TASK-0286 P5 (RFC-0098 C4): the memory record at the same late fence —
        // pool, page tables, objects and every address space's residency.
        let (m, all) = crate::mm::usage::snapshot(spaces, None);
        log_info!(
            target: "mm",
            "KSELFTEST: mm frames (banks={} total={} free={} reserved={} excluded={} allocs={} frees={} exhausted={} pt_frames={} vmos={} vmo_bytes={} dma_bytes={} spaces={} rss_sum={} rss_max={})",
            m.banks,
            m.total,
            m.free,
            m.reserved,
            m.excluded,
            m.allocs,
            m.frees,
            m.exhausted,
            m.pt_frames,
            m.vmos,
            m.vmo_bytes,
            m.dma_bytes,
            all.spaces,
            all.rss_sum,
            all.rss_max
        );
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            // TASK-0054C P1 (RFC-0096 Phase 1): the IPC path as numbers over the
            // steady-state window. No verdict — the budgets are calibrated from
            // these lines before P4 asserts them. `handoff_hit` is 0 by
            // construction until P4 lands (no direct handoff exists), so every
            // receiver wake is a handoff miss.
            let ipc = crate::ipc_stats::report();
            // TASK-0054C P3b: kernel heap bytes in use at the same fence. The inline tier makes
            // `Message` bigger to make most messages allocation-free, and a `Message` is moved on
            // every send, push, pop and error return — so whether the tier PAYS has to be a
            // number, not an argument. Nothing else prints this; only the OOM handler read it.
            let kheap_used = crate::heap_used_bytes();
            // TASK-0054C P4c-2: `handoff_hit` is GONE from this line rather than printed as a
            // hardcoded 0 — a field that cannot be non-zero is not telemetry, it is decoration.
            // What replaces it is the cost of the thing D3 would remove: mean/max ticks in the
            // runqueue half of a wake.
            let (wake_enq_max, wake_enq_mean, _) = crate::trap::budgets::wake_enqueue_report();
            log_info!(
                target: "ipc",
                "KSELFTEST: ipc stats (sends={} heap_allocs={} copies={} copy_bytes={} wake_ipis={} handoff_miss={} kheap_used={} calls_in_regs={} wake_enq_ticks={}/{})",
                ipc.sends,
                ipc.payload_allocs,
                ipc.payload_copies,
                ipc.payload_copy_bytes,
                ipc.wake_ipis,
                ipc.recv_wakes,
                kheap_used,
                ipc.calls_in_regs,
                wake_enq_mean,
                wake_enq_max
            );
            // TASK-0054C P3a: the payload-size distribution, so the inline tier P3b builds is
            // sized by measurement and not by the guess RFC-0096 wrote down. An average cannot
            // choose it — many tiny frames and a few large ones average like all-medium ones.
            let h = ipc.payload_hist;
            log_info!(
                target: "ipc",
                "KSELFTEST: ipc payload hist (zero={} le32={} le64={} le128={} le256={} le512={} le1k={} gt1k={})",
                h[0],
                h[1],
                h[2],
                h[3],
                h[4],
                h[5],
                h[6],
                h[7]
            );
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
