// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-CPU scheduler main loop + context-switch primitives (A3)
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (smp exec cpuN ok, per-hart ticks, KGATE)
//! PUBLIC API: cpu_main(), kmain_secondary(), stage_idle_reentry_frame()
//! DEPENDS_ON: trap::KernelGuard (BKL), smp::HartLocal (resume staging), sched
//! INVARIANTS: no lock is ever held across a context switch (A2b: schedule
//!   decision stages the frame into HartLocal under the BKL, guard drops,
//!   then the sret path reads only hart-local state); secondaries park in
//!   WFI until smp::runtime_ready().
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md

use crate::{
    sched::QosClass,
    task::Pid,
    types::{CpuId, HartId},
};

/// Fresh re-entry into this hart's scheduler loop (target of the idle
/// re-entry frame staged by `stage_idle_reentry_frame`).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" fn idle_reentry() -> ! {
    cpu_main(crate::smp::cpu_current_id())
}

/// Stages `frame` so the trap epilogue sret-returns into THIS hart's
/// `cpu_main` on a fresh stack (A3): used when a syscall leaves no valid
/// user task to resume on this hart. Before A3 the return target fell back
/// to PID 0's stale frame — the BOOT hart's S-mode context — which is
/// catastrophically wrong on a secondary hart.
///
/// Only meaningful once `smp::runtime_ready()` (before that, PID 0's frame
/// IS the live selftest/bring-up context and must keep working).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn stage_idle_reentry_frame(frame: &mut crate::trap::TrapFrame) {
    const SSTATUS_SPP: usize = 1 << 8;
    const SSTATUS_SPIE: usize = 1 << 5;

    let cpu = crate::smp::cpu_current_id();
    let (gp, tp): (usize, usize);
    // SAFETY: reading gp/tp in S-mode kernel context (both are the kernel's).
    unsafe {
        core::arch::asm!(
            "mv {g}, gp", "mv {t}, tp",
            g = out(reg) gp,
            t = out(reg) tp,
            options(nomem, nostack, preserves_flags)
        );
    }
    let sstatus: usize;
    // SAFETY: side-effect-free CSR read.
    unsafe {
        core::arch::asm!("csrr {s}, sstatus", s = out(reg) sstatus, options(nomem, nostack, preserves_flags));
    }

    *frame = crate::trap::TrapFrame::EMPTY;
    frame.sepc = idle_reentry as usize;
    // Fresh stack: cpu_main never returns and holds nothing at entry, so the
    // hart's full stack is available again. The trap epilogue reads the whole
    // frame BEFORE it switches sp (sp is restored last), so overlapping the
    // dead trap frame at the stack top is safe.
    frame.x[2] = crate::smp::hart_stack_top(cpu);
    frame.x[3] = gp;
    frame.x[4] = tp;
    // sret to S-mode with interrupts enabled after the return.
    frame.sstatus = sstatus | SSTATUS_SPP | SSTATUS_SPIE;
}

