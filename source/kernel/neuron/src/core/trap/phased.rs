// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Declaratively phased syscalls (P2, LockClass = "expensive middle
//! runs unlocked") — vmo_create zeroes and exec copies ELF bytes with the
//! BKL DROPPED, then re-acquire and finish. Split out of `handler.rs`
//! (structure-gate); frame bookkeeping mirrors `handle_ecall` exactly.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker gates (vmo/exec phases; `bkl budget ok`)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

use super::errno::*;

/// The syscalls whose expensive middle runs with the BKL dropped — the one list both dispatch
/// sites of the trap handler read.
pub(super) fn is_phased(nr: usize) -> bool {
    use crate::syscall::{
        SYSCALL_EXEC_VMO, SYSCALL_VMO_CLEAN, SYSCALL_VMO_CREATE, SYSCALL_VM_UNMAP,
    };
    matches!(nr, SYSCALL_VMO_CREATE | SYSCALL_EXEC_VMO | SYSCALL_VM_UNMAP | SYSCALL_VMO_CLEAN)
}

/// Spin-reacquire the BKL after a phased syscall dropped it (phase B); the later phase's hold
/// counts for the syscall `nr` like its first (`lock_share`).
fn reacquire_kernel(nr: usize) -> super::runtime::KernelGuard {
    loop {
        if let Ok(k) = super::runtime::KernelGuard::acquire_as(nr) {
            break k;
        }
        core::hint::spin_loop();
    }
}

