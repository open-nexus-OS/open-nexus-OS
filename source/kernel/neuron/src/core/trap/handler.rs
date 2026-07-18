// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Trap dispatch split out of the former single-file trap.rs:
//! __trap_rust (interrupt dispatch S_SOFT/S_TIMER/S_EXT + exception/page-fault
//! paths), handle_ecall syscall dispatch, -errno encoding and the timer-tick
//! user preemption path. Safety invariants (BKL scope, sret discipline,
//! lock-free S_SOFT ack) are documented inline and moved verbatim.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

// ——— syscall path (unchanged API) ———

#[allow(dead_code)]
/// P2: declaratively phased syscalls (LockClass = "expensive middle runs
/// unlocked"): vmo_create zeroes and exec copies ELF bytes with the BKL
/// DROPPED. Mirrors handle_ecall's frame bookkeeping (save frame, then write
/// ret + sepc into the CURRENT task's frame) so the common epilogue behaves
/// identically. Safety: reserved ranges are unreachable until the result is
/// visible (vmo cap installs in phase C; exec'd tasks spawn suspended and
/// resume only after this syscall returns), and this hart cannot be
/// preempted (SIE off in trap context), so `current` is stable.
fn phased_syscall(
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
                kernel = loop {
                    if let Ok(k) = super::runtime::KernelGuard::acquire() {
                        break k;
                    }
                    core::hint::spin_loop();
                };
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
            let mut kernel = loop {
                if let Ok(k) = super::runtime::KernelGuard::acquire() {
                    break k;
                }
                core::hint::spin_loop();
            };
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

pub fn handle_ecall(frame: &mut TrapFrame, table: &SyscallTable, ctx: &mut api::Context<'_>) {
    // Save current frame into the current task before handling the syscall.
    let old_pid = ctx.tasks.current_pid();
    // a7 = syscall number; a0..a5 = args
    let number = frame.x[17]; // a7
    let log_syscall = false;
    if log_syscall {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL start old=0x");
            uart_write_hex(&mut u, old_pid as usize);
            let _ = u.write_str("\n");
        });
    }
    if let Some(task) = ctx.tasks.task_mut(old_pid) {
        if log_syscall {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("HECALL save frame pid=0x");
                uart_write_hex(&mut u, old_pid as usize);
                let _ = u.write_str("\n");
            });
        }
        *task.frame_mut() = *frame;
    } else if log_syscall {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL missing task pid=0x");
            uart_write_hex(&mut u, old_pid as usize);
            let _ = u.write_str("\n");
        });
    }
    record(frame);
    let args =
        Args::new([frame.x[10], frame.x[11], frame.x[12], frame.x[13], frame.x[14], frame.x[15]]);
    // NOTE: keep trap-side UART logging minimal; use trap_ring/trap_symbols for post-mortem triage.
    if log_syscall {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL dispatch num=0x");
            uart_write_hex(&mut u, number);
            let _ = u.write_str("\n");
        });
    }
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL table ptr=0x");
        uart_write_hex(&mut u, table as *const SyscallTable as usize);
        let _ = u.write_str(" handler=0x");
        if let Some(addr) = table.debug_handler_addr(number) {
            uart_write_hex(&mut u, addr);
        } else {
            let _ = u.write_str("none");
        }
        let _ = u.write_str("\n");
    });
    let mut maybe_ret = None;
    match table.dispatch(number, ctx, &args) {
        Ok(ret) => maybe_ret = Some(ret),
        Err(SysError::TaskExit) => {}
        // Reschedule means: do not advance SEPC and do not write a return value. Trap-exit will
        // pick up the next task (SATP switch happens there); when this task runs again it will
        // retry the same syscall instruction.
        Err(SysError::Reschedule) => {}
        Err(err) => {
            let _errno_val = encode_error(err);
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("HECALL dispatch err num=0x");
                uart_write_hex(&mut u, number);
                let _ = u.write_str(" sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str(" err=");
                uart_write_hex(&mut u, _errno_val);
                let _ = u.write_str("\n");
            });
            // ABI: return -errno in a0; do not terminate the calling task for expected syscall errors.
            // (See RFC-0005 and docs/architecture/01-neuron-kernel.md.)
            maybe_ret = Some(_errno_val);
        }
    }

    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL dispatch done maybe=");
        match maybe_ret {
            Some(ret) => uart_write_hex(&mut u, ret),
            None => {
                let _ = u.write_str("none");
            }
        }
        let _ = u.write_str("\n");
    });
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("handle_ecall before advance sepc=0x");
        uart_write_hex(&mut u, frame.sepc);
        let _ = u.write_str("\n");
    });
    // Advance caller PC and store return in its saved frame (a0).
    if let Some(ret) = maybe_ret {
        if let Some(task) = ctx.tasks.task_mut(old_pid) {
            let f = task.frame_mut();
            uart_dbg_block!({
                ecall_log(|u| {
                    use core::fmt::Write as _;
                    let _ = write!(
                        u,
                        "ECALL pre-advance pid=0x{:x} sepc=0x{:x}\n",
                        old_pid as usize, f.sepc
                    );
                });
            });
            f.sepc = f.sepc.wrapping_add(4);
            f.x[10] = ret;
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("HECALL ret store pid=0x");
                uart_write_hex(&mut u, old_pid as usize);
                let _ = u.write_str(" sepc=0x");
                uart_write_hex(&mut u, f.sepc);
                let _ = u.write_str(" a0=0x");
                uart_write_hex(&mut u, ret);
                let _ = u.write_str("\n");
            });
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("task.frame_mut after sepc=0x");
                uart_write_hex(&mut u, f.sepc);
                let _ = u.write_str("\n");
            });
            uart_dbg_block!({
                ecall_log(|u| {
                    use core::fmt::Write as _;
                    let _ = write!(
                        u,
                        "ECALL post-advance pid=0x{:x} sepc=0x{:x}\n",
                        old_pid as usize, f.sepc
                    );
                });
            });
        }
    }
    // Load the next task's frame into the live trap frame.
    let new_pid = ctx.tasks.current_pid();
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL load next pid=0x");
        uart_write_hex(&mut u, new_pid as usize);
        let _ = u.write_str("\n");
    });
    if let Some(task) = ctx.tasks.task_mut(new_pid) {
        *frame = *task.frame();
        // If the syscall path switched `current_pid` (e.g. SYSCALL_YIELD), ensure we
        // also switch SATP + SSCRATCH before returning to U-mode. Do NOT do this for
        // non-switching syscalls (debug_putc etc) or we will spam the UART and slow
        // boot to a crawl.
        if new_pid != old_pid {
            #[cfg(not(feature = "selftest_no_satp"))]
            if let Some(handle) = task.address_space() {
                if ctx.address_spaces.activate(handle).is_err() {
                    // Fail-fast: returning with a mismatched SATP is unsafe.
                    ctx.tasks.exit_current(-22);
                }
            }
        }
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL frame updated sepc=0x");
            uart_write_hex(&mut u, frame.sepc);
            let _ = u.write_str("\n");
        });
        uart_dbg_block!({
            ecall_log(|u| {
                use core::fmt::Write as _;
                let _ =
                    write!(u, "ECALL load pid=0x{:x} sepc=0x{:x}\n", new_pid as usize, frame.sepc);
            });
        });
    }
}

