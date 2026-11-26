// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-CPU scheduler main loop (A3); how a picked task starts — the dispatch without
//! the kernel lock (TASK-0363 P2 S3) and the context switch — is `dispatch.rs`
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU SMP proofs (smp exec cpuN ok, per-hart ticks, KGATE)
//! PUBLIC API: cpu_main(), kmain_secondary(), stage_idle_reentry_frame()
//! DEPENDS_ON: trap::KernelGuard (BKL), smp::HartLocal (resume staging), sched
//! INVARIANTS: no lock is ever held across a context switch (A2b: the frame is staged into
//!   HartLocal — from the task's record without the BKL for a task picked from the own queue,
//!   under the BKL otherwise — then the sret path reads only hart-local state); secondaries park
//!   in WFI until smp::runtime_ready().
//! ADR: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md,
//!   docs/adr/0100-a-queued-task-is-the-hart-s-that-takes-it-the-dispatch-takes-no-kernel-lock.md

use crate::{sched::QosClass, types::CpuId};

// How a picked task starts: the unlocked dispatch (TASK-0363 P2 S3) and the context switch.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[path = "dispatch.rs"]
mod dispatch;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use dispatch::{context_switch_to_task, dispatch_unlocked, note_first_user_dispatch};
// A hart without a task: when its idle path takes the lock, how it sleeps, its timer's wakes.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[path = "idle.rs"]
pub(crate) mod idle;

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
/// Iterations of the hart-speed loop: two instructions each, a few milliseconds once per hart.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const SPEED_LOOP_ITERS: u64 = 1_000_000;

