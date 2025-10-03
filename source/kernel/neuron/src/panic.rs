// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Unified panic handler emitting deterministic diagnostics over UART.

use core::{fmt::Write, panic::PanicInfo};

use crate::{trap, uart::KernelUart};

/// Emits a panic message including source location and the last trap frame.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut uart = KernelUart::lock();
    match (info.location(), info.message()) {
        (Some(location), Some(message)) => {
            let _ = writeln!(uart, "PANIC {}:{}: {message}", location.file(), location.line());
        }
        (Some(location), None) => {
            let _ = writeln!(uart, "PANIC {}:{}", location.file(), location.line());
        }
        (None, Some(message)) => {
            let _ = writeln!(uart, "PANIC: {message}");
        }
        (None, None) => {
            let _ = writeln!(uart, "PANIC");
        }
    }

    if let Some(frame) = trap::last_trap() {
        let _ = writeln!(uart, "PANIC: trap context:");
        let _ = trap::fmt_trap(&frame, &mut uart);
    }

    drop(uart);

    loop {
        crate::arch::riscv::wait_for_interrupt();
    }
}
