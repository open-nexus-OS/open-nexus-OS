// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Syscall handlers exposed to the dispatcher
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! PUBLIC API: install_handlers(table), Context, Args, SysResult
//! DEPENDS_ON: sched::Scheduler, task::TaskTable, ipc::Router, mm::AddressSpaceManager
//! INVARIANTS: Stable syscall IDs; Decode→Check→Execute pattern; W^X for user mappings
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::vec::Vec;
use core::cmp;
use core::ptr;

use crate::types::{PageLen, SlotIndex, VirtAddr};
use crate::{
    cap::{CapError, Capability, CapabilityKind, Rights},
    hal::Timer,
    ipc::{self, header::MessageHeader},
    mm::{
        AddressSpaceError, AddressSpaceManager, AsHandle, MapError, PageFlags, PAGE_SIZE,
        USER_VMO_ARENA_BASE, USER_VMO_ARENA_LEN,
    },
    sched::{QosClass, Scheduler, SetQosOutcome},
    task,
};
use core::slice;
use spin::Mutex;

use crate::task::BlockReason;

// Mechanical split of the former single-file api.rs (TASK: god-file split).
// Every item stays reachable as `crate::syscall::api::*` via the re-imports
// below; submodule-private helpers are widened to pub(super) only.
mod caps;
mod exec;
mod ipc_msg;
mod ipc_recv_v2;
mod sched_task;
mod sched_telemetry;
mod sync_objects;
mod task_image;
mod vm_map;
mod vmo;
mod vmo_pool;

#[cfg(test)]
mod tests;

use caps::*;
use exec::*;
pub(crate) use exec::{exec_phase_a, exec_v2_phase_a, run_copy_plan, CopyPlan};
use ipc_msg::*;
use ipc_recv_v2::*;
use sched_task::*;
use sync_objects::*;
pub(crate) use task_image::exit_current_and_release;
use vm_map::*;
pub use vmo::vmo_idle_zero_step;
use vmo::*;
pub(crate) use vmo::{vmo_create_finish, vmo_create_reserve};
use vmo_pool::*;

pub(crate) use sched_task::selftest_sched_op;

use super::{
    Args, Error, SysResult, SyscallTable, SYSCALL_AS_CREATE, SYSCALL_AS_MAP, SYSCALL_AS_SELF,
    SYSCALL_BOOT_DISPLAY_MODE, SYSCALL_BOOT_MODE, SYSCALL_CAP_QUERY, SYSCALL_CAP_TRANSFER,
    SYSCALL_CAP_TRANSFER_TO, SYSCALL_DEBUG_PUTC, SYSCALL_DEBUG_WRITE, SYSCALL_DEVICE_CAP_CREATE,
    SYSCALL_EXEC, SYSCALL_EXEC_V2, SYSCALL_EXIT, SYSCALL_IPC_ENDPOINT_CREATE, SYSCALL_IPC_RECV_V1,
    SYSCALL_IPC_SEND_V1, SYSCALL_MAP, SYSCALL_MMIO_MAP, SYSCALL_NSEC, SYSCALL_RECV, SYSCALL_SCHED,
    SYSCALL_SEND, SYSCALL_SPAWN, SYSCALL_SPAWN_LAST_ERROR, SYSCALL_TASK_QOS, SYSCALL_TASK_RESUME,
    SYSCALL_TIMER_CANCEL, SYSCALL_TIMER_CREATE, SYSCALL_TIMER_SET, SYSCALL_VMO_CREATE,
    SYSCALL_VMO_WRITE, SYSCALL_WAIT, SYSCALL_YIELD,
};

/// Execution context shared across syscalls.
pub struct Context<'a> {
    pub scheduler: &'a mut Scheduler,
    pub tasks: &'a mut task::TaskTable,
    pub router: &'a mut ipc::Router,
    pub address_spaces: &'a mut AddressSpaceManager,
    pub timer: &'a dyn Timer,
    pub hart_timers: &'a mut crate::timer::HartTimers,
    pub waitsets: &'a mut crate::waitset::WaitsetTable,
    pub fences: &'a mut crate::fence::FenceTable,
    pub last_message: Option<ipc::Message>,
}

