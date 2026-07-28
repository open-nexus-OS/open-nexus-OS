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

/// Spin-reacquire the BKL after a phased syscall dropped it (phase B).
fn reacquire_kernel() -> super::runtime::KernelGuard {
    loop {
        if let Ok(k) = super::runtime::KernelGuard::acquire() {
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
    if nr == crate::syscall::SYSCALL_EXEC || nr == crate::syscall::SYSCALL_EXEC_V2 {
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
            let result = if nr == crate::syscall::SYSCALL_EXEC {
                api::exec_phase_a(&mut ctx, &args, &mut plan)
            } else {
                api::exec_v2_phase_a(&mut ctx, &args, &mut plan)
            };
            (pid, result)
        };
        let value = match result {
            Ok(ret) => {
                // Phase B: move the bytes with the BKL dropped.
                drop(kernel);
                api::run_copy_plan(&plan);
                kernel = reacquire_kernel();
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
                kernel = reacquire_kernel();
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
        (pid, api::vmo_create_reserve(&args))
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
        Ok((base, len, needs_zero, slot_raw)) => {
            // Phase B: zero with the BKL dropped — other harts' syscalls
            // (the UI hotpath) proceed while we memset.
            drop(kernel);
            if needs_zero {
                unsafe {
                    core::ptr::write_bytes(base as *mut u8, 0, len);
                }
            }
            // Phase C: re-acquire, install the cap, write the result.
            let mut kernel = reacquire_kernel();
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
                match api::vmo_create_finish(&mut ctx, base, len, slot_raw) {
                    Ok(slot) => slot,
                    Err(err) => encode_error(err),
                }
            };
            write_result(&mut kernel, ret);
            kernel
        }
    }
}
