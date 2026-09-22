// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel console — formatted writes over the UART the device tree
//! named (`/chosen/stdout-path`, RFC-0098 C3). The register base, stride and
//! width live in `hal::platform`, read per byte from lock-free statics, so the
//! first log line after `init_from_fdt` and the raw path inside a trap handler
//! use the same three numbers. No address is guessed: before the platform is
//! initialised every byte is dropped.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every QEMU marker is a byte through this file

use core::fmt::{self, Write};

#[cfg(not(debug_assertions))]
use spin::Mutex;

#[cfg(debug_assertions)]
type UartLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type UartLock<T> = Mutex<T>;
#[cfg(debug_assertions)]
type UartGuard<'a> = crate::sync::dbg_mutex::DbgMutexGuard<'a, KernelUart>;
#[cfg(not(debug_assertions))]
type UartGuard<'a> = spin::MutexGuard<'a, KernelUart>;

/// Global UART writer used for boot logs. Holds no address: the platform
/// statics are the one place the console's registers are known.
static UART0: UartLock<KernelUart> = UartLock::new(KernelUart);

/// UART implementation capable of formatted writes.
#[derive(Clone, Copy)]
pub struct KernelUart;

impl KernelUart {
    /// Returns a guard for the boot UART singleton.
    pub fn lock() -> UartGuard<'static> {
        UART0.lock()
    }
}

/// Raw, lock-free UART emission for trap/panic contexts where the mutex may
/// already be held.
pub struct RawUart;

impl Write for RawUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            if byte == b'\n' {
                crate::hal::platform::console_write_byte(b'\r');
            }
            crate::hal::platform::console_write_byte(byte);
        }
        Ok(())
    }
}

pub fn raw_writer() -> RawUart {
    RawUart
}

impl Write for KernelUart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        RawUart.write_str(s)
    }
}

/// Writes the provided string via the global UART.
#[allow(dead_code)]
pub fn write_str(message: &str) {
    let mut uart = KernelUart::lock();
    let _ = uart.write_str(message);
}

/// Writes a line terminated by `\n` to the UART.
#[allow(dead_code)]
pub fn write_line(message: &str) {
    write_str(message);
    write_str("\n");
}