/// Per-CPU scheduler main loop (A3): every schedule decision runs under the
/// kernel BKL (`trap::KernelGuard`); the chosen task's frame is staged into
/// this hart's `HartLocal` slot and the guard is dropped BEFORE the sret
/// path runs. No lock is ever held across a context switch.
///
/// Boot hart: also drives the timer-cap/IRQ backstop and IPC deadline
/// delivery while idle (v1: PLIC + timer-cap delivery stay boot-owned).
/// Secondary harts: WFI idle; woken by resched IPIs and their own timer.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn cpu_main(cpu: CpuId) -> ! {
    // Once per hart (idle re-entries repeat silently at debug level): proves
    // the hart reached its scheduler loop with the identity it claims.
    static SCHED_LOOP_ANNOUNCED: [core::sync::atomic::AtomicUsize; crate::smp::MAX_CPUS] =
        [const { core::sync::atomic::AtomicUsize::new(0) }; crate::smp::MAX_CPUS];
    if SCHED_LOOP_ANNOUNCED[cpu.as_index()].swap(1, core::sync::atomic::Ordering::AcqRel) == 0 {
        log_info!(target: "smp", "KINIT: cpu{} sched loop", cpu.as_index());
    }
    log_debug!(target: "kmain", "KMAIN: cpu{} entering scheduler loop", cpu.as_index());

    /// Outcome of one guarded scheduling attempt.
    enum Attempt {
        /// Frame staged + AS activated: switch to user (lock-free).
        Switch(*const crate::trap::TrapFrame),
        /// Picked task was not runnable in user mode: retry immediately.
        Retry,
        /// Nothing runnable: idle delivery done, back off.
        Idle,
    }

    // A8 rate gate: at most one steal attempt per millisecond per CPU.
    const STEAL_MIN_INTERVAL_NS: u64 = 1_000_000;
    // A3 exec proof: emitted once per secondary from the boot hart's loop.
    static EXEC_PROOF_EMITTED: [core::sync::atomic::AtomicUsize; crate::smp::MAX_CPUS] =
        [const { core::sync::atomic::AtomicUsize::new(0) }; crate::smp::MAX_CPUS];

    loop {
        if cpu.is_boot() {
            // Watchdog: ensure forward progress; ~10ms in mtimer ticks.
            crate::liveness::check(crate::trap::DEFAULT_TICK_CYCLES * 3);
        }

        static LOOP_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        let count = LOOP_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        if count % 10000 == 0 {
            log_debug!(target: "kmain", "KMAIN: idle_loop iteration {}", count);
        }

        let attempt = {
            let Ok(mut kernel) = crate::trap::KernelGuard::acquire() else {
                // Runtime not installed yet — nothing to schedule.
                core::hint::spin_loop();
                continue;
            };
            let (scheduler, tasks, router, spaces, timer, hart_timers, _waitsets, _fences) =
                kernel.parts();

            // Own queue first, then a bounded, rate-gated steal (A8).
            let picked = scheduler.schedule_next().or_else(|| {
                if crate::smp::cpu_online_mask().count_ones() > 1
                    && crate::smp::steal_rate_gate(cpu, timer.now(), STEAL_MIN_INTERVAL_NS)
                {
                    let stolen = scheduler.steal_into_current(QosClass::PerfBurst);
                    if let Some(pid) = stolen {
                        // Explicit migration: the thief becomes the home CPU.
                        tasks.set_home_cpu(pid, cpu);
                    }
                    stolen
                } else {
                    None
                }
            });

            // Bounded anomaly probe (A3 bring-up): a non-empty queue that
            // schedule_next does not pick would prove an indexing mismatch.
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            if !cpu.is_boot() && picked.is_none() {
                let qlen: usize =
                    [QosClass::Idle, QosClass::Normal, QosClass::Interactive, QosClass::PerfBurst]
                        .iter()
                        .map(|q| scheduler.selftest_queue_len(cpu, *q))
                        .sum();
                if qlen > 0 {
                    static PICK_MISS_LOGGED: core::sync::atomic::AtomicUsize =
                        core::sync::atomic::AtomicUsize::new(0);
                    if PICK_MISS_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 3 {
                        log_error!(
                            target: "smp",
                            "KINIT: cpu{} pick-miss qlen={} (tp_cpu={})",
                            cpu.as_index(),
                            qlen,
                            crate::smp::cpu_current_id().as_index()
                        );
                    }
                }
            }

            if let Some(next_pid) = picked {
                tasks.set_current(next_pid);

                // Bounded bring-up diagnostic: first picks on a secondary,
                // with every validation input visible.
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                if !cpu.is_boot() {
                    static SEC_PICK_LOGGED: core::sync::atomic::AtomicUsize =
                        core::sync::atomic::AtomicUsize::new(0);
                    if SEC_PICK_LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 3 {
                        let (has_as, sstatus, sepc) = tasks
                            .task(next_pid)
                            .map(|t| {
                                (t.address_space().is_some(), t.frame().sstatus, t.frame().sepc)
                            })
                            .unwrap_or((false, 0, 0));
                        log_info!(
                            target: "smp",
                            "KINIT: cpu{} pick pid={} as={} sstatus=0x{:x} sepc=0x{:x}",
                            cpu.as_index(),
                            next_pid,
                            has_as,
                            sstatus,
                            sepc
                        );
                    }
                }

                match tasks.task(next_pid) {
                    None => Attempt::Retry,
                    Some(task) => {
                        const SSTATUS_SPP: usize = 1 << 8;
                        const KERNEL_BASE: usize = 0x80000000;
                        let frame = task.frame();
                        let is_umode = (frame.sstatus & SSTATUS_SPP) == 0;
                        let is_user_addr = frame.sepc < KERNEL_BASE;
                        match task.address_space() {
                            Some(handle) if is_umode && is_user_addr => {
                                if count < 10 {
                                    log_debug!(
                                        target: "kmain",
                                        "KMAIN: switch to U-mode pid={} sepc=0x{:x} sp=0x{:x}",
                                        next_pid,
                                        frame.sepc,
                                        frame.x[2]
                                    );
                                }
                                let staged = crate::smp::hart_local_stage_resume(cpu, frame);
                                if spaces.activate(handle).is_err() {
                                    log_error!(
                                        target: "kmain",
                                        "KMAIN: AS activate failed pid={}",
                                        next_pid
                                    );
                                    Attempt::Retry
                                } else {
                                    crate::smp::record_user_dispatch(cpu);
                                    // A3 proof: emitted by the secondary hart
                                    // itself on its FIRST user dispatch (event-
                                    // anchored; a boot-hart poll can miss the
                                    // harness window).
                                    if !cpu.is_boot()
                                        && EXEC_PROOF_EMITTED[cpu.as_index()]
                                            .swap(1, core::sync::atomic::Ordering::AcqRel)
                                            == 0
                                    {
                                        log_info!(
                                            target: "smp",
                                            "KSELFTEST: smp exec cpu{} ok",
                                            cpu.as_index()
                                        );
                                    }
                                    Attempt::Switch(staged)
                                }
                            }
                            Some(_) => {
                                if count < 10 {
                                    log_debug!(
                                        target: "kmain",
                                        "KMAIN: skip non-user task pid={} (SPP/sepc)",
                                        next_pid
                                    );
                                }
                                Attempt::Retry
                            }
                            None => {
                                if count < 10 {
                                    log_debug!(
                                        target: "kmain",
                                        "KMAIN: skip kernel task pid={} (no AS)",
                                        next_pid
                                    );
                                }
                                Attempt::Retry
                            }
                        }
                    }
                }
            } else if cpu.is_boot() {
                // Reactive idle delivery (nothing runnable): fired timer caps,
                // pending device IRQs, and elapsed IPC deadlines would
                // otherwise be missed here — the async handlers only act on
                // U-mode interrupts. v1: boot-hart-owned (PLIC contexts and
                // timer-cap delivery move per-hart in A6/A7).
                crate::trap::process_expired_timers(timer, hart_timers, router, tasks, scheduler);
                crate::irq::dispatch_external(router, tasks, scheduler);
                let now = timer.now();
                let len = tasks.len();
                for pid_usize in 0..len {
                    let pid = Pid::from_raw(pid_usize as u32);
                    let Some(t) = tasks.task(pid) else {
                        continue;
                    };
                    if !t.is_blocked() {
                        continue;
                    }
                    match t.block_reason() {
                        Some(crate::task::BlockReason::IpcRecv { endpoint, deadline_ns })
                            if deadline_ns != 0 && now >= deadline_ns =>
                        {
                            let _ = router.remove_recv_waiter(endpoint, pid.as_raw());
                            let _ = tasks.wake(pid, scheduler);
                        }
                        Some(crate::task::BlockReason::IpcSend { endpoint, deadline_ns })
                            if deadline_ns != 0 && now >= deadline_ns =>
                        {
                            let _ = router.remove_send_waiter(endpoint, pid.as_raw());
                            let _ = tasks.wake(pid, scheduler);
                        }
                        _ => {}
                    }
                }
                Attempt::Idle
            } else {
                Attempt::Idle
            }
        }; // BKL guard drops HERE — before any context switch (A2b invariant).

        match attempt {
            Attempt::Switch(frame_ptr) => {
                // Legacy tripwire: the runtime must stay visible after the
                // SATP switch (kernel statics are mapped in every AS).
                if !crate::trap::runtime_installed() {
                    panic!("trap runtime invisible after AS activate");
                }
                // Interim TLB safety (until A5): consume a pending lazy flush
                // BEFORE dispatching user code (ASID recycle on another hart).
                if crate::smp::take_lazy_tlb_flush(cpu) {
                    // SAFETY: full local TLB flush; over-invalidation is safe.
                    unsafe {
                        core::arch::asm!("sfence.vma x0, x0", options(nostack, preserves_flags));
                    }
                }
                // A4: guarantee a preemption tick while user code runs.
                #[cfg(feature = "timer_irq")]
                crate::trap::timer_arm(crate::trap::DEFAULT_TICK_CYCLES);
                // SAFETY: hart-local staged frame; AS activated; no locks held.
                unsafe { context_switch_to_task(&*frame_ptr) }
            }
            Attempt::Retry => continue,
            Attempt::Idle => {
                if cpu.is_boot() {
                    // Boot hart keeps its short spin so backstop delivery and
                    // the liveness watchdog stay responsive.
                    for _ in 0..1000 {
                        core::hint::spin_loop();
                    }
                } else {
                    // Secondary WFI idle: mask SIE, sleep unless a wake hint
                    // arrived, unmask. The hint (not RESCHED_PENDING, which
                    // the S_SOFT ack path consumes) closes the lost-wakeup
                    // window: request_resched sets it before the IPI, and only
                    // this loop consumes it. A pending-but-masked interrupt
                    // still wakes WFI.
                    // SAFETY: SIE toggling around wfi; interrupts are handled
                    // right after set_sie re-enables them.
                    // Secondary WFI idle: mask SIE, sleep unless a wake hint
                    // arrived, unmask. The hint (not RESCHED_PENDING, which
                    // the S_SOFT ack path consumes) closes the lost-wakeup
                    // window. A pending-but-masked interrupt still wakes WFI.
                    // WFI (not spinning) is also what keeps QEMU vCPUs parked
                    // instead of burning scheduler quanta on the BKL.
                    // SAFETY: SIE toggling around wfi; interrupts are handled
                    // right after set_sie re-enables them.
                    unsafe {
                        riscv::register::sie::set_ssoft();
                        riscv::register::sie::set_stimer();
                        riscv::register::sstatus::clear_sie();
                        if !crate::smp::take_wake_hint(cpu) {
                            core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
                        }
                        riscv::register::sstatus::set_sie();
                    }
                }
            }
        }
    }
}

