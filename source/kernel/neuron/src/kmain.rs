// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel main routine responsible for subsystem bring-up.

use crate::{
    arch::riscv,
    cap::{CapTable, Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    ipc::{self, header::MessageHeader},
    mm::PageTable,
    sched::{QosClass, Scheduler},
    syscall::{self, api, SyscallTable},
    uart,
    BANNER,
};

/// Aggregated kernel state initialised during boot.
struct KernelState {
    hal: VirtMachine,
    scheduler: Scheduler,
    caps: CapTable,
    ipc: ipc::Router,
    address_space: PageTable,
    syscalls: SyscallTable,
}

impl KernelState {
    fn new() -> Self {
        let mut caps = CapTable::new();
        // Slot 0: bootstrap endpoint loopback
        let _ = caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        );
        // Slot 1: identity VMO for bootstrap mappings
        let _ = caps.set(
            1,
            Capability { kind: CapabilityKind::Vmo { base: 0x8000_0000, len: 0x10_0000 }, rights: Rights::MAP },
        );

        let mut scheduler = Scheduler::new();
        scheduler.enqueue(0, QosClass::Idle);

        let mut syscalls = SyscallTable::new();
        api::install_handlers(&mut syscalls);

        Self {
            hal: VirtMachine::new(),
            scheduler,
            caps,
            ipc: ipc::Router::new(4),
            address_space: PageTable::new(),
            syscalls,
        }
    }

    fn banner(&self) {
        uart::write_line(BANNER);
    }

    fn exercise_ipc(&mut self) {
        // Send a bootstrap message to prove IPC wiring works before tasks run.
        let header = MessageHeader::new(0, 0, 0x100, 0, 0);
        let _ = self.ipc.send(0, ipc::Message::new(header, alloc::vec::Vec::new()));
    }

    fn idle_loop(&mut self) -> ! {
        loop {
            let mut ctx = api::Context::new(
                &mut self.scheduler,
                &mut self.caps,
                &mut self.ipc,
                &mut self.address_space,
                self.hal.timer(),
            );
            let mut frame = crate::trap::TrapFrame::default();
            frame.a[7] = syscall::SYSCALL_YIELD;
            crate::trap::handle_ecall(&mut frame, &self.syscalls, &mut ctx);
            riscv::wait_for_interrupt();
        }
    }
}

/// Kernel main invoked after boot assembly completed.
pub fn kmain() -> ! {
    let mut kernel = KernelState::new();
    kernel.banner();
    kernel.exercise_ipc();
    kernel.idle_loop()
}
