// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel main bring-up and idle loop
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU selftests/marker ladder (see scripts/qemu-test.sh)
//! PUBLIC API: kmain()
//! DEPENDS_ON: hal::VirtMachine, mm::AddressSpaceManager, sched::Scheduler, ipc::Router
//! INVARIANTS: Activate kernel AS before complex init; cooperative scheduling via SYSCALL_YIELD
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use alloc::vec::Vec;
use core::{fmt::Write as _, mem::MaybeUninit};

use crate::ipc;
use crate::ipc::header::MessageHeader;
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    hal::{IrqCtl, Tlb},
    mm::{AddressSpaceManager, AsHandle},
    sched::{EnqueueOutcome, QosClass, Scheduler},
    selftest,
    syscall::{api, SyscallTable},
    task::{Pid, TaskTable},
    types::{CpuId, HartId},
};

// (no private selftest stack; kernel stack is provisioned by linker)

/// Aggregated kernel state initialised during boot.
struct KernelState {
    hal: VirtMachine,
    scheduler: Scheduler,
    tasks: TaskTable,
    ipc: ipc::Router,
    address_spaces: AddressSpaceManager,
    #[allow(dead_code)]
    kernel_as: AsHandle,
    #[allow(dead_code)]
    syscalls: SyscallTable,
    #[cfg(target_os = "none")]
    #[allow(dead_code)]
    hart_timers: crate::timer::HartTimers,
    #[cfg(target_os = "none")]
    #[allow(dead_code)]
    waitsets: crate::waitset::WaitsetTable,
    #[cfg(target_os = "none")]
    #[allow(dead_code)]
    fences: crate::fence::FenceTable,
}

static mut KERNEL_STATE: MaybeUninit<KernelState> = MaybeUninit::uninit();

#[allow(static_mut_refs)]
unsafe fn init_kernel_state() -> &'static mut KernelState {
    unsafe { KERNEL_STATE.write(KernelState::new()) }
}

