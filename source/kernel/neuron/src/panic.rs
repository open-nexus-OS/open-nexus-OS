// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Unified panic handler emitting deterministic diagnostics over UART.

use core::{fmt, fmt::Write, panic::PanicInfo};

use crate::{trap, uart::KernelUart};

/// Emits a panic message including source location and the last trap frame.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut uart = KernelUart::lock();
    let msg = info.message(); // PanicMessage implements Display
    if let Some(location) = info.location() {
        let _ = writeln!(uart, "PANIC {}:{}: {}", location.file(), location.line(), msg);
    } else {
        let _ = writeln!(uart, "PANIC: {}", msg);
    }

    if let Some(frame) = trap::last_trap() {
        let _ = writeln!(uart, "PANIC: trap context:");
        // Adapter: fmt_trap erwartet Formatter; wir geben über Display-Wrapper auf UART aus.
        struct TrapFmt<'a>(&'a trap::TrapFrame);
        impl fmt::Display for TrapFmt<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                trap::fmt_trap(self.0, f)
            }
        }
        let _ = writeln!(uart, "{}", TrapFmt(&frame));
    }

    drop(uart);

    loop {
        crate::arch::riscv::wait_for_interrupt();
    }
}
