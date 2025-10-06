// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! HAL implementation targeting QEMU's `virt` machine.

use core::ptr::{read_volatile, write_volatile};

use crate::arch::riscv;

use super::{IrqCtl, Mmio, Timer, Tlb, Uart};

#[allow(dead_code)]
const UART0_BASE: usize = 0x1000_0000;
#[allow(dead_code)]
const UART_TX: usize = 0x0;
#[allow(dead_code)]
const UART_LSR: usize = 0x5;
#[allow(dead_code)]
const LSR_TX_IDLE: u8 = 1 << 5;

/// Collection of HAL devices for the virt machine.
pub struct VirtMachine {
    timer: VirtTimer,
    #[allow(dead_code)]
    uart: VirtUart,
    #[allow(dead_code)]
    tlb: VirtTlb,
    #[allow(dead_code)]
    irq: VirtIrq,
}

impl VirtMachine {
    /// Constructs the HAL facade.
    pub const fn new() -> Self {
        Self { timer: VirtTimer, uart: VirtUart, tlb: VirtTlb, irq: VirtIrq }
    }

    /// Returns a reference to the timer implementation.
    #[allow(dead_code)]
    pub const fn timer(&self) -> &VirtTimer {
        &self.timer
    }

    /// Returns a reference to the UART implementation.
    #[allow(dead_code)]
    pub const fn uart(&self) -> &VirtUart {
        &self.uart
    }

    /// Returns a reference to the TLB helper.
    #[allow(dead_code)]
    pub const fn tlb(&self) -> &VirtTlb {
        &self.tlb
    }

    /// Returns a reference to the IRQ controller helper.
    #[allow(dead_code)]
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
        riscv::set_timer(ticks);
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

impl Mmio for VirtUart {
    unsafe fn write32(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile((UART0_BASE + offset) as *mut u32, value);
        }
    }

    unsafe fn read32(&self, offset: usize) -> u32 {
        unsafe {
            read_volatile((UART0_BASE + offset) as *const u32)
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