const EPERM: usize = 1;
const ENOMEM: usize = 12;
const EAGAIN: usize = 11;
const EINVAL: usize = 22;
const ENOSPC: usize = 28;
const ENOSYS: usize = 38;
const ESRCH: usize = 3;
const ECHILD: usize = 10;
const ETIMEDOUT: usize = 110;

#[allow(dead_code)]
fn encode_error(err: SysError) -> usize {
    match err {
        SysError::InvalidSyscall => errno(ENOSYS),
        SysError::Capability(cap) => match cap {
            crate::cap::CapError::NoSpace => errno(ENOSPC),
            _ => errno(EPERM),
        },
        SysError::Ipc(ipc_err) => ipc_errno(&ipc_err),
        SysError::Spawn(spawn) => spawn_errno(&spawn),
        SysError::Transfer(_) => errno(EPERM),
        SysError::AddressSpace(as_err) => address_space_errno(&as_err),
        SysError::Wait(wait) => wait_errno(&wait),
        SysError::TaskExit => errno(EINVAL),
        SysError::Reschedule => errno(EAGAIN),
        SysError::InvalidTarget => errno(ESRCH),
        SysError::RunQueueFull => errno(ENOSPC),
    }
}

#[allow(dead_code)]
fn ipc_errno(err: &crate::ipc::IpcError) -> usize {
    match err {
        crate::ipc::IpcError::NoSuchEndpoint => errno(ESRCH),
        crate::ipc::IpcError::QueueFull | crate::ipc::IpcError::QueueEmpty => errno(EAGAIN),
        crate::ipc::IpcError::PermissionDenied => errno(EPERM),
        crate::ipc::IpcError::TimedOut => errno(ETIMEDOUT),
        crate::ipc::IpcError::NoSpace => errno(ENOSPC),
    }
}

#[allow(dead_code)]
fn spawn_errno(err: &task::SpawnError) -> usize {
    use task::SpawnError::*;
    match err {
        InvalidParent | InvalidEntryPoint | InvalidStackPointer => errno(EINVAL),
        BootstrapNotEndpoint => errno(EPERM),
        Capability(_) => errno(EPERM),
        Ipc(_) => errno(EINVAL),
        AddressSpace(as_err) => address_space_errno(as_err),
        StackExhausted => errno(ENOMEM),
        RunQueueFull => errno(EAGAIN),
    }
}

#[allow(dead_code)]
fn address_space_errno(err: &AddressSpaceError) -> usize {
    match err {
        AddressSpaceError::InvalidHandle | AddressSpaceError::InvalidArgs => errno(EINVAL),
        AddressSpaceError::AsidExhausted => errno(ENOSPC),
        AddressSpaceError::InUse => errno(EPERM),
        AddressSpaceError::Unsupported => errno(ENOSYS),
        AddressSpaceError::Mapping(MapError::PermissionDenied) => errno(EPERM),
        AddressSpaceError::Mapping(_) => errno(EINVAL),
    }
}

#[allow(dead_code)]
fn wait_errno(err: &task::WaitError) -> usize {
    use task::WaitError::*;
    match err {
        NoChildren => errno(ECHILD),
        NoSuchPid => errno(ESRCH),
        InvalidTarget => errno(EINVAL),
        WouldBlock => errno(EINVAL),
    }
}

