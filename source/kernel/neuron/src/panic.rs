// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unified panic handler emitting deterministic diagnostics over UART
//! OWNERS: @kernel-team
//! PUBLIC API: panic handler (no_std)
//! DEPENDS_ON: trap::last_trap(), uart::raw_writer()
//! INVARIANTS: Minimal formatting; no allocations; stable output fields
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use core::{fmt::Write, panic::PanicInfo};

use crate::{trap, uart};

/// Emits a panic message including source location and the last trap frame.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut w = uart::raw_writer();

    // CRITICAL: Use minimal formatting to avoid triggering another panic
    let _ = w.write_str("\nPANIC: ");
    if let Some(location) = info.location() {
        let _ = w.write_str(location.file());
        let _ = w.write_str(":");
        // Line number without formatting
        let line = location.line();
        crate::trap::uart_write_hex(&mut w, line as usize);
        let _ = w.write_str(": ");
    }
    // Message - try to write it, but be prepared for failure
    if let Some(msg_str) = info.message().as_str() {
        let _ = w.write_str(msg_str);
    } else {
        let _ = w.write_str("<complex msg>");
    }
    let _ = w.write_str("\n");

    // Show current PC (return address) to identify where panic originated
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let ra: usize;
        unsafe { core::arch::asm!("mv {}, ra", out(reg) ra) };
        let _ = w.write_str("PANIC ra=0x");
        crate::trap::uart_write_hex(&mut w, ra);
        let _ = w.write_str("\n");
    }

    if let Some(frame) = trap::last_trap() {
        let _ = w.write_str("PANIC: last trap: sepc=0x");
        crate::trap::uart_write_hex(&mut w, frame.sepc);
        let _ = w.write_str(" scause=0x");
        crate::trap::uart_write_hex(&mut w, frame.scause);
        let _ = w.write_str(" stval=0x");
        crate::trap::uart_write_hex(&mut w, frame.stval);
        let _ = w.write_str("\n");
    }

    drop(w);

    loop {
        crate::arch::riscv::wait_for_interrupt();
    }
}
