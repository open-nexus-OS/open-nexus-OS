//! RISC-V architecture hooks for NEURON.

pub fn init() {
    // Early init hooks such as paging and interrupt setup will live here.
}

pub fn wait_for_interrupt() {
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
    }
}