const fn errno(code: usize) -> usize {
    (-(code as isize)) as usize
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
/// Must be called only from the supervisor timer trap with a **user-mode** interrupted
/// context (`sstatus.SPP == 0`) on the boot hart. That guarantees `tasks`/`scheduler`/
/// `spaces` are not concurrently borrowed by S-mode kernel code, so the `&mut`s are the
/// unique live borrows. The function itself contains no `unsafe`.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn preempt_current_user_task(
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

// ——— Rust trap handler called from assembly ———

#[no_mangle]
extern "C" fn __trap_rust(frame: &mut TrapFrame) {
    // CRITICAL: Trap entry marker with cause (safe UART, no heap)
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("TRAP[");
        uart_write_hex(&mut u, frame.scause);
        let _ = u.write_str("] sepc=0x");
        uart_write_hex(&mut u, frame.sepc);
        let _ = u.write_str("\n");
        core::mem::drop(u);
    });

    // Liveness heartbeat on every trap entry
    crate::liveness::bump();
    if is_interrupt(frame.scause) {
        const S_SOFT_INT: usize = 1;
        // Supervisor timer: rearm via SBI and return.
        const S_TIMER_INT: usize = 5;
        let code = frame.scause & (usize::MAX >> 1);
        if code == S_SOFT_INT {
            let cpu = crate::smp::cpu_current_id();
            // A5: TLB shootdown responder FIRST (lock-free — the initiator
            // may hold the BKL while waiting for this ack), then resched.
            let _ = crate::smp::tlb::poll_mailbox(cpu);
            let outcome = crate::smp::handle_ssoft_resched(cpu);
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            unsafe {
                riscv::register::sip::clear_ssoft();
            }
            // A4: an acked resched request PREEMPTS the interrupted user task
            // so a cross-core-woken task runs within IPI latency, not a full
            // timer period. Interrupted S-mode (WFI idle / cpu_main) needs
            // nothing: the loop rescans its queue right after the trap.
            const SSTATUS_SPP: usize = 1 << 8;
            if matches!(outcome, crate::smp::ReschedTrapOutcome::Acked)
                && frame.sstatus & SSTATUS_SPP == 0
            {
                if let Ok(mut kernel) = KernelGuard::acquire() {
                    let (scheduler, tasks, _router, spaces, _timer, _ht, _ws, _fences) =
                        kernel.parts();
                    preempt_current_user_task(frame, tasks, scheduler, spaces);
                }
            }
            return;
        }
        if code == S_TIMER_INT {
            // A7: per-hart tick bookkeeping; the FIRST tick on a secondary
            // hart is the per-hart-timer liveness proof (event-anchored).
            {
                let cpu = crate::smp::cpu_current_id();
                let prev = crate::smp::record_timer_tick(cpu);
                if prev == 0 && !cpu.is_boot() {
                    log_info!(target: "smp", "KSELFTEST: smp per-hart ticks ok");
                }
                // TASK-0288: event-anchored tick-budget proof on the first
                // secondary hart — over the first 8 ticks the tick rate must
                // stay bounded (never faster than 1/ms). Emitted from the
                // experiencing hart itself (a boot-hart poll would race the
                // secondaries' park/release window).
                if !cpu.is_boot() {
                    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
                    static WINDOW_START: AtomicU64 = AtomicU64::new(0);
                    static BUDGET_EMITTED: AtomicBool = AtomicBool::new(false);
                    const BUDGET_TICKS: usize = 8;
                    if prev == 0 {
                        let _ = WINDOW_START.compare_exchange(
                            0,
                            riscv::register::time::read() as u64,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                    } else if prev + 1 == BUDGET_TICKS
                        && !BUDGET_EMITTED.swap(true, Ordering::AcqRel)
                    {
                        let start = WINDOW_START.load(Ordering::Acquire);
                        let window_ms =
                            ((riscv::register::time::read() as u64).saturating_sub(start) / 10_000)
                                .max(1);
                        if (BUDGET_TICKS as u64) <= window_ms {
                            log_info!(target: "smp", "KSELFTEST: runtime timer budget ok");
                        } else {
                            log_error!(
                                target: "smp",
                                "KSELFTEST: runtime timer budget FAIL ticks={} window_ms={}",
                                BUDGET_TICKS,
                                window_ms
                            );
                        }
                    }
                }
            }
            // Supervisor timer tick. Preemption + scheduler/task access is only safe
            // when we interrupted a USER-mode task (sstatus.SPP == 0): such a context
            // holds no kernel borrows, so mutating the scheduler/task table here cannot
            // alias S-mode kernel code (timer IRQs are hardware-masked while SIE is
            // clear inside any S-mode trap handler). When we interrupt S-mode (kernel
            // idle/boot), we only re-arm the heartbeat and resume untouched.
            const SSTATUS_SPP: usize = 1 << 8;
            let interrupted_user = frame.sstatus & SSTATUS_SPP == 0;
            if interrupted_user {
                if let Ok(mut kernel) = KernelGuard::acquire() {
                    // BKL held for the whole delivery+preempt sequence (A2a);
                    // released on scope exit, before the asm epilogue runs.
                    let (scheduler, tasks, router, spaces, timer, hart_timers, _waitsets, _fences) =
                        kernel.parts();
                    // Reactive: deliver fired timer caps + re-arm the next deadline.
                    process_expired_timers(timer, hart_timers, router, tasks, scheduler);
                    // Backstop for device IRQs: the S_EXT handler delivers them
                    // immediately while a task runs, but if one asserted during an
                    // S-mode window (or while every task was blocked) the timer tick
                    // drains and delivers it here within one period. No-op when
                    // nothing is pending (claim() returns None).
                    crate::irq::dispatch_external(router, tasks, scheduler);
                    // Reactive IPC deadlines: wake any task whose timed recv/send
                    // (set_wakeup-armed) has elapsed — windowd's pacer, gpud's spin
                    // re-present. Makes the timer IRQ the single wake source for ALL
                    // deadlines (caps + IPC), like a production microkernel.
                    wake_expired_ipc_deadlines(timer, router, tasks, scheduler);
                    // Preemptive: rotate the running user task so no service can
                    // monopolise the cooperative scheduler (anti-starvation).
                    // B (TASK-0042): shares grant whole extra ticks within the
                    // QoS class (shares/100, clamped [1,10]); default 100 = one
                    // tick = pre-B behavior.
                    let slice_ticks = tasks
                        .task(tasks.current_pid())
                        .map(|t| ((t.shares() / 100) as usize).clamp(1, 10))
                        .unwrap_or(1);
                    let cpu = crate::smp::cpu_current_id();
                    if crate::smp::preempt_tick_and_rotate(cpu, slice_ticks) {
                        preempt_current_user_task(frame, tasks, scheduler, spaces);
                    }
                    return;
                }
            }
            // S-mode interrupt (kernel idle/boot) or runtime not yet installed:
            // re-arm the heartbeat and resume the interrupted context untouched.
            #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
            {
                let next = riscv::register::time::read() as u64 + DEFAULT_TICK_CYCLES;
                sbi::set_timer(next);
            }
        }
        const S_EXT_INT: usize = 9;
        if code == S_EXT_INT {
            // External device interrupt via the PLIC. Deliver to bound userspace
            // drivers (waking a blocked driver) only when we interrupted U-mode —
            // the same exclusivity guarantee as the timer path (no S-mode kernel
            // code is concurrently borrowing the router/tasks/scheduler).
            const SSTATUS_SPP: usize = 1 << 8;
            if frame.sstatus & SSTATUS_SPP == 0 {
                if let Ok(mut kernel) = KernelGuard::acquire() {
                    let (scheduler, tasks, router, _spaces, _timer, _ht, _ws, _fences) =
                        kernel.parts();
                    crate::irq::dispatch_external(router, tasks, scheduler);
                    return;
                }
            }
            // S-mode interrupt or runtime not yet installed: drain without delivery
            // so a stray source cannot storm (bound sources stay enabled for the
            // next U-mode trap).
            crate::irq::drain_undelivered();
        }
        return;
    }

    // Exception path (print limited diagnostics only for exceptions)
    const ILLEGAL_INSTRUCTION: usize = 2;
    const ECALL_UMODE: usize = 8;
    const ECALL_SMODE: usize = 9;
    const LOAD_PAGE_FAULT: usize = 13;
    const STORE_PAGE_FAULT: usize = 15;
    const INST_PAGE_FAULT: usize = 12;
    let exc = frame.scause & (usize::MAX >> 1);
    // Quiet exception banner during bring-up to avoid fmt/alloc paths
    if exc == ECALL_UMODE || exc == ECALL_SMODE {
        // Debug: Log FIRST ecall only using safe UART (no heap allocation)
        static ECALL_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        // Guard against true ECALL storms (same sepc repeating), but do not penalize
        // normal syscall-heavy workloads (e.g. init printing boot markers).
        static LAST_ECALL_SEPC: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        static SAME_ECALL_SEPC_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        let count = ECALL_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            // CRITICAL: Minimal logging in separate scope, explicit drop before proceeding
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("ECALL #0 sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str("\n");
                core::mem::drop(u);
            });
        }

        // Prevent endless ECALL storms: abort only if we observe a large number of
        // ECALLs from the exact same sepc (no forward progress).
        let last = LAST_ECALL_SEPC.load(core::sync::atomic::Ordering::Relaxed);
        let same = if last == frame.sepc {
            SAME_ECALL_SEPC_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1
        } else {
            LAST_ECALL_SEPC.store(frame.sepc, core::sync::atomic::Ordering::Relaxed);
            SAME_ECALL_SEPC_COUNT.store(0, core::sync::atomic::Ordering::Relaxed);
            0
        };
        if same > 10_000 {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("ECALL-STORM throttle sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str(" ra=0x");
                uart_write_hex(&mut u, frame.x[1]);
                let _ = u.write_str("\n");
            });
            // Throttle/quarantine behavior (no kill):
            // - Return a deterministic error to userspace
            // - Advance past the ECALL to avoid an infinite re-trap at the same sepc
            // This preserves the ABI model (syscalls return -errno) and prevents CPU-burning storms.
            frame.x[10] = errno(EINVAL);
            frame.sepc = frame.sepc.wrapping_add(4);
            return;
        }

        // P2 lock-free class: pure UART/time syscalls never touch the BKL —
        // served here and returned directly (no task switch is possible, so
        // writing the live frame is correct, mirroring the ENOSYS fallback).
        {
            use crate::syscall::Args;
            let args = Args::new([
                frame.x[10],
                frame.x[11],
                frame.x[12],
                frame.x[13],
                frame.x[14],
                frame.x[15],
            ]);
            if let Some(result) = api::lockfree_syscall(frame.x[17], &args) {
                frame.x[10] = match result {
                    Ok(v) => v,
                    Err(err) => encode_error(err),
                };
                frame.sepc = frame.sepc.wrapping_add(4);
                record(frame);
                return;
            }
        }

        // Acquire the BKL for the full syscall dispatch (A2a).
        let mut kernel = match KernelGuard::acquire() {
            Ok(kernel) => kernel,
            Err(reason) => {
                // RFC-0003: keep logs unified and deterministic; avoid ad-hoc debug spam.
                // Use raw UART to avoid mutex recursion in trap context, but keep the same
                // `[LEVEL target]` prefix as the centralized logger.
                static WARN_COUNT: core::sync::atomic::AtomicUsize =
                    core::sync::atomic::AtomicUsize::new(0);
                if WARN_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 4 {
                    let mut u = crate::uart::raw_writer();
                    let msg = match reason {
                        RuntimeKernelAccessFailure::NotInstalled => {
                            "[WARN trap] trap runtime not installed\n"
                        }
                    };
                    let _ = u.write_str(msg);
                }
                frame.x[10] = errno(ENOSYS);
                frame.sepc = frame.sepc.wrapping_add(4);
                return;
            }
        };

        // P2: declaratively phased syscall — SYSCALL_VMO_CREATE's zeroing
        // runs with the BKL DROPPED (phase B). Runs BEFORE parts(): the
        // guard is moved in and the RE-ACQUIRED one comes back, so the
        // epilogue below borrows from a guard that owns the BKL again. Safe:
        // the reserved range is unreachable until phase C installs the cap,
        // and this hart cannot be preempted (SIE off in trap context), so
        // `current` is stable across the phases.
        if matches!(
            frame.x[17],
            crate::syscall::SYSCALL_VMO_CREATE
                | crate::syscall::SYSCALL_EXEC
                | crate::syscall::SYSCALL_EXEC_V2
        ) {
            kernel = phased_syscall(frame, kernel);
        }

        static LOGGED_ENV_PTRS: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !LOGGED_ENV_PTRS.swap(true, core::sync::atomic::Ordering::SeqCst) {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("TRAPENV sched=0x");
                uart_write_hex(&mut u, kernel.handles.scheduler.as_ptr() as usize);
                let _ = u.write_str(" tasks=0x");
                uart_write_hex(&mut u, kernel.handles.tasks.as_ptr() as usize);
                let _ = u.write_str(" router=0x");
                uart_write_hex(&mut u, kernel.handles.router.as_ptr() as usize);
                let _ = u.write_str(" spaces=0x");
                uart_write_hex(&mut u, kernel.handles.spaces.as_ptr() as usize);
                let _ = u.write_str("\n");
            });
        }

        let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
            kernel.parts();

        // User-mode sanity: verify sepc is mapped in current AS and looks executable.
        // This is diagnostic-only and protects against jumping into rodata.
        const SSTATUS_SPP: usize = 1 << 8;
        let from_user = (frame.sstatus & SSTATUS_SPP) == 0;
        if from_user {
            let pid = tasks.current_pid();
            if let Some(task) = tasks.task(pid) {
                if let Some(as_handle) = task.address_space() {
                    if let Ok(space) = spaces.get(as_handle) {
                        let pt = space.page_table();
                        let maybe_sepc = pt.translate(frame.sepc);
                        if let Some(_pa) = maybe_sepc {
                            uart_dbg_block!({
                                let mut u = crate::uart::raw_writer();
                                let _ = u.write_str("ECALL-BOUNDS sepc ok pa=0x");
                                uart_write_hex(&mut u, _pa);
                                let _ = u.write_str("\n");
                            });
                        } else {
                            uart_dbg_block!({
                                let mut u = crate::uart::raw_writer();
                                let _ = u.write_str("ECALL-BOUNDS unmapped sepc=0x");
                                uart_write_hex(&mut u, frame.sepc);
                                let _ = u.write_str(" ra=0x");
                                uart_write_hex(&mut u, frame.x[1]);
                                let _ = u.write_str(" sp=0x");
                                uart_write_hex(&mut u, frame.x[2]);
                                let _ = u.write_str("\n");
                            });
                            frame.x[10] = errno(EINVAL);
                            tasks.exit_current(-22);
                            return;
                        }
                    }
                }
            }
        }

        let current_pid = tasks.current_pid();
        let domain_id = tasks
            .task(current_pid)
            .map(|task| task.trap_domain())
            .unwrap_or_else(runtime_default_domain);
        let syscalls_ptr = runtime_domain(domain_id)
            .or_else(|| runtime_domain(runtime_default_domain()))
            .expect("trap domain not available");
        let table = unsafe { syscalls_ptr.as_ref() };

        #[allow(unused_variables)]
        let old_pid = tasks.current_pid();
        // P2: declaratively phased syscall — SYSCALL_VMO_CREATE's zeroing runs
        // with the BKL DROPPED. Safe: the reserved range is unreachable until
        // phase C installs the capability, and this hart cannot be preempted
        // (SIE off in trap context) so `current` stays stable across phases.
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
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let ecall_t0 = riscv::register::time::read() as u64;
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let ecall_nr = frame.x[17];
        if !matches!(
            frame.x[17],
            crate::syscall::SYSCALL_VMO_CREATE
                | crate::syscall::SYSCALL_EXEC
                | crate::syscall::SYSCALL_EXEC_V2
        ) {
            handle_ecall(frame, table, &mut ctx);
        }
        // Bounded probe: syscalls holding the BKL >10ms are the source of the
        // cross-hart contention stalls (A3 diagnosis).
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let held = (riscv::register::time::read() as u64).saturating_sub(ecall_t0);
            super::budgets::record_ecall_hold(held, ecall_nr as u64);
            if held > 100_000 {
                static LONG_ECALL_LOGGED: core::sync::atomic::AtomicUsize =
                    core::sync::atomic::AtomicUsize::new(0);
                if LONG_ECALL_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 8 {
                    log_info!(
                        target: "smp",
                        "KINIT: long ecall nr={} {}ms cpu{}",
                        ecall_nr,
                        held / 10_000,
                        crate::smp::cpu_current_id().as_index()
                    );
                }
            }
        }

        let current_pid = ctx.tasks.current_pid();
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("CTX PID old=0x");
            uart_write_hex(&mut u, old_pid as usize);
            let _ = u.write_str(" new=0x");
            uart_write_hex(&mut u, current_pid as usize);
            let _ = u.write_str("\n");
        });
        // A3: once the runtime is released, a hart whose syscall left no valid
        // USER task to resume must sret back into ITS OWN cpu_main — the
        // legacy fallback (PID 0's stale frame = the boot hart's S-mode
        // context) is catastrophically wrong on a secondary hart.
        let valid_user_target = match ctx.tasks.task(current_pid) {
            None => false,
            Some(task) => {
                const SSTATUS_SPP: usize = 1 << 8;
                const KERNEL_BASE: usize = 0x8000_0000;
                let tf = task.frame();
                task.address_space().is_some()
                    && tf.sstatus & SSTATUS_SPP == 0
                    && tf.sepc < KERNEL_BASE
            }
        };
        if !valid_user_target && crate::smp::runtime_ready() {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                crate::cpu_main::stage_idle_reentry_frame(frame);
                return;
            }
        }
        if let Some(task) = ctx.tasks.task(current_pid) {
            let tf = task.frame();
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("TASK FRAME sepc=0x");
                uart_write_hex(&mut u, tf.sepc);
                let _ = u.write_str("\n");
            });
            frame.x.copy_from_slice(&tf.x);
            frame.sepc = tf.sepc;
            frame.sstatus = tf.sstatus;
            frame.scause = tf.scause;
            frame.stval = tf.stval;
        } else {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("TASK FRAME missing for pid=0x");
                uart_write_hex(&mut u, current_pid as usize);
                let _ = u.write_str("\n");
            });
        }
        return;
    }
    if exc == ILLEGAL_INSTRUCTION {
        // Decode from stval (avoid touching faulting PC); emulate only whitelisted CSR reads.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let inst = riscv::register::stval::read() as u32;
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        let inst: u32 = 0;
        if is_rdcycle_or_time(inst) || is_rdinstret(inst) {
            let rd = ((inst >> 7) & 0x1f) as usize;
            if rd != 0 {
                frame.set_x(rd, 0);
            }
            record(frame);
            frame.sepc = frame.sepc.wrapping_add(4);
            return;
        }
        // Emit precise diagnostics: sepc, ra, stval, and instruction bytes at sepc.
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            let stval_now = riscv::register::stval::read();
            #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
            let stval_now: usize = 0;
            let _ = writeln!(
                u,
                "ILLEGAL-D: sepc=0x{:x} ra=0x{:x} stval=0x{:x}",
                frame.sepc, frame.x[1], stval_now
            );
            // Best-effort fetch of instruction bytes at sepc
            let i16 = unsafe { core::ptr::read_volatile(frame.sepc as *const u16) } as u16;
            let i32 = unsafe { core::ptr::read_volatile(frame.sepc as *const u32) } as u32;
            let _ = writeln!(u, "ILLEGAL-D: inst16=0x{:04x} inst32=0x{:08x}", i16, i32);
            // Extra: dump PTE flags for sepc page if available (debug aid)
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                // Walk the active SATP to locate the PTE for the faulting page and dump flags.
                fn vpn_indices_sv39(va: usize) -> [usize; 3] {
                    let vpn0 = (va >> 12) & 0x1ff;
                    let vpn1 = (va >> 21) & 0x1ff;
                    let vpn2 = (va >> 30) & 0x1ff;
                    [vpn2, vpn1, vpn0] // hardware order: L2->L1->L0
                }
                let satp_now = riscv::register::satp::read().bits();
                let ppn = satp_now & ((1 << 44) - 1);
                if ppn == 0 {
                    let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
                    let _ = writeln!(
                        u,
                        "ILLEGAL-D: satp=0x{:x} page=0x{:x} (ppn=0)",
                        satp_now, page_va
                    );
                } else {
                    let mut table = (ppn << 12) as *const usize;
                    let indices = vpn_indices_sv39(frame.sepc);
                    let mut pte: usize = 0;
                    let mut found = true;
                    for (level, idx) in indices.iter().enumerate() {
                        let entry_ptr = unsafe { table.add(*idx) };
                        let entry = unsafe { core::ptr::read_volatile(entry_ptr) };
                        if entry & 1 == 0 {
                            found = false;
                            break;
                        }
                        let is_leaf = (entry & ((1 << 1) | (1 << 2) | (1 << 3))) != 0; // any of R/W/X
                        if level == 2 {
                            if !is_leaf {
                                found = false;
                                break;
                            }
                            pte = entry;
                            break;
                        }
                        if is_leaf {
                            found = false;
                            break;
                        }
                        let next_ppn = (entry >> 10) & ((1 << 44) - 1);
                        table = (next_ppn << 12) as *const usize;
                    }
                    if found {
                        let flags = pte & 0x3ff;
                        let _ = writeln!(
                            u,
                            "ILLEGAL-D: satp=0x{:x} pte=0x{:x} flags=0x{:x}",
                            satp_now, pte, flags
                        );
                    } else {
                        let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
                        let _ = writeln!(
                            u,
                            "ILLEGAL-D: satp=0x{:x} page=0x{:x} (unmapped or non-leaf)",
                            satp_now, page_va
                        );
                    }
                }
            }
        }
        record(frame);
        // Avoid formatted panic to prevent allocator/formatting faults during bring-up
        panic!("ILLEGAL");
    }

    // Handle page faults (common for user processes)
    if exc == LOAD_PAGE_FAULT || exc == STORE_PAGE_FAULT || exc == INST_PAGE_FAULT {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let stval_now = riscv::register::stval::read();
            const SSTATUS_SPP: usize = 1 << 8;
            let from_user = (frame.sstatus & SSTATUS_SPP) == 0;

            if from_user {
                // User page fault - ROBUST logging via direct MMIO (no heap, no fmt)
                // This cannot crash because it uses no dynamic allocation
                const UART_BASE: usize = 0x10000000;
                const UART_TX: usize = 0x0;
                const UART_LSR: usize = 0x5;
                const LSR_TX_IDLE: u8 = 1 << 5;

                unsafe {
                    // Helper to write one byte
                    let write_byte = |b: u8| {
                        while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8)
                            & LSR_TX_IDLE
                            == 0
                        {}
                        core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
                    };

                    // Write "[USER-PF] type @ sepc=0x... stval=0x...\n"
                    for &b in b"[USER-PF] " {
                        write_byte(b);
                    }

                    // Fault type
                    let fault_name: &[u8] = match exc {
                        LOAD_PAGE_FAULT => b"LOAD",
                        STORE_PAGE_FAULT => b"STORE",
                        INST_PAGE_FAULT => b"INST",
                        _ => b"???",
                    };
                    for &b in fault_name {
                        write_byte(b);
                    }

                    for &b in b" @ sepc=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.sepc >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }

                    for &b in b" stval=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((stval_now >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }

                    for &b in b" pid=0x" {
                        write_byte(b);
                    }
                    if let Ok(handles) = runtime_kernel_handles_diagnostic() {
                        let pid = handles.tasks.as_ref().current_pid().as_index();
                        for shift in (0..4).rev() {
                            let nibble = ((pid >> (shift * 4)) & 0xf) as u8;
                            let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                            write_byte(ch);
                        }
                    }
                    write_byte(b'\n');

                    // RFC-0004 Phase 1 diagnostics: if this fault hits a known guard page, emit a tag.
                    if let Ok(handles) = runtime_kernel_handles_diagnostic() {
                        let tasks = handles.tasks.as_ref();
                        let current_pid = tasks.current_pid();
                        if let Some(task) = tasks.task(current_pid) {
                            if let Some(info) = task.user_guard_info() {
                                let guard_tag: &[u8] = if stval_now == info.stack_guard_va {
                                    b"STACK"
                                } else if info.info_guard_va == Some(stval_now) {
                                    b"BOOTINFO"
                                } else {
                                    b""
                                };
                                if !guard_tag.is_empty() {
                                    for &b in b"[USER-PF] guard=" {
                                        write_byte(b);
                                    }
                                    for &b in guard_tag {
                                        write_byte(b);
                                    }
                                    write_byte(b'\n');
                                }
                            }
                        }
                    }

                    for &b in b"[USER-PF] regs ra=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[1] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    for &b in b" sp=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[2] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs gp=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[3] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a0=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[10] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a1=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[11] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a2=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[12] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a3=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[13] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    // Additional diagnostics to catch stray branch targets.
                    let regs_to_dump = [
                        (&b"t0"[..], 5usize),
                        (&b"t1"[..], 6usize),
                        (&b"t2"[..], 7usize),
                        (&b"s0"[..], 8usize),
                        (&b"s1"[..], 9usize),
                        (&b"s2"[..], 18usize),
                        (&b"s3"[..], 19usize),
                        (&b"s4"[..], 20usize),
                        (&b"s5"[..], 21usize),
                        (&b"s6"[..], 22usize),
                        (&b"s7"[..], 23usize),
                        (&b"s8"[..], 24usize),
                        (&b"s9"[..], 25usize),
                        (&b"s10"[..], 26usize),
                        (&b"s11"[..], 27usize),
                        (&b"t3"[..], 28usize),
                        (&b"t4"[..], 29usize),
                        (&b"t5"[..], 30usize),
                        (&b"t6"[..], 31usize),
                    ];
                    for &(label, reg_idx) in regs_to_dump.iter() {
                        for &b in b"[USER-PF] regs " {
                            write_byte(b);
                        }
                        for &b in label.iter() {
                            write_byte(b);
                        }
                        for &b in b"=0x" {
                            write_byte(b);
                        }
                        let value = frame.x[reg_idx];
                        for shift in (0..16).rev() {
                            let nibble = ((value >> (shift * 4)) & 0xf) as u8;
                            let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                            write_byte(ch);
                        }
                        write_byte(b'\n');
                    }
                }

                // Snapshot current task's saved frame for additional diagnostics
                if let Ok(handles) = runtime_kernel_handles_diagnostic() {
                    unsafe {
                        let tasks = handles.tasks.as_ref();
                        let spaces = handles.spaces.as_ref();
                        let current_pid = tasks.current_pid();
                        if let Some(task) = tasks.task(current_pid) {
                            dump_user_stack_for_task(task, spaces, frame.x[2]);
                            let tf = task.frame();
                            let write_field = |label: &[u8], value: usize| {
                                let write_byte = |b: u8| {
                                    while core::ptr::read_volatile(
                                        (UART_BASE + UART_LSR) as *const u8,
                                    ) & LSR_TX_IDLE
                                        == 0
                                    {}
                                    core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
                                };
                                for &b in b"[USER-PF] task " {
                                    write_byte(b);
                                }
                                for &b in label {
                                    write_byte(b);
                                }
                                for &b in b"=0x" {
                                    write_byte(b);
                                }
                                for shift in (0..16).rev() {
                                    let nibble = ((value >> (shift * 4)) & 0xf) as u8;
                                    let ch = if nibble < 10 {
                                        b'0' + nibble
                                    } else {
                                        b'a' + (nibble - 10)
                                    };
                                    write_byte(ch);
                                }
                                write_byte(b'\n');
                            };
                            write_field(b"sepc", tf.sepc);
                            write_field(b"sp", tf.x[2]);
                        }
                    }
                }

                // Fail-fast: kill the offending user task and hand control back to the scheduler.
                // Leaving the task alive produces an infinite fault storm and blocks boot markers.
                if let Ok(mut kernel) = KernelGuard::acquire() {
                    // U-mode fault → this hart holds no BKL; safe to acquire.
                    {
                        let (scheduler, tasks, router, spaces, _timer, _ht, _ws, _fences) =
                            kernel.parts();

                        // Kill the faulting task and ensure it won't be scheduled again.
                        let doomed = tasks.current_pid();
                        // RFC-0005 lifecycle hardening: close endpoints owned by the task and wake any blocked peers.
                        let waiters = router.close_endpoints_for_owner(doomed.as_raw());
                        for pid in waiters {
                            match tasks.wake(crate::types::Pid::from_raw(pid), scheduler) {
                                crate::task::WakeOutcome::Woken
                                | crate::task::WakeOutcome::WokenNoopSelftest
                                | crate::task::WakeOutcome::TaskNotBlocked
                                | crate::task::WakeOutcome::TaskNotFound
                                | crate::task::WakeOutcome::EnqueueRejected => {}
                            }
                        }
                        // Also remove this PID from any waiter queues it may be registered in.
                        router.remove_waiter_from_all(doomed.as_raw());
                        tasks.exit_current(-22);
                        scheduler.purge(doomed);
                        scheduler.finish_current();

                        // Select a runnable task and switch to it (bounded attempts to avoid loops).
                        for _ in 0..8 {
                            let Some(next) = scheduler.schedule_next() else {
                                break;
                            };
                            let next_pid = next;
                            tasks.set_current(next_pid);

                            #[cfg(not(feature = "selftest_no_satp"))]
                            {
                                let as_handle =
                                    tasks.task(next_pid).and_then(|t| t.address_space());
                                if let Some(handle) = as_handle {
                                    if spaces.activate(handle).is_err() {
                                        // Fail-fast: this task cannot be safely resumed.
                                        let doomed = tasks.current_pid();
                                        tasks.exit_current(-22);
                                        scheduler.purge(doomed);
                                        scheduler.finish_current();
                                        continue;
                                    }
                                }
                            }

                            if let Some(task) = tasks.task(next_pid) {
                                *frame = *task.frame();
                                return;
                            }
                        }

                        // Fallback: return to PID 0 if possible.
                        tasks.set_current(crate::types::Pid::KERNEL);
                        if let Some(task) = tasks.task(crate::types::Pid::KERNEL) {
                            *frame = *task.frame();
                            return;
                        }
                    }
                }

                // If runtime handles are unavailable, safest is to stop here.
                return;
            }

            // Kernel page fault - emit minimal diagnostics via raw MMIO then panic
            {
                const UART_BASE: usize = 0x10000000;
                const UART_TX: usize = 0x0;
                const UART_LSR: usize = 0x5;
                const LSR_TX_IDLE: u8 = 1 << 5;
                unsafe {
                    let write_byte = |b: u8| {
                        while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8)
                            & LSR_TX_IDLE
                            == 0
                        {}
                        core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
                    };
                    let write_hex = |val: usize, digits: usize| {
                        for shift in (0..digits).rev() {
                            let nibble = ((val >> (shift * 4)) & 0xf) as u8;
                            let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
                            write_byte(ch);
                        }
                    };
                    for &b in b"KPGF sepc=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.sepc, 16);
                    for &b in b" stval=0x" {
                        write_byte(b);
                    }
                    write_hex(stval_now, 16);
                    for &b in b" scause=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.scause, 16);
                    for &b in b" ra=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[1], 16);
                    // Optional symbol hint for the return address (best-effort, allocation-free).
                    #[cfg(all(
                        target_arch = "riscv64",
                        target_os = "none",
                        feature = "trap_symbols"
                    ))]
                    {
                        if let Some((name, off)) = nearest_symbol(frame.x[1]) {
                            for &b in b" sym=" {
                                write_byte(b);
                            }
                            // Emit up to 64 bytes of the symbol name to keep logs bounded.
                            let bytes = name.as_bytes();
                            let n = core::cmp::min(bytes.len(), 64);
                            for &b in &bytes[..n] {
                                write_byte(b);
                            }
                            for &b in b"+0x" {
                                write_byte(b);
                            }
                            write_hex(off, 4);
                        }
                    }
                    for &b in b" sp=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[2], 16);
                    for &b in b" a7=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[17], 16);
                    for &b in b" a0=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[10], 16);
                    for &b in b" a1=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[11], 16);
                    for &b in b" a2=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[12], 16);
                    for &b in b" sstatus=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.sstatus, 16);
                    for &b in b" satp=0x" {
                        write_byte(b);
                    }
                    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                    write_hex(riscv::register::satp::read().bits(), 16);
                    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                    write_hex(0, 16);
                    // Best-effort task context: current PID and its saved sepc (helps catch corrupted frames).
                    if let Ok(handles) = runtime_kernel_handles_diagnostic() {
                        let tasks = handles.tasks.as_ref();
                        let pid = tasks.current_pid();
                        for &b in b" pid=0x" {
                            write_byte(b);
                        }
                        write_hex(pid.as_index(), 4);
                        if let Some(task) = tasks.task(pid) {
                            for &b in b" t_sepc=0x" {
                                write_byte(b);
                            }
                            write_hex(task.frame().sepc, 16);
                        }
                    }
                    write_byte(b'\n');
                }
            }
            record(frame);
            panic!("KPGF");
        }
    }

    // Other exceptions
    {
        // Non-Illegal exceptions: emit minimal diagnostics
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let stval_now = riscv::register::stval::read();
            uart_print_exc(frame.scause, frame.sepc, stval_now);

            // Check if fault is from user mode (SPP=0) or kernel mode (SPP=1)
            const SSTATUS_SPP: usize = 1 << 8;
            let from_user = (frame.sstatus & SSTATUS_SPP) == 0;

            if from_user {
                // User task fault - log and halt
                // CRITICAL: Use safe UART (no allocation/formatting)
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("[USER-FAULT] scause=0x");
                uart_write_hex(&mut u, frame.scause);
                let _ = u.write_str(" sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str(" stval=0x");
                uart_write_hex(&mut u, stval_now);
                let _ = u.write_str("\n");
                // Hang user task: leave `sepc` unchanged so the faulting
                // instruction re-executes indefinitely (no PC advance).
                return;
            }

            // Kernel fault - this is a bug
            if stval_now < 0x1000 {
                panic!("KNULL");
            }
        }
        record(frame);
        panic!("KEXC");
    }
}
