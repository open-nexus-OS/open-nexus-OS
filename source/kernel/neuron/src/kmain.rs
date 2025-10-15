// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel main routine responsible for subsystem bring-up.

use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::{
    arch::riscv,
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    hal::{IrqCtl, Tlb},
    ipc::{self, header::MessageHeader},
    mm::{AddressSpaceManager, AsHandle},
    sched::{QosClass, Scheduler},
    selftest,
    syscall::{self, api, SyscallTable},
    task::TaskTable,
    uart,
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

impl KernelState {
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
        if let Err(err) = address_spaces.attach(kernel_as, 0) {
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
        #[cfg(not(all(target_arch = "riscv64", target_os = "none", feature = "bringup_identity")))]
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
        }
        // Bind kernel AS handle to bootstrap task
        tasks.bootstrap_mut().address_space = Some(kernel_as);

        let mut scheduler = Scheduler::new();
        scheduler.enqueue(0, QosClass::Normal);

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
        log_info!(target: "boot", "| '_ \\\ / _ \\\ | | | '__/ _ \\| '_ \\");
        log_info!(target: "boot", "| | | |  __/ |_| | | | (_) | | | |");
        log_info!(target: "boot", "|_| |_|\\\\___|\\\\__,_|_|  \\\\___/|_| |_|");
        log_info!(target: "boot", "neuron vers. 0.1.0 - One OS. Many Devices.");
    }

    #[allow(dead_code)]
    fn exercise_ipc(&mut self) {
        // Send a bootstrap message to prove IPC wiring works before tasks run.
        let header = MessageHeader::new(0, 0, 0x100, 0, 0);
        if self.ipc.send(0, ipc::Message::new(header, Vec::new())).is_ok() {
            let _ = self.ipc.recv(0);
        }
    }

    fn idle_loop(&mut self) -> ! {
        loop {
            // Watchdog: ensure forward progress; 10ms in mtimer ticks (10MHz) ~ 100_000 cycles
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            crate::liveness::check(crate::trap::DEFAULT_TICK_CYCLES * 3);
            // Register trap fastpath environment for S-mode SYSCALL_YIELD once per loop
            unsafe {
                crate::trap::register_scheduler_env(
                    &mut self.scheduler,
                    &mut self.tasks,
                    &mut self.ipc,
                    &mut self.address_spaces,
                );
            }
            let _ctx = api::Context::new(
                &mut self.scheduler,
                &mut self.tasks,
                &mut self.ipc,
                &mut self.address_spaces,
                self.hal.timer(),
            );
            // Real S-mode ECALL to enter trap path and switch to next task frame
            unsafe {
                core::arch::asm!(
                    "li a7, {id}\n ecall",
                    id = const syscall::SYSCALL_YIELD,
                    options(nostack)
                );
            }
            riscv::wait_for_interrupt();
        }
    }
}

/// Kernel main invoked after boot assembly completed.
pub fn kmain() -> ! {
    #[cfg(feature = "boot_timing")]
    let t0 = crate::arch::riscv::read_time();
    let mut kernel = KernelState::new();
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
        // Make trap fastpath available to selftests for real scheduling via ECALL
        // before borrowing components into the selftest context.
        unsafe {
            crate::trap::register_scheduler_env(
                &mut kernel.scheduler,
                &mut kernel.tasks,
                &mut kernel.ipc,
                &mut kernel.address_spaces,
            );
        }
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
            let ra: usize;
            let sp: usize;
            unsafe { core::arch::asm!("mv {o}, ra", o = out(reg) ra, options(nostack, preserves_flags)); }
            unsafe { core::arch::asm!("mv {o}, sp", o = out(reg) sp, options(nostack, preserves_flags)); }
            #[cfg(feature = "debug_uart")]
            {
                let mut u = crate::uart::raw_writer();
                let _ = write!(u, "GATE: before selftest ra=0x{:x} sp=0x{:x} pc=0x{:x}\n", ra, sp, target_pc);
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
