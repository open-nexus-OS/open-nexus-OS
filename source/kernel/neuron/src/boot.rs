// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Early boot routines for the NEURON microkernel.

use crate::uart;

#[cfg(not(test))]
extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

/// Perform the machine initialisation required before the kernel can run.
///
/// # Safety
///
/// This must only be invoked once on the boot CPU before any Rust code that
/// relies on initialised memory or traps executes. Callers must ensure the
/// stack is valid and interrupts are masked until setup completes.
pub fn early_boot_init() {
    // SAFETY: called once during early boot, before interrupts/threads.
    unsafe {
        zero_bss();
    }
    uart::write_line("boot: ok");

    // SAFETY: privileged context, trap vector install once.
    unsafe {
        crate::trap::install_trap_vector();
        // Arm first tick; enabling SIE is deferred to kmain for safer sequencing
        crate::trap::timer_arm(crate::trap::DEFAULT_TICK_CYCLES);
    }
    uart::write_line("traps: ok");

    uart::write_line("A: before heap init");
    crate::init_heap();
    uart::write_line("B: after heap init");
    uart::write_line("boot: returning to wrapper");
}

unsafe fn zero_bss() {
    #[cfg(not(test))]
    {
        crate::arch::riscv::clear_bss(
            core::ptr::addr_of_mut!(__bss_start),
            core::ptr::addr_of_mut!(__bss_end),
        );
    }
}
