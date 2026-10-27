// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: what a trap entry does to the hart's CURRENT task before anything else runs on
//! its behalf. (1) The orders (TASK-0333 6d, ADR-0082): a `Pending::Kill` or
//! `Pending::Freeze` another hart left while this task was running here, delivered by a
//! resched IPI — checked at the two entries a user task has into the kernel, the syscall
//! path (`handle_ecall`) and the interrupt path (timer tick, IPI), the way a signal is
//! delivered on the return to user mode; the frozen task's trap re-executes on the thaw
//! (its `sepc` is not advanced). (2) The timer-tick preemption of a user task
//! (`preempt_current_user_task`), the switch both share.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU SMP lanes (`KSELFTEST: task freeze ok`, `KSELFTEST: process exit takes
//!   threads ok` with a running worker thread); the preemption by every lane's input chain

use super::*;
use crate::task::Pending;

/// The syscall entry's check, from the syscall context.
pub(crate) fn run_pending_in(
    frame: &mut TrapFrame,
    ctx: &mut crate::syscall::api::Context<'_>,
) -> bool {
    run_pending_for_current(
        frame,
        ctx.tasks,
        ctx.router,
        ctx.scheduler,
        ctx.address_spaces,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    )
}

/// An interrupt entry's check, from the kernel guard (BKL held by the caller).
pub(crate) fn run_pending_guarded(frame: &mut TrapFrame, kernel: &mut KernelGuard) -> bool {
    let (scheduler, tasks, router, spaces, _timer, hart_timers, waitsets, fences) = kernel.parts();
    run_pending_for_current(frame, tasks, router, scheduler, spaces, hart_timers, waitsets, fences)
}

/// Preempts the running user task on a timer tick: re-enqueue it, pick the next
/// runnable task (round-robin within its QoS class), and load that task's saved
/// frame so the trap epilogue resumes it. Resolves cooperative-scheduler starvation
/// — without this, a service that stays runnable via `poll + yield` (e.g. the GPU
/// compositor's reactive pacing under the heavy virgl bring-up) can monopolise the
/// CPU and freeze lower-traffic services such as the input chain.
///
/// No-op (resumes the interrupted task) when it is the only runnable task.
///
/// # Safety contract (caller-enforced)
/// Must be called only from a supervisor interrupt trap with a **user-mode** interrupted
/// context (`sstatus.SPP == 0`) under the BKL. That guarantees `tasks`/`scheduler`/
/// `spaces` are not concurrently borrowed by S-mode kernel code, so the `&mut`s are the
/// unique live borrows. The function itself contains no `unsafe`.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn preempt_current_user_task(
    frame: &mut TrapFrame,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
    spaces: &mut AddressSpaceManager,
) {
    let old_pid = tasks.current_pid();
    // Re-enqueue the running task at the back of its class, then pick the next. If
    // round-robin hands back the same task, nothing else is runnable — resume it
    // with its registers untouched (cheap fast path for a single busy task).
    scheduler.yield_current();
    let Some(next) = scheduler.schedule_next() else {
        return;
    };
    if next == old_pid {
        return;
    }
    // Commit the switch, mirroring the syscall path: persist the interrupted task's
    // live registers, retarget the task table, swap the address space, then load the
    // next task's saved frame for the trap epilogue to restore.
    if let Some(task) = tasks.task_mut(old_pid) {
        *task.frame_mut() = *frame;
    }
    tasks.set_current(next);
    #[cfg(not(feature = "selftest_no_satp"))]
    if let Some(handle) = tasks.task(next).and_then(|t| t.address_space()) {
        // On activation failure the next task cannot be resumed safely; the page-fault
        // path reaps it when it runs. Loading its frame below is still correct.
        let _ = spaces.activate(handle);
    }
    if let Some(task) = tasks.task(next) {
        *frame = *task.frame();
    }
}

/// Executes the current task's pending order, if any, and stages the switch to the next
/// runnable task into `frame`. Returns `true` when the current task was taken off this hart —
/// the caller returns to the trap epilogue without touching the task again.
///
/// `frame` holds the task's live registers (the syscall path saved them already; saving them
/// again is harmless).
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_pending_for_current(
    frame: &mut TrapFrame,
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    scheduler: &mut Scheduler,
    spaces: &mut AddressSpaceManager,
    hart_timers: &mut crate::timer::HartTimers,
    waitsets: &mut crate::waitset::WaitsetTable,
    fences: &mut crate::fence::FenceTable,
) -> bool {
    let cur = tasks.current_pid();
    if cur == crate::types::Pid::KERNEL {
        return false;
    }
    let Some(order) = tasks.take_pending(cur) else {
        return false;
    };
    if let Some(task) = tasks.task_mut(cur) {
        *task.frame_mut() = *frame;
    }
    match order {
        Pending::Kill => {
            crate::syscall::api::exit_current_and_release(
                tasks,
                router,
                scheduler,
                crate::syscall::api::ExitTables {
                    hart_timers,
                    waitsets,
                    fences,
                    address_spaces: spaces,
                },
                -9,
                crate::task::ExitReason::Killed,
            );
            scheduler.purge(cur);
            scheduler.finish_current();
        }
        Pending::Freeze => tasks.freeze_current(scheduler),
    }
    switch_to_next(frame, tasks, router, scheduler, spaces, hart_timers, waitsets, fences);
    true
}

/// Picks the next runnable task for this hart and stages its frame (its space activated); a
/// task whose space cannot be activated is killed and skipped. With nothing runnable the hart
/// re-enters its own scheduler loop.
#[allow(clippy::too_many_arguments)]
fn switch_to_next(
    frame: &mut TrapFrame,
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    scheduler: &mut Scheduler,
    spaces: &mut AddressSpaceManager,
    hart_timers: &mut crate::timer::HartTimers,
    waitsets: &mut crate::waitset::WaitsetTable,
    fences: &mut crate::fence::FenceTable,
) {
    for _ in 0..8 {
        let Some(next) = scheduler.schedule_next() else { break };
        tasks.set_current(next);
        #[cfg(not(feature = "selftest_no_satp"))]
        if let Some(handle) = tasks.task(next).and_then(|t| t.address_space()) {
            if spaces.activate(handle).is_err() {
                crate::syscall::api::exit_current_killed(
                    tasks,
                    router,
                    scheduler,
                    crate::syscall::api::ExitTables {
                        hart_timers,
                        waitsets,
                        fences,
                        address_spaces: spaces,
                    },
                );
                scheduler.purge(next);
                scheduler.finish_current();
                continue;
            }
        }
        if let Some(task) = tasks.task(next) {
            *frame = *task.frame();
            return;
        }
    }
    // Nothing runnable on this hart: re-enter THIS hart's scheduler loop (A3), never PID 0's
    // stale frame (see `fault::kill_faulting_user_task`).
    tasks.set_current(crate::types::Pid::KERNEL);
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    if crate::cpu_main::sched_loop_entered(crate::smp::cpu_current_id()) {
        crate::cpu_main::stage_idle_reentry_frame(frame);
        return;
    }
    if let Some(task) = tasks.task(crate::types::Pid::KERNEL) {
        *frame = *task.frame();
    }
}
