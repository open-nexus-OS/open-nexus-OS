// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Early boot routines for the NEURON microkernel.

use crate::{arch::riscv, kmain};

#[cfg(not(test))]
extern "C" {
    static mut __bss_start: u8;
    static mut __bss_end: u8;
    fn __trap_vector();
}

/// Kernel entry point invoked by the linker script.
#[cfg_attr(not(test), no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        zero_bss();
        init_traps();
    }
    kmain::kmain()
}

unsafe fn zero_bss() {
    #[cfg(not(test))]
    {
        riscv::clear_bss(core::ptr::addr_of_mut!(__bss_start), core::ptr::addr_of_mut!(__bss_end));
    }
}

unsafe fn init_traps() {
    #[cfg(not(test))]
    {
        riscv::configure_traps(__trap_vector as usize);
        riscv::enable_timer_interrupts();
    }
}
