// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unified panic handler emitting deterministic diagnostics over UART
//! OWNERS: @kernel-team
//! PUBLIC API: panic handler (no_std)
//! DEPENDS_ON: trap::last_trap(), uart::raw_writer()
//! INVARIANTS: Minimal formatting; no allocations; stable output fields
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use core::{fmt, fmt::Write, panic::PanicInfo};

use crate::{trap, uart};

/// Emits a panic message including source location and the last trap frame.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut w = uart::raw_writer();
    let msg = info.message(); // PanicMessage implements Display
    if let Some(location) = info.location() {
        let _ = writeln!(w, "PANIC {}:{}: {}", location.file(), location.line(), msg);
    } else {
        let _ = writeln!(w, "PANIC: {}", msg);
    }

    if let Some(frame) = trap::last_trap() {
        let _ = writeln!(w, "PANIC: trap context:");
        // Adapter: fmt_trap expects Formatter; we output via Display wrapper to UART.
        struct TrapFmt<'a>(&'a trap::TrapFrame);
        impl fmt::Display for TrapFmt<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                trap::fmt_trap(self.0, f)
            }
        }
        let _ = writeln!(w, "{}", TrapFmt(&frame));
    }

    // Emit a compact dump of the trap ring (latest 8 entries) if available.
    // We avoid heavy formatting; print only sepc/scause/stval.
    let _ = writeln!(w, "PANIC: last traps:");
    for _i in 0..8 {
        if let Some(tf) = crate::trap::last_trap() {
            let _ = writeln!(
                w,
                " sepc=0x{:x} scause=0x{:x} stval=0x{:x}",
                tf.sepc, tf.scause, tf.stval
            );
        }
    }

    drop(w);

    loop {
        crate::arch::riscv::wait_for_interrupt();
    }
}
