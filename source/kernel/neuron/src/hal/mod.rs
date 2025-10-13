// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Hardware abstraction layer traits.

pub mod virt;

/// Abstraction for a monotonic timer.
pub trait Timer {
    /// Returns the current time in nanoseconds.
    fn now(&self) -> u64;
    /// Programs the next wake-up time in nanoseconds.
    #[allow(dead_code)]
    fn set_wakeup(&self, deadline: u64);
}

/// UART abstraction used for kernel logging.
pub trait Uart {
    /// Writes a single byte to the UART.
    // Host builds don't exercise UART MMIO; keep the HAL contract without forcing usage.
    #[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
    fn write_byte(&self, byte: u8);
}

/// Interrupt controller primitive.
pub trait IrqCtl {
    /// Enables the interrupt line.
    fn enable(&self, irq: usize);
    /// Disables the interrupt line.
    fn disable(&self, irq: usize);
}

/// TLB management operations.
pub trait Tlb {
    /// Flushes the entire translation cache.
    fn flush_all(&self);
}
