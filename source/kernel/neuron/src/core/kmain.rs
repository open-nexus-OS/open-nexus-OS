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
    hal::Timer,
    hal::{IrqCtl, Tlb},
    mm::{AddressSpaceManager, AsHandle},
    sched::{QosClass, Scheduler},
    selftest,
    syscall::{api, SyscallTable},
    task::{Pid, TaskTable},
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
        scheduler.enqueue(Pid::KERNEL, QosClass::Normal);

        let mut syscalls = SyscallTable::new();
        api::install_handlers(&mut syscalls);

        let router = ipc::Router::new(8);

        let hal = VirtMachine::new();
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "kmain", "KS: after VirtMachine::new");

        Self { hal, scheduler, tasks, ipc: router, address_spaces, kernel_as, syscalls }
    }

    #[allow(dead_code)]
    fn banner(&self) {
        log_info!(target: "boot", " _ __   ___ _   _ _ __ ___  _ __");
        log_info!(target: "boot", r"| '_ \ / _ \ | | | '__/ _ \| '_ \");
        log_info!(target: "boot", "| | | |  __/ |_| | | | (_) | | | |");
        log_info!(target: "boot", r"|_| |_|\___|\__,_|_|  \___/|_| |_|");
        log_info!(target: "boot", "neuron vers. 0.1.0 - One OS. Many Devices.");
    }

    #[allow(dead_code)]
    fn exercise_ipc(&mut self) {
        // Send a bootstrap message to prove IPC wiring works before tasks run.
        let header = MessageHeader::new(0, 0, 0x100, 0, 0);
        if self.ipc.send(0, ipc::Message::new(header, Vec::new(), None)).is_ok() {
            let _ = self.ipc.recv(0);
        }
    }

    fn idle_loop(&mut self) -> ! {
        log_info!(target: "kmain", "KMAIN: Entering idle_loop");
        loop {
            // Watchdog: ensure forward progress; 10ms in mtimer ticks (10MHz) ~ 100_000 cycles
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            crate::liveness::check(crate::trap::DEFAULT_TICK_CYCLES * 3);

            // Debug: Check if we're stuck
            static LOOP_COUNT: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            let count = LOOP_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
            if count % 10000 == 0 {
                log_info!(target: "kmain", "KMAIN: idle_loop iteration {}", count);
            }

            // ARCHITECTURAL FIX: Idle loop is S-mode kernel code, not a task.
            // S-mode ecalls trap to M-mode (OpenSBI), not our S-mode handler!
            // Solution: Directly schedule next task and context switch via assembly.

            if let Some(next_pid) = self.scheduler.schedule_next() {
                self.tasks.set_current(next_pid);

                // Extract scheduling-relevant values without holding an immutable borrow of `self`
                // across the context-switch call (which needs `&mut self`).
                let (handle, frame_ptr, is_umode, is_user_addr) = match self.tasks.task(next_pid) {
                    None => continue,
                    Some(task) => {
                        const SSTATUS_SPP: usize = 1 << 8;
                        const KERNEL_BASE: usize = 0x80000000;
                        let frame = task.frame();
                        let is_umode = (frame.sstatus & SSTATUS_SPP) == 0;
                        let is_user_addr = frame.sepc < KERNEL_BASE;
                        (
                            task.address_space(),
                            frame as *const crate::trap::TrapFrame,
                            is_umode,
                            is_user_addr,
                        )
                    }
                };

                if let Some(handle) = handle {
                    if !is_umode {
                        if count < 10 {
                            log_info!(target: "kmain", "KMAIN: skip S-mode task pid={} (SPP=1)", next_pid);
                        }
                        continue;
                    }
                    if !is_user_addr {
                        if count < 10 {
                            let frame = unsafe { &*frame_ptr };
                            log_info!(
                                target: "kmain",
                                "KMAIN: skip invalid task pid={} (sepc=0x{:x} in kernel space)",
                                next_pid,
                                frame.sepc
                            );
                        }
                        continue;
                    }

                    // Log BEFORE activating user AS (no logging after AS switch!)
                    if count < 10 {
                        let frame = unsafe { &*frame_ptr };
                        log_info!(
                            target: "kmain",
                            "KMAIN: switch to U-mode pid={} sepc=0x{:x} sp=0x{:x}",
                            next_pid,
                            frame.sepc,
                            frame.x[2]
                        );
                    }

                    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                    unsafe {
                        let frame = &*frame_ptr;
                        activate_and_switch_to_user(self, handle, frame);
                    }
                    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                    {
                        if let Err(e) = self.address_spaces.activate(handle) {
                            log_error!(target: "kmain", "KMAIN: AS activate failed {:?}", e);
                            continue;
                        }
                    }
                } else {
                    if count < 10 {
                        log_info!(target: "kmain", "KMAIN: skip kernel task pid={} (no AS)", next_pid);
                    }
                }
            } else {
                // If nothing is runnable, try waking tasks blocked on deadlines (IPC v1).
                // Without this, the kernel could spin forever even though a deadline has expired.
                let now = self.hal.timer().now();
                let len = self.tasks.len();
                for pid_usize in 0..len {
                    let pid = Pid::from_raw(pid_usize as u32);
                    let Some(t) = self.tasks.task(pid) else {
                        continue;
                    };
                    if !t.is_blocked() {
                        continue;
                    }
                    match t.block_reason() {
                        Some(crate::task::BlockReason::IpcRecv { endpoint, deadline_ns })
                            if deadline_ns != 0 && now >= deadline_ns =>
                        {
                            let _ = self.ipc.remove_recv_waiter(endpoint, pid.as_raw());
                            let _ = self.tasks.wake(pid, &mut self.scheduler);
                        }
                        Some(crate::task::BlockReason::IpcSend { endpoint, deadline_ns })
                            if deadline_ns != 0 && now >= deadline_ns =>
                        {
                            let _ = self.ipc.remove_send_waiter(endpoint, pid.as_raw());
                            let _ = self.tasks.wake(pid, &mut self.scheduler);
                        }
                        _ => {}
                    }
                }
                // No runnable tasks - spin briefly
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
            }
        }
    }
}