/// True once `cpu` has entered its scheduler loop — the per-hart flag
/// (`hart_state`: `sched_loop_entered`) set once at the first `cpu_main` entry, never
/// cleared. THE predicate for "a trap that leaves no runnable task may sret into the idle
/// re-entry frame": before a hart enters its loop, PID 0's frame is that hart's LIVE
/// bring-up context (kmain still running the kernel selftests on the boot hart while the
/// secondaries are already scheduling) and must be resumed, not abandoned — gating on the
/// global `smp::runtime_ready()` did exactly that (boot hart parked out of kmain by the
/// selftest fault-probe kill, init never spawned, 2026-09-08).
#[inline]
pub(crate) fn sched_loop_entered(cpu: CpuId) -> bool {
    crate::smp::hart_state::of(cpu).is_some_and(|hart| {
        hart.sched_loop_entered.load(core::sync::atomic::Ordering::Acquire) != 0
    })
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
/// What the boot stack must have left where the boot hart enters its first idle turn.
const BOOT_STACK_FLOOR: usize = 32 * 1024;

/// The boot hart's first idle turn runs on top of `kmain`'s frames — the one time its stack does
/// not start at the top — and what is left below must hold the idle loop, its traps and their
/// lines. Nothing guards the stack's end: below it lie the kernel's statics. A `kmain` frame of
/// 61 KiB once left 608 bytes (TASK-0363 P3: 20 KiB run queues built inline) and the first trap
/// overwrote them. The depth is a property of the build, so a build that leaves less than the
/// floor ends its boot here, on every lane, with the number.
fn check_boot_stack() {
    extern "C" {
        static __stack_bottom: u8;
    }
    let sp: usize;
    // SAFETY: reads the stack pointer into a register; nothing is written.
    unsafe {
        core::arch::asm!("mv {sp}, sp", sp = out(reg) sp, options(nomem, nostack));
    }
    // SAFETY: a linker symbol's address — the boot stack's lowest byte in the kernel image.
    let bottom = unsafe { &__stack_bottom as *const u8 as usize };
    let top = crate::smp::linker_boot_stack_top();
    if !(bottom..=top).contains(&sp) {
        return;
    }
    let left = sp - bottom;
    log_info!(target: "smp", "KINIT: boot stack (used={} left={} of={})", top - sp, left, top - bottom);
    assert!(left >= BOOT_STACK_FLOOR, "boot stack: {left} bytes left at the first idle turn");
}

pub(crate) fn cpu_main(cpu: CpuId) -> ! {
    // Once per hart (idle re-entries repeat silently at debug level): proves
    // the hart reached its scheduler loop with the identity it claims.
    let first_entry = crate::smp::hart_state::of(cpu).is_some_and(|hart| {
        hart.sched_loop_entered.swap(1, core::sync::atomic::Ordering::AcqRel) == 0
    });
    if first_entry && cpu.is_boot() {
        check_boot_stack();
    }
    if first_entry {
        log_info!(target: "smp", "KINIT: cpu{} sched loop", cpu.as_index());
        // TASK-0330 P0: the hart's speed as the time of one fixed loop of two instructions
        // (the same machine code runs on the stock system for the comparison).
        let (t0, mut left) = (riscv::register::time::read() as u64, SPEED_LOOP_ITERS);
        // SAFETY: a register-only countdown; `left` is the one register written.
        unsafe {
            core::arch::asm!("2: addi {n}, {n}, -1", "bnez {n}, 2b", n = inout(reg) left, options(nomem, nostack));
        }
        let ticks = (riscv::register::time::read() as u64).saturating_sub(t0);
        log_info!(
            target: "smp",
            "KINIT: cpu{} speed (loop_us={} iters={} left={})",
            cpu.as_index(),
            ticks / crate::hal::platform::ticks_per_us().max(1),
            SPEED_LOOP_ITERS,
            left
        );
    }
    log_debug!(target: "kmain", "KMAIN: cpu{} entering scheduler loop", cpu.as_index());
    // EVERY hart must accept S-soft IPIs from here on — only secondaries used
    // to enable it, so the boot hart was DEAF to shootdown doorbells while
    // running userspace: the first secondary-initiated vm_unmap shootdown
    // (RFC-0085) timed out against cpu0, deterministically, in the SMP lane.
    // SAFETY: enabling an interrupt class this hart's trap handler serves.
    unsafe {
        riscv::register::sie::set_ssoft();
    }

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

    // Fresh liveness window at loop entry: the bring-up phase before this loop
    // is bounded but can be LONG without any bump — sequential hart_start waits
    // up to 500ms per LOST hart (`wait_for_online_mask`). Losing hart1 leaves
    // nobody bumping during that wait (hart2/3 start only afterwards), so the
    // FIRST watchdog check below inherited a ~600ms-stale window and panicked
    // before this hart ever scheduled — the fatal hart1-loss boot class (the
    // user-visible "watchdog: no progress" right after "cpu0 sched loop").
    crate::liveness::bump();

    loop {
        if cpu.is_boot() {
            // Watchdog backstop (~20ms): only fires if we can't even ACQUIRE the
            // kernel lock to snapshot (e.g. a wedged BKL holder). The primary
            // watchdog runs INSIDE the guard below at ~10ms and dumps the blocked
            // task set first, so a normal fleet-collapse gets a labeled snapshot.
            crate::liveness::check(crate::trap::default_tick_cycles() * 6);
        }

        static LOOP_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        let count = LOOP_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        if count % 10000 == 0 {
            log_debug!(target: "kmain", "KMAIN: idle_loop iteration {}", count);
        }

        // TASK-0363 P2 S3: this hart's own queue first, without the kernel lock — the picked
        // task's frame, floating-point states and space come from its scheduling record, which
        // this hart holds from the pick on. A task the record cannot dispatch goes to the locked
        // path below, already picked (nothing is lost, nothing is picked twice).
        let fast = crate::trap::runtime_installed()
            .then(crate::sched::Scheduler::pick_here)
            .flatten()
            .map(|pid| dispatch_unlocked(cpu, pid).ok_or(pid));
        // A8 steal, bounded and rate-gated per hart, with nothing of its own to run: under the
        // queue locks alone (TASK-0330 P4) — the stolen task is this hart's running task as a
        // picked one is, and is dispatched the same way.
        let fast = fast.or_else(|| {
            (crate::trap::runtime_installed()
                && crate::smp::cpu_online().count() > 1
                && crate::smp::steal_rate_gate(cpu, idle::now_ns(), STEAL_MIN_INTERVAL_NS))
            .then(|| crate::sched::Scheduler::steal_here(QosClass::PerfBurst))
            .flatten()
            .map(|pid| dispatch_unlocked(cpu, pid).ok_or(pid))
        });
        let attempt = if let Some(Ok(staged)) = fast {
            Attempt::Switch(staged)
        } else if fast.is_none() && !idle::lock_due(cpu) {
            // TASK-0363 P6/P4: nothing picked, nothing stolen, nothing due — no deadline, no
            // timer cap, no stashed line (and on the boot hart no pressure, no heartbeat) — so
            // no kernel lock: sleep.
            Attempt::Idle
        } else if fast.is_none() && idle::caps_due_alone(cpu) {
            // TASK-0330 P4: timer caps alone are due — typically ones a task armed here
            // (windowd's pacer, a driver's poll). This hart delivers them under their own
            // locks, then looks again: the woken task may be its own.
            let fired = crate::trap::deliver_timers().unwrap_or(0);
            idle::note_walk(cpu, true, fired);
            Attempt::Retry
        } else {
            let pre_picked = fast.and_then(Result::err);
            // The idle loop's holds by what they are for (`KINIT: bkl idle split`).
            let holder = match (pre_picked, cpu.is_boot()) {
                (Some(_), _) => crate::lock_share::IDLE,
                (None, true) => crate::lock_share::IDLE_DELIVER,
                (None, false) => crate::lock_share::IDLE_DUE,
            };
            let Ok(mut kernel) = crate::trap::KernelGuard::acquire_as(holder) else {
                // Runtime not installed yet — nothing to schedule.
                core::hint::spin_loop();
                continue;
            };
            let (scheduler, tasks, router, spaces, timer, hart_timers, waitsets, fences) =
                kernel.parts();
            // Where this hold stands, for a stall line (`KINIT: bkl stall`, `smp::tlb`'s tags).
            let stage =
                |n| crate::smp::tlb::note_activity(cpu, crate::smp::tlb::ACT_IDLE_LOCKED, n);
            stage(1);

            // Primary liveness watchdog (~10ms): we hold the guard, so name the
            // blocked task set (who is parked on which endpoint/waitset/fence)
            // before panicking — a fleet-collapse leaves a labeled snapshot for
            // root-causing the lost-wakeup instead of a bare "no progress".
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            if cpu.is_boot() && crate::liveness::is_stalled(crate::trap::default_tick_cycles() * 3)
            {
                crate::liveness::dump_snapshot(tasks, timer.now());
                panic!("watchdog: no progress");
            }

            // The task the unlocked path picked or stole and could not start, else the own
            // queue (a task that arrived since the look above).
            let picked = pre_picked.or_else(|| scheduler.schedule_next());
            stage(2);

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
                        let frame = task.frame();
                        let is_umode = (frame.sstatus & SSTATUS_SPP) == 0;
                        let is_user_addr = !crate::phys::is_kernel_va(frame.sepc);
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
                                stage(3);
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
                                    note_first_user_dispatch(cpu);
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
                // TASK-0363 P6: the one gated deadline walk first (every kind, skipped while
                // nothing is due), then the events — whose re-arm reads the walk's exact gate.
                stage(4);
                let now = timer.now();
                idle::note_delivery(now);
                let t_walk = riscv::register::time::read() as u64;
                let due = idle::timed_due(now);
                let woke = crate::syscall::api::expire_deadlines(
                    now, tasks, router, scheduler, waitsets, fences,
                );
                let t_events = riscv::register::time::read() as u64;
                crate::lock_share::record_deliver(
                    crate::lock_share::DeliverPart::DeadlineWalk,
                    t_walk,
                    t_events.saturating_sub(t_walk),
                );
                stage(5);
                let fired =
                    crate::trap::process_expired_timers(timer, hart_timers, router, scheduler);
                idle::note_walk(cpu, due, woke + fired);
                crate::irq::dispatch_external(router, tasks, scheduler);
                if crate::mm::pressure_endpoint::pending() {
                    crate::mm::pressure_endpoint::dispatch(router, tasks, scheduler);
                }
                crate::lock_share::record_deliver(
                    crate::lock_share::DeliverPart::Events,
                    t_events,
                    (riscv::register::time::read() as u64).saturating_sub(t_events),
                );
                // TASK-0324 P5-a liveness witness (ADR-0062): the watchdog above measures the
                // PROGRESS EPOCH, which every trap bumps — a timer tick included — so it cannot
                // tell a wedged fleet from one that waits quietly. This check names the wedge:
                // no task dispatched since the last sample, every online hart idle, and at least
                // one task blocked. Once per boot, FAIL-shaped so the harness gate fails the lane.
                stage(6);
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                crate::liveness::quiet_stall_witness(cpu, tasks, now);
                // Look again before sleeping (TASK-0363 P4): a delivery that woke a task homed
                // here — the display and input drivers live on this hart — sends no IPI, and a
                // hart that slept on it ran it at its next heartbeat (late_idle 2 -> 859, board
                // 2026-11-16T14-46-46). The look finds nothing due and sleeps when nothing woke.
                Attempt::Retry
            } else if crate::irq::has_stash(cpu) {
                // RFC-0118: a line this hart claimed while it was idle (its driver is homed
                // here) is delivered under the lock held now; the driver it woke is on this
                // hart's queue, so look again instead of sleeping.
                stage(7);
                crate::irq::dispatch_external(router, tasks, scheduler);
                Attempt::Retry
            } else if idle::timed_due(timer.now()) {
                // TASK-0363 P6: a deadline or a timer cap is due — typically one a task that
                // blocked or armed here set this hart's timer for (windowd's pacer, a driver's
                // poll): this hart delivers it, the boot hart may be asleep. Then look again:
                // the woken task may be this hart's.
                stage(8);
                let now = timer.now();
                let woke = crate::syscall::api::expire_deadlines(
                    now, tasks, router, scheduler, waitsets, fences,
                );
                let fired =
                    crate::trap::process_expired_timers(timer, hart_timers, router, scheduler);
                idle::note_walk(cpu, true, woke + fired);
                Attempt::Retry
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
                // A4: guarantee a preemption tick while user code runs — never past this hart's
                // own next deadline (TASK-0330 P4: every switch re-armed the tick, and a hart
                // switching faster than it never took its timer interrupt). From here to the
                // user code with interrupts masked: a deadline already due fires in the user
                // code, whose timer handler delivers it, not in this loop.
                #[cfg(feature = "timer_irq")]
                {
                    // SAFETY: masks this hart's interrupts; the switch to user code re-enables
                    // them (U-mode always takes S-mode interrupts), and nothing below waits.
                    unsafe { riscv::register::sstatus::clear_sie() };
                    crate::trap::dispatch_tick_arm(crate::trap::default_tick_cycles());
                }
                // A shootdown that skipped this hart while it was idle
                // (`CPU_IDLE_MASK`) lands here: flush before any user
                // translation is used again.
                let _ = crate::smp::tlb::poll_mailbox(cpu);
                // SAFETY: hart-local staged frame; AS activated; no locks held.
                unsafe { context_switch_to_task(&*frame_ptr) }
            }
            Attempt::Retry => continue,
            Attempt::Idle => {
                crate::smp::tlb::note_activity(cpu, crate::smp::tlb::ACT_IDLE_LOOP, 0);
                // WFI, not spinning: also what keeps QEMU vCPUs parked instead of burning quanta
                // on the BKL. The boot hart sleeps like the others (TASK-0363 P4).
                idle::sleep(cpu);
            }
        }
    }
}