/// Pure assembly context switch - loads TrapFrame and executes sret.
/// This function never returns - it jumps to the task's saved PC.
/// CRITICAL: #[inline(always)] ensures the asm! block is inlined directly into the caller.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
unsafe fn context_switch_to_task(frame: &crate::trap::TrapFrame) -> ! {
    // CRITICAL: This function is inline(always), so it's inserted directly after activate()
    // NO Rust code here - only raw pointer conversion and direct assembly jump
    let frame_ptr = frame as *const crate::trap::TrapFrame;

    // Direct jump to assembly - no logs, no temporaries, no cleanup code possible
    unsafe {
        core::arch::asm!(
        // Set up CSRs first (sepc, sstatus)
        "ld t0, {off_sepc}(t3)",
        "csrw sepc, t0",
        "ld t1, {off_sstatus}(t3)",
        "csrw sstatus, t1",

        // Load GPRs from frame (skip x0)
        "ld ra,   1*8(t3)",
        "ld gp,   3*8(t3)",
        "ld tp,   4*8(t3)",
        "ld t0,   5*8(t3)",
        "ld t1,   6*8(t3)",
        "ld t2,   7*8(t3)",
        "ld s0,   8*8(t3)",
        "ld s1,   9*8(t3)",
        "ld a0,  10*8(t3)",
        "ld a1,  11*8(t3)",
        "ld a2,  12*8(t3)",
        "ld a3,  13*8(t3)",
        "ld a4,  14*8(t3)",
        "ld a5,  15*8(t3)",
        "ld a6,  16*8(t3)",
        "ld a7,  17*8(t3)",
        "ld s2,  18*8(t3)",
        "ld s3,  19*8(t3)",
        "ld s4,  20*8(t3)",
        "ld s5,  21*8(t3)",
        "ld s6,  22*8(t3)",
        "ld s7,  23*8(t3)",
        "ld s8,  24*8(t3)",
        "ld s9,  25*8(t3)",
        "ld s10, 26*8(t3)",
        "ld s11, 27*8(t3)",
        "ld t4,  29*8(t3)",
        "ld t5,  30*8(t3)",
        "ld t6,  31*8(t3)",
        // Load sp before restoring pointer register
        "ld sp,   2*8(t3)",
        // Restore pointer register itself LAST to avoid clobbering base early
        "ld t3,  28*8(t3)",

        // Return to task via sret
        "sret",

        in("t3") frame_ptr,
        off_sepc = const core::mem::offset_of!(crate::trap::TrapFrame, sepc),
        off_sstatus = const core::mem::offset_of!(crate::trap::TrapFrame, sstatus),
        options(noreturn)
        );
    }
}