#[allow(dead_code)]
/// P2: declaratively phased syscalls (LockClass = "expensive middle runs
/// unlocked"): vmo_create zeroes and exec copies ELF bytes with the BKL
/// DROPPED. Mirrors handle_ecall's frame bookkeeping (save frame, then write
/// ret + sepc into the CURRENT task's frame) so the common epilogue behaves
/// identically. Safety: reserved ranges are unreachable until the result is
/// visible (vmo cap installs in phase C; exec'd tasks spawn suspended and
/// resume only after this syscall returns), and this hart cannot be
/// preempted (SIE off in trap context), so `current` is stable.
pub(super) fn phased_syscall(
    frame: &mut TrapFrame,
    mut kernel: super::runtime::KernelGuard,
) -> super::runtime::KernelGuard {
    use crate::syscall::Args;
    let nr = frame.x[17];
    let args =
        Args::new([frame.x[10], frame.x[11], frame.x[12], frame.x[13], frame.x[14], frame.x[15]]);
    if nr == crate::syscall::SYSCALL_EXEC_VMO {
        // Phase A (BKL): full exec minus the byte moves (staged into `plan`).
        let mut plan = api::CopyPlan::new();
        let (pid, result) = {
            let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
                kernel.parts();
            let mut ctx = api::Context::new(
                scheduler,
                tasks,
                router,
                spaces,
                timer,
                hart_timers,
                waitsets,
                fences,
            );
            let pid = ctx.tasks.current_pid();
            if let Some(task) = ctx.tasks.task_mut(pid) {
                *task.frame_mut() = *frame;
            }
            record(frame);
            let result = api::exec_vmo_phase_a(&mut ctx, &args, &mut plan);
            (pid, result)
        };
        let value = match result {
            Ok(ret) => {
                // Phase B: move the bytes with the BKL dropped.
                drop(kernel);
                api::run_copy_plan(&plan);
                kernel = reacquire_kernel(nr);
                ret
            }
            Err(err) => encode_error(err),
        };
        {
            let (_, tasks, ..) = kernel.parts();
            if let Some(task) = tasks.task_mut(pid) {
                let f = task.frame_mut();
                f.sepc = f.sepc.wrapping_add(4);
                f.x[10] = value;
            }
        }
        return kernel;
    }
    if nr == crate::syscall::SYSCALL_VM_UNMAP {
        // Phase A (BKL): clear the PTEs; the region stays recorded so its va
        // cannot be reused. Phase B: the TLB shootdown ack wait runs with the
        // BKL DROPPED — a BKL-held wait tripped `bkl budget ok` at SMP≥2.
        let (pid, cleared) = {
            let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
                kernel.parts();
            let mut ctx = api::Context::new(
                scheduler,
                tasks,
                router,
                spaces,
                timer,
                hart_timers,
                waitsets,
                fences,
            );
            let pid = ctx.tasks.current_pid();
            if let Some(task) = ctx.tasks.task_mut(pid) {
                *task.frame_mut() = *frame;
            }
            record(frame);
            let cleared = api::vm_unmap_clear(&mut ctx, &args);
            (pid, cleared)
        };
        let value = match cleared {
            Ok(()) => {
                drop(kernel);
                crate::smp::tlb::shootdown_all();
                kernel = reacquire_kernel(nr);
                let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
                    kernel.parts();
                let mut ctx = api::Context::new(
                    scheduler,
                    tasks,
                    router,
                    spaces,
                    timer,
                    hart_timers,
                    waitsets,
                    fences,
                );
                match api::vm_unmap_finish(&mut ctx, &args) {
                    Ok(ret) => ret,
                    Err(err) => encode_error(err),
                }
            }
            Err(err) => encode_error(err),
        };
        {
            let (_, tasks, ..) = kernel.parts();
            if let Some(task) = tasks.task_mut(pid) {
                let f = task.frame_mut();
                f.sepc = f.sepc.wrapping_add(4);
                f.x[10] = value;
            }
        }
        return kernel;
    }
    if nr == crate::syscall::SYSCALL_VMO_CLEAN {
        // ADR-0092. Phase A (BKL): check the capability and the range, note the job. Phase B:
        // clean with the BKL dropped — `cbo.clean` changes no memory, a frame recycled meanwhile
        // only gets a redundant write-back. Phase C: the result.
        let (pid, planned) = {
            let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
                kernel.parts();
            let mut ctx = api::Context::new(
                scheduler,
                tasks,
                router,
                spaces,
                timer,
                hart_timers,
                waitsets,
                fences,
            );
            let pid = ctx.tasks.current_pid();
            if let Some(task) = ctx.tasks.task_mut(pid) {
                *task.frame_mut() = *frame;
            }
            record(frame);
            (pid, api::vmo_clean_plan(&mut ctx, &args))
        };
        let value = match planned {
            Ok(Some(job)) => {
                drop(kernel);
                crate::mm::vmo::clean::run(&job);
                kernel = reacquire_kernel(nr);
                0
            }
            Ok(None) => 0,
            Err(err) => encode_error(err),
        };
        let (_, tasks, ..) = kernel.parts();
        if let Some(task) = tasks.task_mut(pid) {
            let f = task.frame_mut();
            f.sepc = f.sepc.wrapping_add(4);
            f.x[10] = value;
        }
        return kernel;
    }
    // Phase A (BKL): save the caller frame + reserve the range.
    let (pid, reserved) = {
        let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
            kernel.parts();
        let ctx = api::Context::new(
            scheduler,
            tasks,
            router,
            spaces,
            timer,
            hart_timers,
            waitsets,
            fences,
        );
        let pid = ctx.tasks.current_pid();
        if let Some(task) = ctx.tasks.task_mut(pid) {
            *task.frame_mut() = *frame;
        }
        record(frame);
        (pid, api::vmo_create_reserve(&ctx, &args))
    };
    let write_result = |kernel: &mut super::runtime::KernelGuard, value: usize| {
        let (_, tasks, ..) = kernel.parts();
        if let Some(task) = tasks.task_mut(pid) {
            let f = task.frame_mut();
            f.sepc = f.sepc.wrapping_add(4);
            f.x[10] = value;
        }
    };
    match reserved {
        Err(err) => {
            let errno = encode_error(err);
            write_result(&mut kernel, errno);
            kernel
        }
        Ok((id, len, slot_raw)) => {
            // Phase B: zero with the BKL dropped — other harts' syscalls
            // (the UI hotpath) proceed while we memset. The object is owned
            // by this syscall until phase C installs the cap.
            drop(kernel);
            crate::mm::vmo::zero(id);
            // Phase C: re-acquire, install the cap, write the result.
            let mut kernel = reacquire_kernel(nr);
            let ret = {
                let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
                    kernel.parts();
                let mut ctx = api::Context::new(
                    scheduler,
                    tasks,
                    router,
                    spaces,
                    timer,
                    hart_timers,
                    waitsets,
                    fences,
                );
                match api::vmo_create_finish(&mut ctx, id, len, slot_raw) {
                    Ok(slot) => slot,
                    Err(err) => encode_error(err),
                }
            };
            write_result(&mut kernel, ret);
            kernel
        }
    }
}
