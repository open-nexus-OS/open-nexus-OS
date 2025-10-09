// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal UART support for boot diagnostics.

use core::fmt::{self, Write};
use spin::Mutex;

/// Address of the first UART on the `virt` machine.
const UART0_BASE: usize = 0x1000_0000;
const UART_TX: usize = 0x0;
const UART_LSR: usize = 0x5;
const LSR_TX_IDLE: u8 = 1 << 5;

/// Global UART writer used for boot logs.
static UART0: Mutex<KernelUart> = Mutex::new(KernelUart::new(UART0_BASE));

/// UART implementation capable of formatted writes.
#[derive(Clone, Copy)]
pub struct KernelUart {
    base: usize,
}

impl KernelUart {
    /// Creates a UART abstraction rooted at `base`.
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    /// Returns a guard for the boot UART singleton.
    pub fn lock() -> spin::MutexGuard<'static, KernelUart> {
        UART0.lock()
    }

    fn write_raw(&self, offset: usize, value: u8) {
        let addr = (self.base + offset) as *mut u8;
        unsafe {
            while core::ptr::read_volatile((self.base + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {
            }
            core::ptr::write_volatile(addr, value);
        }
    }
}

// Raw, lock-free UART emission for trap/panic contexts where the mutex may already be held.
#[inline]
fn write_raw_mmio(offset: usize, value: u8) {
    let addr = (UART0_BASE + offset) as *mut u8;
    unsafe {
        while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        core::ptr::write_volatile(addr, value);
    }
}

pub struct RawUart;

impl Write for RawUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            if byte == b'\n' {
                write_raw_mmio(UART_TX, b'\r');
            }
            write_raw_mmio(UART_TX, byte);
        }
        Ok(())
    }
}

pub fn raw_writer() -> RawUart {
    RawUart
}

impl Write for KernelUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            if byte == b'\n' {
                self.write_raw(UART_TX, b'\r');
            }
            self.write_raw(UART_TX, byte);
        }
        Ok(())
    }
}

/// Writes the provided string via the global UART.
pub fn write_str(message: &str) {
    let mut uart = KernelUart::lock();
    let _ = uart.write_str(message);
}

/// Writes a line terminated by `\n` to the UART.
pub fn write_line(message: &str) {
    write_str(message);
    write_str("\n");
}