impl<'a> Context<'a> {
    /// Creates a new context for the current task.
    pub fn new(
        scheduler: &'a mut Scheduler,
        tasks: &'a mut task::TaskTable,
        router: &'a mut ipc::Router,
        address_spaces: &'a mut AddressSpaceManager,
        timer: &'a dyn Timer,
        hart_timers: &'a mut crate::timer::HartTimers,
        waitsets: &'a mut crate::waitset::WaitsetTable,
        fences: &'a mut crate::fence::FenceTable,
    ) -> Self {
        Self {
            scheduler,
            tasks,
            router,
            address_spaces,
            timer,
            hart_timers,
            waitsets,
            fences,
            last_message: None,
        }
    }

    /// Returns the last received message header for inspection.
    #[cfg(test)]
    pub fn last_message(&self) -> Option<&ipc::Message> {
        self.last_message.as_ref()
    }
}

#[inline]
fn observe_wake_outcome(outcome: task::WakeOutcome) {
    // P0.2 fail-loud: a wake that silently fails IS the "parked forever"
    // class — the waiter was popped from the endpoint, so nobody will ever
    // retry it. One raw line per failure kind per boot (no UART storm), with
    // the kind named so the log pins the mechanism directly.
    match outcome {
        task::WakeOutcome::Woken
        | task::WakeOutcome::WokenNoopSelftest
        | task::WakeOutcome::TaskNotBlocked => {}
        task::WakeOutcome::TaskNotFound => {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                use core::sync::atomic::{AtomicBool, Ordering};
                static LOGGED: AtomicBool = AtomicBool::new(false);
                if !LOGGED.swap(true, Ordering::Relaxed) {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "KERNEL: FAIL ipc wake (task-not-found)");
                }
            }
        }
        task::WakeOutcome::EnqueueRejected => {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                use core::sync::atomic::{AtomicBool, Ordering};
                static LOGGED: AtomicBool = AtomicBool::new(false);
                if !LOGGED.swap(true, Ordering::Relaxed) {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "KERNEL: FAIL ipc wake (enqueue-rejected)");
                }
            }
        }
    }
}