pub(crate) fn kmain_secondary(hart: HartId, stack_top: usize) -> ! {
    let cpu = CpuId::from_hart(hart);
    let stage = &crate::smp::BRINGUP_STAGE[cpu.as_index()];
    stage.store(1, core::sync::atomic::Ordering::Release);
    // Idempotent re-prepare (the boot hart already prepared this block before
    // hart_start); establishes trap stack + identity before any trap or log.
    crate::smp::hart_local_prepare(cpu, stack_top);
    stage.store(2, core::sync::atomic::Ordering::Release);
    // SAFETY: secondary hart installs its own trap vector once before entering the wait loop.
    unsafe {
        crate::trap::install_trap_vector_for(cpu);
    }
    stage.store(3, core::sync::atomic::Ordering::Release);
    crate::smp::mark_cpu_online(cpu);
    stage.store(4, core::sync::atomic::Ordering::Release);
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        // Secondary harts must accept supervisor software interrupts for IPI proofs.
        riscv::register::sie::set_ssoft();
        riscv::register::sstatus::set_sie();
    }

    // Parked until the boot hart finishes selftests + init spawn: that phase
    // mutates kernel state through kmain's direct borrows (not the BKL), so
    // secondaries must not schedule yet. S_SOFT resched evidence and the A2
    // lock-ping selftest stay serviceable while parked. Park in WFI (not a
    // hot spin): under icount/TCG a spinning vCPU steals whole scheduler
    // quanta from the boot hart. The boot hart kicks us via IPI for the
    // lock-ping request and for the runtime-ready release.
    let mut lock_ping_done = false;
    while !crate::smp::runtime_ready() {
        crate::smp::lock_ping_participate(&mut lock_ping_done);
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        // SAFETY: wfi with SIE enabled; any interrupt (IPI/timer) resumes us.
        // Defensively re-assert the S-soft enable: an empty `sie` was observed
        // after the first S_SOFT trap on secondaries (root cause still open),
        // which left the hart deaf to IPIs.
        unsafe {
            riscv::register::sie::set_ssoft();
            core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
        }
        core::hint::spin_loop();
    }

    // A3/A4: join the scheduler with a per-hart preemption tick.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        riscv::register::sie::set_stimer();
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
    crate::trap::timer_arm(crate::trap::DEFAULT_TICK_CYCLES);
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        cpu_main(cpu)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    loop {
        core::hint::spin_loop();
    }
}
