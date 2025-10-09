// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! HAL implementation targeting QEMU's `virt` machine.

use core::ptr::{read_volatile, write_volatile};

use crate::arch::riscv;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

use super::{IrqCtl, Timer, Tlb, Uart};

const UART0_BASE: usize = 0x1000_0000;
const UART_TX: usize = 0x0;
const UART_LSR: usize = 0x5;
const LSR_TX_IDLE: u8 = 1 << 5;

/// Collection of HAL devices for the virt machine.
pub struct VirtMachine {
    timer: VirtTimer,
    uart: VirtUart,
    tlb: VirtTlb,
    irq: VirtIrq,
}

impl VirtMachine {
    /// Constructs the HAL facade.
    pub const fn new() -> Self {
        Self { timer: VirtTimer, uart: VirtUart, tlb: VirtTlb, irq: VirtIrq }
    }

    /// Returns a reference to the timer implementation.
    pub const fn timer(&self) -> &VirtTimer {
        &self.timer
    }

    /// Returns a reference to the UART implementation.
    pub const fn uart(&self) -> &VirtUart {
        &self.uart
    }

    /// Returns a reference to the TLB helper.
    pub const fn tlb(&self) -> &VirtTlb {
        &self.tlb
    }

    /// Returns a reference to the IRQ controller helper.
    pub const fn irq(&self) -> &VirtIrq {
        &self.irq
    }
}

/// Virt specific timer mapped to the `time` CSR.
pub struct VirtTimer;

impl Timer for VirtTimer {
    fn now(&self) -> u64 {
        // QEMU models a 10 MHz clock. Convert ticks to nanoseconds.
        const TICK_NS: u64 = 100; // 10 MHz -> 100 ns per tick
        riscv::read_time() * TICK_NS
    }

    fn set_wakeup(&self, deadline: u64) {
        const TICK_NS: u64 = 100;
        let ticks = deadline / TICK_NS;
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            // Program mtimer via SBI for S-mode compatibility
            sbi::set_timer(ticks);
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = ticks;
        }
    }
}

/// Memory mapped UART implementation.
pub struct VirtUart;

impl Uart for VirtUart {
    fn write_byte(&self, byte: u8) {
        unsafe {
            while read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
            write_volatile((UART0_BASE + UART_TX) as *mut u8, byte);
        }
    }
}

/// Trivial IRQ controller wrapper.
pub struct VirtIrq;

impl IrqCtl for VirtIrq {
    fn enable(&self, _irq: usize) {}
    fn disable(&self, _irq: usize) {}
}

/// Sv39 TLB helper issuing `sfence.vma` when compiled for RISC-V.
pub struct VirtTlb;

impl Tlb for VirtTlb {
    fn flush_all(&self) {
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("sfence.vma x0, x0", options(nostack));
        }
    }
}
