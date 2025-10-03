// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Early boot routines for the NEURON microkernel.

use crate::{kmain, uart};

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
        uart::write_line("boot: ok");
        init_traps();
        uart::write_line("traps: ok");
    }
    crate::init_heap();
    kmain::kmain()
}

unsafe fn zero_bss() {
    #[cfg(not(test))]
    {
        crate::arch::riscv::clear_bss(core::ptr::addr_of_mut!(__bss_start), core::ptr::addr_of_mut!(__bss_end));
    }
}

unsafe fn init_traps() {
    #[cfg(not(test))]
    {
        crate::arch::riscv::configure_traps(__trap_vector as usize);
        crate::arch::riscv::enable_timer_interrupts();
    }
}