/// Deadline sweep run at EVERY scheduling transition (`yield`, blocking
/// `ipc_recv_v1/v2`, `waitset_wait`) — i.e. the one body all three of the
/// >10 ms BKL holders observed on 2026-07-25 have in common
/// (`KINIT: long ecall nr=0|18|26|40 10..26ms`, 146 waits >10 ms in an
/// interactive 4-hart boot). It is O(tasks) and each expiry it finds wakes a
/// task, which can IPI another hart — so its cost is attributed separately
/// (`trap::budgets::record_sweep`) and, since 2026-07-25, gated: the scan is
/// skipped outright while `now` is before the earliest armed deadline
/// (`budgets::NEXT_DEADLINE_NS`, which documents why that cannot lose a
/// wakeup). Every scan that does run recomputes the exact minimum of the
/// deadlines still pending, so the gate re-arms itself.
fn wake_expired_blocked(ctx: &mut Context<'_>) {
    let now = ctx.timer.now();
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    if crate::trap::budgets::sweep_can_skip(now) {
        crate::trap::budgets::record_sweep_skipped();
        return;
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    let sweep_t0 = riscv::register::time::read() as u64;
    let len = ctx.tasks.len();
    // Wake-half attribution (see `budgets::SWEEP_MAX_WAKES`): counted only on
    // tasks that actually expired, so a quiet sweep pays nothing for it.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    let (mut wakes, mut wake_ticks) = (0usize, 0u64);
    // Exact minimum of the deadlines that remain pending after this sweep —
    // published at the end so the O(1) gate above is precise, not heuristic.
    let mut next_min = u64::MAX;
    for pid_usize in 0..len {
        let pid = task::Pid::from_raw(pid_usize as u32);
        let Some(t) = ctx.tasks.task(pid) else {
            continue;
        };
        if !t.is_blocked() {
            continue;
        }
        if let Some(d) = t.block_reason().and_then(block_reason_deadline) {
            if d != 0 && d > now && d < next_min {
                next_min = d;
            }
        }
        // Does an arm below actually fire for this task? Computed up front so
        // the wake half can be timed without duplicating every arm's guard.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let expired =
            t.block_reason().and_then(block_reason_deadline).is_some_and(|d| d != 0 && now >= d);
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let arm_t0 = if expired { riscv::register::time::read() as u64 } else { 0 };
        match t.block_reason() {
            Some(BlockReason::IpcRecv { endpoint, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                let _ = ctx.router.remove_recv_waiter(endpoint, pid.as_raw());
                observe_wake_outcome(ctx.tasks.wake(pid, ctx.scheduler));
            }
            Some(BlockReason::IpcSend { endpoint, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                let _ = ctx.router.remove_send_waiter(endpoint, pid.as_raw());
                observe_wake_outcome(ctx.tasks.wake(pid, ctx.scheduler));
            }
            Some(BlockReason::Waitset { ws_id, deadline_ns })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                // Deregister the timed-out waiter from every member, then wake it (it
                // returns `TimedOut` on re-entry). Snapshot members to a stack buffer so
                // the `waitsets` and `router` borrows stay disjoint (no heap, bounded ≤16).
                let mut buf = [0u32; crate::waitset::MAX_WAITSET_MEMBERS];
                let n = ctx
                    .waitsets
                    .members(crate::waitset::WaitsetId(ws_id))
                    .map(|m| {
                        buf[..m.len()].copy_from_slice(m);
                        m.len()
                    })
                    .unwrap_or(0);
                for &ep in &buf[..n] {
                    let _ = ctx.router.remove_recv_waiter(ep, pid.as_raw());
                }
                observe_wake_outcome(ctx.tasks.wake(pid, ctx.scheduler));
            }
            Some(BlockReason::Fence { fence_id, deadline_ns, .. })
                if deadline_ns != 0 && now >= deadline_ns =>
            {
                // Deregister the timed-out waiter, then wake it (returns TimedOut on re-entry).
                ctx.fences.remove_waiter(crate::fence::FenceId(fence_id), pid.as_raw());
                observe_wake_outcome(ctx.tasks.wake(pid, ctx.scheduler));
            }
            _ => {}
        }
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        if expired {
            wakes += 1;
            wake_ticks += (riscv::register::time::read() as u64).saturating_sub(arm_t0);
        }
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        crate::trap::budgets::set_next_deadline(next_min);
        crate::trap::budgets::record_sweep(
            (riscv::register::time::read() as u64).saturating_sub(sweep_t0),
            len,
            wakes,
            wake_ticks,
        );
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = next_min;
}

/// The deadline a block reason carries, if any (0 / none = no deadline).
fn block_reason_deadline(reason: BlockReason) -> Option<u64> {
    match reason {
        BlockReason::IpcRecv { deadline_ns, .. }
        | BlockReason::IpcSend { deadline_ns, .. }
        | BlockReason::Waitset { deadline_ns, .. }
        | BlockReason::Fence { deadline_ns, .. } => Some(deadline_ns),
        _ => None,
    }
}

/// Registers the default set of syscall handlers.
pub fn install_handlers(table: &mut SyscallTable) {
    table.register(SYSCALL_YIELD, sys_yield);
    table.register(SYSCALL_NSEC, sys_nsec);
    table.register(SYSCALL_SEND, sys_send);
    table.register(SYSCALL_RECV, sys_recv);
    table.register(SYSCALL_MAP, sys_map);
    table.register(SYSCALL_MMIO_MAP, sys_mmio_map);
    table.register(SYSCALL_CAP_QUERY, sys_cap_query);
    table.register(SYSCALL_DEVICE_CAP_CREATE, sys_device_cap_create);
    table.register(SYSCALL_VMO_CREATE, sys_vmo_create);
    table.register(SYSCALL_VMO_WRITE, sys_vmo_write);
    table.register(crate::syscall::SYSCALL_VMO_DESTROY, sys_vmo_destroy);
    table.register(crate::syscall::SYSCALL_VMO_SHARE_RO, sys_vmo_share_ro);
    table.register(crate::syscall::SYSCALL_VMO_READ, sys_vmo_read);
    table.register(SYSCALL_SPAWN, sys_spawn);
    table.register(SYSCALL_CAP_TRANSFER, sys_cap_transfer);
    table.register(SYSCALL_CAP_TRANSFER_TO, sys_cap_transfer_to);
    table.register(SYSCALL_AS_CREATE, sys_as_create);
    table.register(SYSCALL_AS_MAP, sys_as_map);
    table.register(SYSCALL_EXIT, sys_exit);
    table.register(SYSCALL_WAIT, sys_wait);
    table.register(crate::syscall::SYSCALL_WAIT_NOHANG, sys_wait_nohang);
    table.register(SYSCALL_EXEC, sys_exec);
    table.register(SYSCALL_IPC_SEND_V1, sys_ipc_send_v1);
    table.register(SYSCALL_EXEC_V2, sys_exec_v2);
    table.register(SYSCALL_IPC_RECV_V1, sys_ipc_recv_v1);
    table.register(SYSCALL_IPC_ENDPOINT_CREATE, sys_ipc_endpoint_create);
    table.register(crate::syscall::SYSCALL_CAP_CLOSE, sys_cap_close);
    table.register(crate::syscall::SYSCALL_CAP_CLONE, sys_cap_clone);
    table.register(crate::syscall::SYSCALL_IPC_ENDPOINT_CLOSE, sys_ipc_endpoint_close);
    table.register(crate::syscall::SYSCALL_IPC_ENDPOINT_CREATE_V2, sys_ipc_endpoint_create_v2);
    table.register(crate::syscall::SYSCALL_IPC_ENDPOINT_CREATE_FOR, sys_ipc_endpoint_create_for);
    table.register(crate::syscall::SYSCALL_GETPID, sys_getpid);
    table.register(SYSCALL_TASK_QOS, sys_task_qos);
    table.register(SYSCALL_SCHED, sys_sched);
    table.register(SYSCALL_AS_SELF, sys_as_self);
    table.register(SYSCALL_TASK_RESUME, sys_task_resume);
    table.register(SYSCALL_TIMER_CREATE, sys_timer_create);
    table.register(SYSCALL_TIMER_SET, sys_timer_set);
    table.register(SYSCALL_TIMER_CANCEL, sys_timer_cancel);
    table.register(crate::syscall::SYSCALL_IRQ_BIND, sys_irq_bind);
    table.register(crate::syscall::SYSCALL_IRQ_COMPLETE, sys_irq_complete);
    table.register(crate::syscall::SYSCALL_WAITSET_CREATE, sys_waitset_create);
    table.register(crate::syscall::SYSCALL_WAITSET_ADD, sys_waitset_add);
    table.register(crate::syscall::SYSCALL_WAITSET_WAIT, sys_waitset_wait);
    table.register(crate::syscall::SYSCALL_FENCE_CREATE, sys_fence_create);
    table.register(crate::syscall::SYSCALL_FENCE_SIGNAL, sys_fence_signal);
    table.register(crate::syscall::SYSCALL_FENCE_WAIT, sys_fence_wait);
    table.register(crate::syscall::SYSCALL_IPC_RECV_V2, sys_ipc_recv_v2);
    table.register(SYSCALL_SPAWN_LAST_ERROR, sys_spawn_last_error);
    table.register(SYSCALL_DEBUG_PUTC, sys_debug_putc);
    table.register(SYSCALL_DEBUG_WRITE, sys_debug_write);
    table.register(SYSCALL_BOOT_MODE, sys_boot_mode);
    table.register(SYSCALL_BOOT_DISPLAY_MODE, sys_boot_display_mode);
    // RFC-0068: fold this per-process syscall-table install echo into the `syscalls` verdict
    // (NEXUS_LOG_EXPAND=syscalls to see them raw). One tally per install event.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    if !crate::log::syscalls_fold() {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("SYSCALL install debug_putc=0x");
        crate::trap::uart_write_hex(&mut u, sys_debug_putc as usize);
        let _ = u.write_str("\n");
        let _ = u.write_str("SYSCALL install ep_create=0x");
        if let Some(addr) = table.debug_handler_addr(SYSCALL_IPC_ENDPOINT_CREATE) {
            crate::trap::uart_write_hex(&mut u, addr);
        } else {
            let _ = u.write_str("none");
        }
        let _ = u.write_str("\n");
    }
}

fn sys_getpid(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.tasks.current_pid().as_index())
}

fn sys_nsec(ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(ctx.timer.now() as usize)
}

fn read_u16_le(bytes: &[u8], off: usize) -> Result<u16, Error> {
    let end = off.checked_add(2).ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Result<u32, Error> {
    let end = off.checked_add(4).ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64_le(bytes: &[u8], off: usize) -> Result<u64, Error> {
    let end = off.checked_add(8).ok_or(AddressSpaceError::InvalidArgs)?;
    let slice = bytes.get(off..end).ok_or(AddressSpaceError::InvalidArgs)?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

/// Minimal debug UART write for userspace: writes one byte `a0` to UART.
/// Returns the byte written on success. This is best-effort and meant only
/// for early bring-up. It does not perform permission checks.
// CRITICAL: Debug only. No permission checks; avoid locks; do not expand scope.
/// P2: the declarative LOCK-FREE syscall class — pure UART/time operations
/// that touch NO kernel state. Served by the trap handler BEFORE acquiring
/// the BKL (they were measured holding it for milliseconds under MTTCG:
/// nr=44 debug_write up to 3ms, nr=1 nsec convoys).
pub(crate) fn lockfree_syscall(nr: usize, _args: &Args) -> Option<SysResult<usize>> {
    match nr {
        // debug_putc/debug_write went BACK to the BKL class: running them
        // lock-free let user UART writes interleave with kernel log lines
        // emitted under the BKL (`SELFTEST:` markers shredded byte-wise on
        // the SMP gate) — the BKL was the de-facto log-channel serializer.
        // Only nsec (writes nothing) stays lock-free.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        SYSCALL_NSEC => {
            // virt mtime runs at 10MHz (budgets::TICKS_PER_US) — ns = ticks*100.
            let ticks = riscv::register::time::read() as u64;
            Some(Ok((ticks * (1_000 / crate::trap::budgets::TICKS_PER_US)) as usize))
        }
        _ => None,
    }
}

fn sys_debug_putc(_ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    let byte = (args.get(0) & 0xff) as u8;
    // Use the raw writer to avoid taking locks under scheduler paths.
    let mut u = crate::uart::raw_writer();
    use core::fmt::Write as _;
    let ch = [byte];
    let s = core::str::from_utf8(&ch).unwrap_or("");
    let _ = u.write_str(s);
    Ok(byte as usize)
}

/// Returns the resolved boot mode for verdict folding: `1` for an interactive boot (services should
/// fold their markers into a `<service> N/N` verdict), `0` for proof/unknown (raw markers, so
/// `verify-uart` stays deterministic). Pure read of the kernel's fw_cfg-derived flag; no args.
fn sys_boot_mode(_ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(usize::from(crate::boot_mode::fold_verdicts()))
}

/// `SYSCALL_BOOT_DISPLAY_MODE` (50): the fw_cfg-configured display mode packed as
/// `w | (h << 16)`, or 0 when unknown/absent (RFC-0074 / ADR-0050). Read-only, no capability.
fn sys_boot_display_mode(_ctx: &mut Context<'_>, _args: &Args) -> SysResult<usize> {
    Ok(crate::boot_mode::display_mode() as usize)
}

/// Atomic debug slice write. Emits the whole user byte slice under the UART lock in one
/// critical section, so a userspace log line cannot interleave mid-line with the kernel's
/// own locked `emit()` or with another process's line. This is the serialized-console fix
/// for the byte-level interleave corruption that `sys_debug_putc`'s lock-free path produced.
/// Args: (ptr, len). Over-long lines are clamped, not rejected.
fn sys_debug_write(_ctx: &mut Context<'_>, args: &Args) -> SysResult<usize> {
    const MAX_DEBUG_WRITE: usize = 1024;
    let ptr = args.get(0);
    let len = core::cmp::min(args.get(1), MAX_DEBUG_WRITE);
    if len == 0 {
        return Ok(0);
    }
    ensure_user_slice(ptr, len)?;
    // SAFETY: `ensure_user_slice` validated [ptr, ptr+len) lies in the caller's user range.
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
    let text = core::str::from_utf8(bytes).unwrap_or("");
    let mut uart = crate::uart::KernelUart::lock();
    use core::fmt::Write as _;
    let _ = (*uart).write_str(text);
    Ok(len)
}