/// A secondary's first Rust: `cpu` is the logical CPU of the entry record the firmware handed
/// the stub (never derived from the hart id), `stack_top` the stack it runs on.
pub(crate) fn kmain_secondary(cpu: CpuId, stack_top: usize) -> ! {
    crate::smp::note_bringup_stage(cpu, 1);
    // Idempotent re-prepare (the boot hart already prepared this block before
    // hart_start); establishes trap stack + identity before any trap or log.
    crate::smp::hart_local_prepare(cpu, stack_top);
    crate::smp::note_bringup_stage(cpu, 2);
    // SAFETY: secondary hart installs its own trap vector once before entering the wait loop.
    unsafe {
        crate::trap::install_trap_vector_for(cpu);
    }
    crate::smp::note_bringup_stage(cpu, 3);
    crate::smp::mark_cpu_online(cpu);
    crate::smp::note_bringup_stage(cpu, 4);
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

    // A3/A4: join the scheduler with a per-hart preemption tick — and with the external
    // interrupt unmasked (RFC-0118): the controller's contexts are initialised by now, and a
    // line is enabled in this hart's only when a driver homed here bound it.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        riscv::register::sie::set_stimer();
        riscv::register::sie::set_sext();
    }
    #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
    crate::trap::timer_arm(crate::trap::default_tick_cycles());
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        cpu_main(cpu)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    loop {
        core::hint::spin_loop();
    }
}
