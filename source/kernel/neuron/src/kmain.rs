// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel main routine responsible for subsystem bring-up.

use alloc::vec::Vec;

use crate::{
    arch::riscv,
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    ipc::{self, header::MessageHeader},
    mm::PageTable,
    sched::{QosClass, Scheduler},
    selftest,
    syscall::{self, api, SyscallTable},
    task::TaskTable,
    uart,
};

/// Aggregated kernel state initialised during boot.
struct KernelState {
    hal: VirtMachine,
    scheduler: Scheduler,
    tasks: TaskTable,
    ipc: ipc::Router,
    address_space: PageTable,
    syscalls: SyscallTable,
}

impl KernelState {
    fn new() -> Self {
        uart::write_line("KS: new enter");
        let mut tasks = TaskTable::new();
        uart::write_line("KS: after TaskTable::new");
        // Slot 0: bootstrap endpoint loopback
        {
            let caps = tasks.bootstrap_mut().caps_mut();
            let _ = caps.set(
                0,
                Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
            );
            uart::write_line("KS: after caps.set ep0");
            // Slot 1: identity VMO for bootstrap mappings
            let _ = caps.set(
                1,
                Capability {
                    kind: CapabilityKind::Vmo { base: 0x8000_0000, len: 0x10_0000 },
                    rights: Rights::MAP,
                },
            );
            uart::write_line("KS: after caps.set vmo1");
        }
        uart::write_line("KS: after TaskTable bootstrap caps");
        let mut scheduler = Scheduler::new();
        uart::write_line("KS: after Scheduler::new");
        scheduler.enqueue(0, QosClass::Idle);
        uart::write_line("KS: after scheduler.enqueue");

        let mut syscalls = SyscallTable::new();
        uart::write_line("KS: after SyscallTable::new");
        api::install_handlers(&mut syscalls);
        uart::write_line("KS: after install_handlers");

        let router = ipc::Router::new(4);
        uart::write_line("KS: after Router::new");

        let address_space = PageTable::new();
        uart::write_line("KS: after PageTable::new");

        let hal = VirtMachine::new();
        uart::write_line("KS: after VirtMachine::new");

        uart::write_line("KS: returning");
        Self { hal, scheduler, tasks, ipc: router, address_space, syscalls }
    }

    fn banner(&self) {
        uart::write_line(
            "

 _ __   ___ _   _ _ __ ___  _ __
| '_ \\ / _ \\ | | | '__/ _ \\| '_ \\
| | | |  __/ |_| | | | (_) | | | |
|_| |_|\\___|\\__,_|_|  \\___/|_| |_|

                                  ",
        );
        uart::write_line("neuron vers. 0.1.0 - One OS. Many Devices.");
    }

    fn exercise_ipc(&mut self) {
        // Send a bootstrap message to prove IPC wiring works before tasks run.
        let header = MessageHeader::new(0, 0, 0x100, 0, 0);
        if self.ipc.send(0, ipc::Message::new(header, Vec::new())).is_ok() {
            let _ = self.ipc.recv(0);
        }
    }

    fn idle_loop(&mut self) -> ! {
        loop {
            let mut ctx = api::Context::new(
                &mut self.scheduler,
                &mut self.tasks,
                &mut self.ipc,
                &mut self.address_space,
                self.hal.timer(),
            );
            let mut frame = crate::trap::TrapFrame::default();
            frame.x[17] = syscall::SYSCALL_YIELD; // a7 = x17
            crate::trap::handle_ecall(&mut frame, &self.syscalls, &mut ctx);
            riscv::wait_for_interrupt();
        }
    }
}

/// Kernel main invoked after boot assembly completed.
pub fn kmain() -> ! {
    uart::write_line("C: entering kmain");
    #[cfg(feature = "boot_timing")]
    let t0 = crate::arch::riscv::read_time();
    let mut kernel = KernelState::new();
    #[cfg(feature = "boot_timing")]
    {
        let t1 = crate::arch::riscv::read_time();
        let delta = (t1 - t0) as u64;
        use core::fmt::Write as _;
        let mut u = crate::uart::KernelUart::lock();
        let _ = write!(u, "T:init={}\n", delta);
    }
    uart::write_line("D: after KernelState::new");
    kernel.banner();
    // reduce IO noise during timing runs
    // SAFETY: trap vector installed; first tick armed; enable S-mode timer interrupts after init
    unsafe {
        crate::trap::enable_timer_interrupts();
    }
    uart::write_line("T: enabled timer interrupts");
    uart::write_line("F: before exercise_ipc");
    kernel.exercise_ipc();
    uart::write_line("G: after exercise_ipc");
    {
        use core::fmt::Write as _;
        let mut w = crate::uart::raw_writer();
        let _ = write!(w, "H: before selftest\n");
    }
    // Quick sanity for OpenSBI environment
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        use core::fmt::Write as _;
        let mut w = crate::uart::raw_writer();
        let _ = write!(w, "ENV: sbi present\n");
    }
    #[cfg(feature = "boot_timing")]
    let t2 = crate::arch::riscv::read_time();
    {
        let mut ctx = selftest::Context {
            hal: &kernel.hal,
            router: &mut kernel.ipc,
            address_space: &mut kernel.address_space,
            tasks: &mut kernel.tasks,
            scheduler: &mut kernel.scheduler,
        };
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
    uart::write_line("I: after selftest");

    // End of kernel bring-up; user-mode services are responsible for
    // emitting their own readiness markers.

    kernel.idle_loop()
}