impl KernelState {
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    fn new() -> Self {
        panic!("KernelState::new is only available on riscv64 none target");
    }

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn new() -> Self {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let cap_new: fn() -> crate::cap::CapTable = crate::cap::CapTable::new;
            let cap_wc: fn(usize) -> crate::cap::CapTable = crate::cap::CapTable::with_capacity;
            unsafe {
                let inst_new = core::ptr::read_volatile((cap_new as usize) as *const u32);
                let inst_wc = core::ptr::read_volatile((cap_wc as usize) as *const u32);
                if inst_new == 0 || inst_wc == 0 {
                    panic!("RX-CHK: zero instruction at beacon");
                }
            }
        }
        // Bring up address-space manager and activate kernel AS before any
        // complex code paths (like task/cap table initialisation) to ensure
        // the kernel text/data mappings are active in SATP.
        let mut address_spaces = AddressSpaceManager::new();
        let kernel_as = match address_spaces.create() {
            Ok(handle) => handle,
            Err(err) => {
                use core::fmt::Write as _;
                let mut w = crate::uart::raw_writer();
                let _ = write!(w, "KS-E: as_create {:?}\n", err);
                panic!("kernel address space create failed");
            }
        };
        if let Err(err) = address_spaces.attach(kernel_as, Pid::KERNEL) {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "KS-E: as_attach {:?}\n", err);
        }
        // Activate kernel address space immediately to ensure deterministic
        // RX mapping for subsequent code paths.
        #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "bringup_identity"))]
        if let Err(err) = address_spaces.activate_via_trampoline(kernel_as) {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "KS-E: as_activate tramp {:?}\n", err);
            panic!("kernel address space activate via trampoline failed");
        }
        #[cfg(not(all(
            target_arch = "riscv64",
            target_os = "none",
            feature = "bringup_identity"
        )))]
        if let Err(err) = address_spaces.activate(kernel_as) {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "KS-E: as_activate {:?}\n", err);
            panic!("kernel address space activate failed");
        }

        // The kernel AS is now active, so the identity-mapped fw_cfg window is reachable.
        // Probe the boot mode (proof vs interactive) once — this gates whether later kernel
        // boot markers fold into the verdict grid (interactive) or stay raw (proof). Safe to
        // call here; defaults to raw on any failure. Alloc-free (fixed buffers, MMIO reads).
        crate::boot_mode::detect();

        // Now proceed with task table and the rest of bring-up under the active SATP.
        let mut tasks = TaskTable::new();
        // If an early trap occurred, print it once to aid bring-up debugging.
        if let Some(tf) = crate::trap::last_trap() {
            let mut u = crate::uart::raw_writer();
            let _ = write!(
                u,
                "TRAP-EARLY: scause=0x{:x} sepc=0x{:x} stval=0x{:x}\n",
                tf.scause, tf.sepc, tf.stval
            );
        }
        // Slot 0: bootstrap endpoint loopback
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            let _ = caps.set(
                0,
                Capability {
                    kind: CapabilityKind::Endpoint(0),
                    rights: Rights::SEND | Rights::RECV,
                },
            );
            // Slot 1: identity VMO for bootstrap mappings
            let _ = caps.set(
                1,
                Capability {
                    kind: CapabilityKind::Vmo { base: 0x8000_0000, len: 0x10_0000 },
                    rights: Rights::MAP,
                },
            );
            // Slot 2: endpoint-factory authority (init-lite receives a derived copy).
            let _ = caps.set(
                2,
                Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE },
            );
        }
        // Bind kernel AS handle to bootstrap task
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut scheduler = Scheduler::new();
        if matches!(scheduler.enqueue(Pid::KERNEL, QosClass::Normal), EnqueueOutcome::Rejected(_)) {
            panic!("scheduler bootstrap enqueue rejected");
        }

        let mut syscalls = SyscallTable::new();
        api::install_handlers(&mut syscalls);

        let router = ipc::Router::new(8);

        let hal = VirtMachine::new();
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "kmain", "KS: after VirtMachine::new");

        Self {
            hal,
            scheduler,
            tasks,
            ipc: router,
            address_spaces,
            kernel_as,
            syscalls,
            hart_timers: crate::timer::HartTimers::new(),
            waitsets: crate::waitset::WaitsetTable::new(),
            fences: crate::fence::FenceTable::new(),
        }
    }

    #[allow(dead_code)]
    fn banner(&self) {
        // Decorative boot banner — emitted RAW (no `[LEVEL target]` prefix) so the logo reads
        // as art, not log lines. The single intentional exception to the `[ts] TAG content`
        // console grid. The version line keeps the literal `neuron vers.` (the harness's first
        // proof-of-life marker) so the marker ladder stays green.
        use crate::uart::write_line;
        write_line("");
        write_line(r"     _ __   ___ _   _ _ __ ___  _ __");
        write_line(r"    | '_ \ / _ \ | | | '__/ _ \| '_ \");
        write_line(r"    | | | |  __/ |_| | | | (_) | | | |");
        write_line(r"    |_| |_|\___|\__,_|_|  \___/|_| |_|");
        write_line("");
        write_line("    neuron vers. 0.1.0  ·  One OS. Many Devices.");
        write_line("");
        self.assert_memory_layout();
    }

    /// P0.1 layout audit (2026-07-07): the boot proceeds over several
    /// FIXED-ADDRESS windows (stack pool, kernel page pool, user VMO arena)
    /// whose neighbors move with the image. A silent overlap corrupts
    /// distant subsystems (the `.data`-cursor zero-guards in StackPool /
    /// alloc_init_page exist BECAUSE this happened before). Check the
    /// invariants ONCE at boot and report loudly — with values — instead of
    /// failing later as an anonymous StackExhausted/oom.
    fn assert_memory_layout(&self) {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            extern "C" {
                static __bss_end: u8;
            }
            // P0.1 perturbation-gate anchor: a compile-time-sized rodata pad
            // that is genuinely REFERENCED (volatile read below), so no
            // linker pass can ever collect it. Appending an unreferenced
            // `#[used] #[no_mangle]` static to a compiled file proved to be
            // a PLACEBO — gc-sections dropped it and the gate's landing
            // check (`pad did not land`) caught exactly that. Sized via
            // `NEURON_LAYOUT_PAD` (contract-image-layout.sh); 0 = zero cost.
            const LAYOUT_PAD_LEN: usize = {
                match option_env!("NEURON_LAYOUT_PAD") {
                    Some(s) => {
                        let b = s.as_bytes();
                        let mut v = 0usize;
                        let mut i = 0;
                        while i < b.len() {
                            if b[i] >= b'0' && b[i] <= b'9' {
                                v = v * 10 + (b[i] - b'0') as usize;
                            }
                            i += 1;
                        }
                        v
                    }
                    None => 0,
                }
            };
            static LAYOUT_PAD: [u8; LAYOUT_PAD_LEN] = [0xA5; LAYOUT_PAD_LEN];
            let pad_probe: usize = if LAYOUT_PAD_LEN > 0 {
                // Volatile read takes the array's ADDRESS — the pad must
                // exist in the image (no const-fold, no GC).
                unsafe { core::ptr::read_volatile(LAYOUT_PAD.as_ptr()) as usize }
            } else {
                0
            };
            let image_end = core::ptr::addr_of!(__bss_end) as usize;
            let pool_base = crate::mm::KERNEL_PAGE_POOL_BASE;
            let pool_end = pool_base + crate::mm::KERNEL_PAGE_POOL_LEN;
            let arena_base = crate::mm::USER_VMO_ARENA_BASE;
            let arena_end = arena_base + crate::mm::USER_VMO_ARENA_LEN;
            let mut ok = true;
            if image_end > pool_base {
                log_error!(
                    "LAYOUT: kernel image end 0x{:x} overlaps page pool 0x{:x} — image grew past the window",
                    image_end,
                    pool_base
                );
                ok = false;
            }
            if pool_end > arena_base {
                log_error!(
                    "LAYOUT: page pool end 0x{:x} overlaps VMO arena 0x{:x}",
                    pool_end,
                    arena_base
                );
                ok = false;
            }
            // Headroom report (values, not vibes): how far the image may
            // still grow before it hits the pool window.
            let headroom = pool_base.saturating_sub(image_end);
            // Armed pad must really be in memory with its fill byte — the
            // volatile read above took its address, this checks its content.
            if LAYOUT_PAD_LEN > 0 && pad_probe != 0xA5 {
                log_error!("LAYOUT: pad probe mismatch (read 0x{:x}, want 0xa5)", pad_probe);
                ok = false;
            }
            if ok {
                log_info!(
                    target: "kmain",
                    "KERNEL: layout ok (image_end=0x{:x} pool=0x{:x} headroom={}K arena_end=0x{:x} pad={})",
                    image_end,
                    pool_base,
                    headroom / 1024,
                    arena_end,
                    LAYOUT_PAD_LEN
                );
            }
            // Under 64K of slack is a red flag long before it is a failure.
            if ok && headroom < 64 * 1024 {
                log_error!(
                    "LAYOUT: only {} bytes headroom between kernel image and page pool",
                    headroom
                );
            }
        }
    }

    #[allow(dead_code)]
    fn exercise_ipc(&mut self) {
        // Send a bootstrap message to prove IPC wiring works before tasks run.
        let header = MessageHeader::new(0, 0, 0x100, 0, 0);
        if self.ipc.send(0, ipc::Message::new(header, Vec::new(), None)).is_ok() {
            let _ = self.ipc.recv(0);
        }
    }
}

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
fn cpu_main(cpu: CpuId) -> ! {
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

/// Kernel main invoked after boot assembly completed.
/// CRITICAL: Activate kernel address space before complex init; idle loop uses SYSCALL_YIELD.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn kmain() -> ! {
    #[cfg(feature = "boot_timing")]
    let t0 = crate::arch::riscv::read_time();
    let kernel = unsafe { init_kernel_state() };
    crate::smp::init_boot_hart_state();
    // Provide the syscall/trap runtime with a stable timer reference. We store it as a raw pointer
    // to avoid borrowing `kernel.hal` for `'static` (we keep mutating `kernel` afterwards).
    let timer: &'static dyn crate::hal::Timer = unsafe {
        let t: &dyn crate::hal::Timer = kernel.hal.timer();
        &*(t as *const dyn crate::hal::Timer)
    };
    let _default_trap_domain = crate::trap::install_runtime(
        &mut kernel.scheduler,
        &mut kernel.tasks,
        &mut kernel.ipc,
        &mut kernel.address_spaces,
        timer,
        &mut kernel.hart_timers,
        &mut kernel.waitsets,
        &mut kernel.fences,
        &kernel.syscalls,
    );
    kernel.tasks.bootstrap_mut().set_trap_domain(_default_trap_domain);

    let expected_online_mask = crate::smp::start_secondary_harts();
    // Bounded HSM bring-up with retry: a start request issued in quick
    // succession can get lost (hart never reaches the kernel entry despite a
    // success return); retry up to 3 times before declaring the timeout.
    let mut online_ok = crate::smp::wait_for_online_mask(expected_online_mask, 500_000_000);
    for _ in 0..3 {
        if online_ok {
            break;
        }
        crate::smp::retry_missing_harts(expected_online_mask);
        online_ok = crate::smp::wait_for_online_mask(expected_online_mask, 500_000_000);
    }
    // Boot gate: per-hart evidence + verdict. A lost hart is loud but NOT
    // fatal — the system boots degraded with the online set (the SMP proof
    // gate still requires its markers, so CI catches it honestly).
    crate::smp::emit_bringup_gate(expected_online_mask);
    let _ = online_ok;
    // Touch HAL traits to satisfy imports
    let uart_dev = kernel.hal.uart();
    let _: &dyn crate::hal::Uart = uart_dev;
    kernel.hal.tlb().flush_all();
    kernel.hal.irq().disable(0);
    kernel.hal.irq().enable(0);
    #[cfg(feature = "boot_timing")]
    {
        let t1 = crate::arch::riscv::read_time();
        let delta = (t1 - t0) as u64;
        use core::fmt::Write as _;
        let mut u = crate::uart::KernelUart::lock();
        let _ = write!(u, "T:init={}\n", delta);
    }
    // Minimal IO only
    #[cfg(feature = "boot_banner")]
    kernel.banner();
    // Keep timer interrupts disabled during selftest to avoid preemption/trap stack usage
    // Keep UART usage minimal before selftests
    // kernel.exercise_ipc();
    // (no pointer debug formatting in OS stage policy)
    // Quick sanity for OpenSBI environment
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        #[cfg(feature = "debug_uart")]
        log_info!(target: "env", "ENV: sbi present");
    }
    #[cfg(feature = "boot_timing")]
    let t2 = crate::arch::riscv::read_time();
    {
        let mut ctx = selftest::Context {
            hal: &kernel.hal,
            router: &mut kernel.ipc,
            address_spaces: &mut kernel.address_spaces,
            tasks: &mut kernel.tasks,
            scheduler: &mut kernel.scheduler,
            hart_timers: &mut kernel.hart_timers,
            waitsets: &mut kernel.waitsets,
            fences: &mut kernel.fences,
        };
        // Gate: validate target PC and dump RA/SP before entering selftest
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            extern "C" {
                static __text_start: u8;
                static __text_end: u8;
            }
            let target_pc = selftest::entry as usize;
            let start = unsafe { &__text_start as *const u8 as usize };
            let end = unsafe { &__text_end as *const u8 as usize };
            if !(target_pc >= start && target_pc < end && (target_pc & 0x3) == 0) {
                panic!("GATE: invalid selftest entry pc=0x{:x}", target_pc);
            }
            let _ra: usize;
            let _sp: usize;
            unsafe {
                core::arch::asm!("mv {o}, ra", o = out(reg) _ra, options(nostack, preserves_flags));
            }
            unsafe {
                core::arch::asm!("mv {o}, sp", o = out(reg) _sp, options(nostack, preserves_flags));
            }
            #[cfg(feature = "debug_uart")]
            {
                let mut u = crate::uart::raw_writer();
                let _ = write!(
                    u,
                    "GATE: before selftest ra=0x{:x} sp=0x{:x} pc=0x{:x}\n",
                    _ra, _sp, target_pc
                );
            }
        }
        // Prefer private selftest stack on OS if enabled; applies to full suite and spawn-only
        #[cfg(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none"))]
        selftest::entry_on_private_stack(&mut ctx);
        #[cfg(not(all(
            feature = "selftest_priv_stack",
            target_arch = "riscv64",
            target_os = "none"
        )))]
        selftest::entry(&mut ctx);
        // Userspace acceptance markers are emitted by daemons and selftest-client.
        // RFC-0068: fold the bounded kernel-init phases (kself, syscalls, sched, boot, smp) into one
        // `<subject> N/N OK <ms>` grid verdict each (interactive boots only; proof boots emitted
        // them raw for verify-uart). By this point — after the selftest, before the idle loop hands
        // off to userspace — each of these phases has emitted all its markers, so the flush pairs
        // with the suppression in diag::log so no folded marker is ever dropped without a verdict.
        // (`as` flushes separately, on its rate-limiter, since it is fed by userspace switches.)
        crate::log::kflush_kernel_phase();
    }
    #[cfg(feature = "boot_timing")]
    {
        let t3 = crate::arch::riscv::read_time();
        let delta = (t3 - t2) as u64;
        use core::fmt::Write as _;
        let mut u = crate::uart::KernelUart::lock();
        let _ = write!(u, "T:selftest={}\n", delta);
    }
    // End of kernel bring-up; user-mode services are responsible for
    // emitting their own readiness markers.

    // Arm the reactive, preemptive scheduler tick. Selftests have completed and the
    // trap runtime is installed, so from here a supervisor timer IRQ (1) delivers
    // fired timer caps reactively and (2) preempts long-running user tasks, so no
    // single service can monopolise the cooperative scheduler. Done last to keep all
    // earlier bring-up/selftest sequencing non-preemptive.
    #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
    unsafe {
        crate::trap::timer_arm(crate::trap::DEFAULT_TICK_CYCLES);
        crate::trap::enable_timer_interrupts();
    }

    // Reactive device input: initialise the PLIC and unmask supervisor external
    // interrupts. No source is enabled until a driver binds its IRQ (irq_bind), so
    // nothing fires here; binding routes the device IRQ to the driver's endpoint.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        crate::hal::plic::plic_init();
        riscv::register::sie::set_sext();
    }

    // A6 structural proofs (SMP>=2 only, right after plic_init): every online
    // hart's S-context is addressable + initialised (threshold readback), and
    // per-context enable bitmaps are isolated (a probe source enabled for the
    // boot context must not leak into cpu1's).
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    if crate::smp::cpu_online_mask().count_ones() > 1 {
        let mut ctx_ok = true;
        for idx in 0..crate::smp::MAX_CPUS {
            let cpu = CpuId::from_raw(idx as u16);
            if !crate::smp::cpu_is_online(cpu) {
                continue;
            }
            if crate::hal::plic::selftest_ctx_threshold(cpu) == 0 {
                log_info!(target: "smp", "KSELFTEST: plic ctx cpu{} ok", idx);
            } else {
                ctx_ok = false;
                log_error!(target: "smp", "KSELFTEST: plic ctx cpu{} FAIL", idx);
            }
        }
        // Probe source 90 is unwired on QEMU virt (virtio 1..8, uart 10,
        // rtc 11, pci 32..35) — enabling it cannot fire anything.
        if let Some(probe) = crate::hal::plic::IrqId::new(90) {
            let cpu1 = CpuId::from_raw(1);
            crate::hal::plic::enable_source(probe, CpuId::BOOT);
            let boot_sees = crate::hal::plic::selftest_source_enabled(probe, CpuId::BOOT);
            let cpu1_sees = crate::hal::plic::selftest_source_enabled(probe, cpu1);
            crate::hal::plic::disable_source(probe, CpuId::BOOT);
            if ctx_ok && boot_sees && !cpu1_sees {
                log_info!(target: "smp", "KSELFTEST: plic isolation ok");
            } else {
                log_error!(
                    target: "smp",
                    "KSELFTEST: plic isolation FAIL boot={} cpu1={}",
                    boot_sees,
                    cpu1_sees
                );
            }
        }
    }

    // A7: announce the active per-hart timer mechanism. SBI set_timer is the
    // v1 path; SSTC (`stimecmp`, saves the SBI trap per tick) is a documented
    // follow-up pending a fault-safe detection probe.
    log_info!(target: "smp", "KINIT: timer sbi");

    // A3: selftests + init spawn are done — release the secondaries into
    // their scheduler loops (IPI punches them out of their park-WFI), then
    // become CPU 0's scheduler.
    crate::smp::mark_runtime_ready();
    for idx in 1..crate::smp::MAX_CPUS {
        let target = CpuId::from_raw(idx as u16);
        if crate::smp::cpu_is_online(target) {
            let _ = crate::smp::request_resched(target);
        }
    }
    cpu_main(CpuId::BOOT)
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn kmain() -> ! {
    panic!("kmain is only available on riscv64 none target");
}
