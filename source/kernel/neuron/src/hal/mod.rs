// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Hardware abstraction layer traits.

pub mod virt;

/// Abstraction for a monotonic timer.
pub trait Timer {
    /// Returns the current time in nanoseconds.
    fn now(&self) -> u64;
    /// Programs the next wake-up time in nanoseconds.
    fn set_wakeup(&self, deadline: u64);
}

/// UART abstraction used for kernel logging.
#[allow(dead_code)]
pub trait Uart {
    /// Writes a single byte to the UART.
    fn write_byte(&self, byte: u8);
}

/// Minimal MMIO accessor.
#[allow(dead_code)]
pub trait Mmio {
    /// Writes a 32-bit value to the device.
    unsafe fn write32(&self, offset: usize, value: u32);
    /// Reads a 32-bit value from the device.
    unsafe fn read32(&self, offset: usize) -> u32;
}

/// Interrupt controller primitive.
#[allow(dead_code)]
pub trait IrqCtl {
    /// Enables the interrupt line.
    fn enable(&self, irq: usize);
    /// Disables the interrupt line.
    fn disable(&self, irq: usize);
}

/// TLB management operations.
#[allow(dead_code)]
pub trait Tlb {
    /// Flushes the entire translation cache.
    fn flush_all(&self);
}

/// Page table backing store abstraction.
#[allow(dead_code)]
pub trait PageTable {
    /// Returns the SATP value representing the root page table.
    fn satp(&self) -> usize;
}
