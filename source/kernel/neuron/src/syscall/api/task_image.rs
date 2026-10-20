// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the ONE exit funnel — every way a task dies (`sys_exit`, a trap kill, a
//! fail-fast) ends here, and what the task held goes back here (RFC-0100 §1, TASK-0333 6a:
//! an object dies with its last capability). The funnel returns the process image to the
//! arena (RFC-0075 8e — bump-only images once exhausted it after a handful of launches),
//! closes the endpoints the task OWNED (RFC-0005), frees the timers, waitsets and fences it
//! owned exactly as a `cap_close` of each would, drops its capability table, and judges every
//! endpoint the table named: a receiver whose last peer left is woken to `PeerClosed`
//! (RFC-0079), a sender whose last receiver left likewise, and an endpoint nothing names any
//! more is destroyed. Last, the death itself is an EVENT: the parent's bound endpoint gets one
//! `OP_CHILD_EXITED` frame (the shape of a timer's fire), so the reaper of record (execd,
//! RFC-0081) wakes on it instead of sweeping on its next request. The VMOs the table named
//! are freed at the reap, once the address space is gone (`mm::reclaim`).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `ImageAllocs` host unit tests (image_allocs.rs); `KSELFTEST: task exit
//!   releases ok (…)` and `KSELFTEST: child exit notify ok` each boot; the board's
//!   open/close series (`KINIT: vm reclaim (… by=reap)` after every close).
//! INVARIANTS:
//!   - Called only for a task that is already a Zombie (purged from the
//!     scheduler, never dispatched again), so no hart executes these pages.
//!   - Only USER image/stack ranges are returned — never page tables, never
//!     kernel mappings; a hart idling with a stale `satp` keeps running
//!     kernel text, which is not part of any returned range.

use crate::cap::Rights;
use crate::image_allocs::ImageAllocs;
use crate::sched::Scheduler;
use crate::task::{Pid, TaskTable};

use super::observe_wake_outcome;

/// Returns every recorded block to the frame pool. Reports bytes reclaimed;
/// a refused block is the pool's own event line (never retried — a wrong
/// free would corrupt the pool).
pub fn release_image(allocs: &ImageAllocs) -> usize {
    let mut bytes = 0usize;
    let rejected = 0usize;
    for block in allocs.iter() {
        crate::mm::frame_pool::free(block);
        bytes = bytes.saturating_add(block.size() as usize);
    }
    if rejected != 0 || allocs.untracked() != 0 {
        // Honest accounting: memory we could NOT return stays allocated.
        log_error!(
            "IMAGE-RECLAIM incomplete: rejected={} untracked={} reclaimed=0x{:x}",
            rejected,
            allocs.untracked(),
            bytes
        );
    } else if bytes != 0 {
        // RFC-0068: process teardown is a perpetual runtime event → DEBUG.
        log_debug!(target: "exec", "IMAGE-RECLAIM 0x{:x} bytes ({} ranges)", bytes, allocs.len());
    }
    bytes
}

/// The kernel tables a death touches besides the task table and the router: the objects a
/// task owns by pid (freed like a `cap_close` of each), handed to the funnel by every caller.
pub struct ExitTables<'a> {
    pub hart_timers: &'a mut crate::timer::HartTimers,
    pub waitsets: &'a mut crate::waitset::WaitsetTable,
    pub fences: &'a mut crate::fence::FenceTable,
}

/// The op byte of the frame a child's death writes into its parent's bound endpoint
/// (`SYSCALL_CHILD_EXIT_BIND`). Payload: `pid: u32le, code: i32le, reason: u32le` — the
/// reason word is `ExitReason::wire_bits` (ADR-0056), the same word `wait` returns.
pub const OP_CHILD_EXITED: u8 = 0x31;