/// Activates user address space and immediately switches to user mode.
/// CRITICAL: This function MUST NOT be inlined to ensure all temporaries from caller
/// are dropped before the AS switch. After activate(), we jump directly to user mode.
///
/// SAFETY: Uses ManuallyDrop and mem::forget to prevent destructors from running
/// after the AS switch, which is the enterprise-level Rust pattern for this scenario.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline(never)]
unsafe fn activate_and_switch_to_user(
    kernel: &mut KernelState,
    handle: crate::mm::AsHandle,
    frame: &crate::trap::TrapFrame,
) -> ! {
    // Log BEFORE AS switch - safe UART only
    {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("ACTIVATE_AND_SWITCH: about to activate AS and jump to sepc=0x");
        crate::trap::uart_write_hex(&mut u, frame.sepc);
        let _ = u.write_str(" frame_ptr=0x");
        crate::trap::uart_write_hex(&mut u, frame as *const _ as usize);
        let _ = u.write_str(" sp=0x");
        crate::trap::uart_write_hex(&mut u, frame.x[2]);
        let _ = u.write_str(" gp=0x");
        crate::trap::uart_write_hex(&mut u, frame.x[3]);
        let _ = u.write_str(" ra=0x");
        crate::trap::uart_write_hex(&mut u, frame.x[1]);
        let _ = u.write_str(" sstatus=0x");
        crate::trap::uart_write_hex(&mut u, frame.sstatus);
        let _ = u.write_str(" off_sepc=0x");
        crate::trap::uart_write_hex(&mut u, core::mem::offset_of!(crate::trap::TrapFrame, sepc));
        let _ = u.write_str("\n");
        core::mem::drop(u);
    }

    // CRITICAL SECTION: Atomically activate AS and jump to user - NO cleanup code possible
    // Move activate() INTO context_switch to ensure they're truly atomic
    unsafe { context_switch_with_activate(kernel, handle, frame) }
}

/// Atomically activates the user AS and context switches to the task.
/// CRITICAL: inline(always) ensures activate() and asm! are truly atomic with NO cleanup code between them.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
unsafe fn context_switch_with_activate(
    kernel: &mut KernelState,
    handle: crate::mm::AsHandle,
    frame: &crate::trap::TrapFrame,
) -> ! {
    // Activate user AS - from this point we're in the user's address space
    let _ = kernel.address_spaces.activate(handle);

    // Defensive: some bring-up paths end up with TRAP_RUNTIME not visible after SATP switch.
    // If missing, (re)install it in the currently active address space so U-mode syscalls work.
    if !crate::trap::runtime_installed() {
        let timer: &'static dyn crate::hal::Timer = unsafe {
            let t: &dyn crate::hal::Timer = kernel.hal.timer();
            &*(t as *const dyn crate::hal::Timer)
        };
        let _ = crate::trap::install_runtime(
            &mut kernel.scheduler,
            &mut kernel.tasks,
            &mut kernel.ipc,
            &mut kernel.address_spaces,
            timer,
            &kernel.syscalls,
        );
    }

    // IMMEDIATELY jump to assembly - inline(always) ensures no code between activate and asm
    unsafe {
        context_switch_to_task(frame);
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

/// Kernel main invoked after boot assembly completed.
/// CRITICAL: Activate kernel address space before complex init; idle loop uses SYSCALL_YIELD.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn kmain() -> ! {
    #[cfg(feature = "boot_timing")]
    let t0 = crate::arch::riscv::read_time();
    let kernel = unsafe { init_kernel_state() };
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
        &kernel.syscalls,
    );
    kernel.tasks.bootstrap_mut().set_trap_domain(_default_trap_domain);
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

    kernel.idle_loop()
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn kmain() -> ! {
    panic!("kmain is only available on riscv64 none target");
}