/// Exits the current task and releases what it held — the single funnel every exit path
/// uses, so no teardown can forget an object. The caller states WHY the task dies (ADR-0056)
/// so `wait` and the parent's notification carry kernel truth instead of an ambiguous code.
///
/// TASK-0324 P7-b — death wakes: the dying task's SEND caps are dropped with its table, so
/// every endpoint it was the last PEER of is scanned exactly like a `cap_close` would scan
/// it, and a receiver blocked there with EOF opted in is woken to return `PeerClosed`. Before
/// this, `sys_cap_close` ran that scan and exit did not — a client waiting on a reply from a
/// server that died stayed blocked forever, which is why every client carried a timeout.
pub fn exit_current_and_release(
    tasks: &mut TaskTable,
    router: &mut crate::ipc::Router,
    scheduler: &mut Scheduler,
    tables: ExitTables<'_>,
    status: i32,
    reason: crate::task::ExitReason,
) {
    let exiting = tasks.current_pid();
    // Every endpoint the table named, judged after the drop with one census each: the
    // receivers' EOF, the senders' `PeerClosed`, the reclaim.
    let mut named = alloc::vec::Vec::new();
    if let Some(caps) = tasks.caps_of(exiting) {
        caps.endpoint_ids_with(Rights::empty(), &mut named);
    }
    // RFC-0005 lifecycle: the endpoints this task OWNED close with it, their waiters woken
    // (every exit path, not only the voluntary one — a fault kill left them alive before).
    let mut owned = alloc::vec::Vec::new();
    router.owned_alive_ids(exiting.as_raw(), &mut owned);
    let mut woken = router.close_endpoints_for_owner(exiting.as_raw());
    for id in &owned {
        super::ipc_reclaim::forget_dead(tasks, tables.waitsets, tables.hart_timers, *id);
    }
    router.remove_waiter_from_all(exiting.as_raw());
    // The objects it owned by pid: a `cap_close` of each would free them; its death does.
    let timers = tables.hart_timers.free_owned_by(exiting.as_raw());
    let waitsets = tables.waitsets.free_owned_by(exiting.as_raw());
    let fence_waiters = tables.fences.free_owned_by(exiting.as_raw());
    woken.extend(fence_waiters);
    let allocs = tasks.exit_current(status, reason);
    let mut endpoints = 0usize;
    for endpoint in named {
        endpoints += usize::from(super::eof_scan::judge_dropped(
            tasks,
            router,
            scheduler,
            tables.waitsets,
            tables.hart_timers,
            endpoint,
            "exit",
        ));
    }
    for pid in woken {
        observe_wake_outcome(tasks.wake(Pid::from_raw(pid), scheduler));
    }
    if timers != 0 || waitsets != 0 || endpoints != 0 {
        log_debug!(
            target: "task",
            "TASK-RELEASE pid={} timers={} waitsets={} endpoints={}",
            exiting.as_raw(),
            timers,
            waitsets,
            endpoints
        );
    }
    // The parent may be parked in `wait` for exactly this child: every exit path wakes it
    // here, the one funnel (a kernel kill used to repeat this line at each call site).
    tasks.wake_parent_waiter(exiting, scheduler);
    notify_parent(tasks, router, scheduler, exiting, status, reason);
    let _ = release_image(&allocs);
}

/// The death as an event: one `OP_CHILD_EXITED` frame into the endpoint the parent bound
/// (`SYSCALL_CHILD_EXIT_BIND`), a receiver woken like a timer's fire wakes one. Non-blocking:
/// a full queue drops THIS frame, and the parent's sweep (`wait_nohang`) on the next one
/// still reaps every zombie — the frame is a wake, the zombie table is the truth.
fn notify_parent(
    tasks: &mut TaskTable,
    router: &mut crate::ipc::Router,
    scheduler: &mut Scheduler,
    child: Pid,
    status: i32,
    reason: crate::task::ExitReason,
) {
    let Some(endpoint) = tasks.parent_exit_notify(child) else { return };
    let mut payload = [0u8; 13];
    payload[0] = OP_CHILD_EXITED;
    payload[1..5].copy_from_slice(&child.as_raw().to_le_bytes());
    payload[5..9].copy_from_slice(&status.to_le_bytes());
    payload[9..13].copy_from_slice(&reason.wire_bits().to_le_bytes());
    let header = crate::ipc::header::MessageHeader::new(
        0,
        endpoint,
        OP_CHILD_EXITED as u16,
        0,
        payload.len() as u32,
    );
    let msg = crate::ipc::Message::new(header, crate::ipc::Payload::from_slice(&payload), None);
    if router.send(endpoint, msg).is_ok() {
        if let Ok(Some(waiter)) = router.pop_recv_waiter(endpoint) {
            observe_wake_outcome(tasks.wake(Pid::from_raw(waiter), scheduler));
        }
    } else {
        log_debug!(target: "task", "CHILD-EXIT notify dropped pid={}", child.as_raw());
    }
}

/// Trap-handler funnel: the kernel killed the current task on a trap
/// (`ExitReason::Fault`, cause = scause low byte or a synthetic ≥0xF0 code).
pub fn exit_current_faulted(
    tasks: &mut TaskTable,
    router: &mut crate::ipc::Router,
    scheduler: &mut Scheduler,
    tables: ExitTables<'_>,
    cause: u8,
) {
    exit_current_and_release(
        tasks,
        router,
        scheduler,
        tables,
        -22,
        crate::task::ExitReason::Fault { cause },
    );
}

/// `exit_current_killed` from a syscall context (the handler's SATP fail-fast).
pub fn exit_current_killed_in(ctx: &mut super::Context<'_>) {
    exit_current_killed(
        ctx.tasks,
        ctx.router,
        ctx.scheduler,
        ExitTables { hart_timers: ctx.hart_timers, waitsets: ctx.waitsets, fences: ctx.fences },
    );
}

/// Trap-handler funnel: kernel fail-fast termination (`ExitReason::Killed`,
/// e.g. a SATP activation failure that makes the task unresumable).
pub fn exit_current_killed(
    tasks: &mut TaskTable,
    router: &mut crate::ipc::Router,
    scheduler: &mut Scheduler,
    tables: ExitTables<'_>,
) {
    exit_current_and_release(
        tasks,
        router,
        scheduler,
        tables,
        -22,
        crate::task::ExitReason::Killed,
    );
}
