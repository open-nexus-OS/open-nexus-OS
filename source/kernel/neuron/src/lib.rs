// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! NEURON kernel library – binary entry points live in the dedicated
//! `neuron-boot` crate. This library exposes the kernel initialisation
//! routines and runtime used by the boot wrapper.

#![cfg_attr(not(test), no_std)]
#![deny(warnings)]
#![forbid(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use core::ptr::addr_of_mut;
use linked_list_allocator::LockedHeap;

// Global allocator

const HEAP_SIZE: usize = 1024 * 1024;

#[cfg_attr(not(test), link_section = ".bss.heap")]
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[cfg_attr(all(not(test), target_os = "none"), global_allocator)]
static ALLOC: LockedHeap = LockedHeap::empty();

pub fn init_heap() {
    // SAFETY: single-threaded early boot; we only pass a raw pointer + length.
    unsafe {
        let start: *mut u8 = addr_of_mut!(HEAP) as *mut u8;
        ALLOC.lock().init(start, HEAP_SIZE);
    }
}


// Modules

pub mod arch;
pub mod boot;
pub mod cap;
pub mod determinism;
pub mod hal;
pub mod ipc;
pub mod kmain;
pub mod mm;
pub mod sched;
pub mod selftest;
pub mod syscall;
pub mod trap;
pub mod uart;
// compile the kernel panic handler automatically for no_std targets (OS = "none")
#[cfg(all(not(test), target_os = "none"))]
mod panic;

// Constants

pub const BANNER: &str = "NEURON";

/// Perform the low-level machine initialisation required before jumping into
/// the core kernel logic.
///
/// # Safety
///
/// Must be invoked exactly once on the boot CPU before any other kernel code
/// runs. Callers must ensure the stack is valid and interrupts are masked.
pub unsafe fn early_boot_init() {
    boot::early_boot_init();
}

/// Entry point for the kernel runtime. Assumes early boot setup was performed
/// and never returns.
pub fn kmain() -> ! {
    kmain::kmain()
}

// Tests

#[cfg(test)]
mod tests {
    use super::ipc::header::MessageHeader;
    use static_assertions::const_assert_eq;

    #[test]
    fn header_layout() {
        const_assert_eq!(core::mem::size_of::<MessageHeader>(), 16);
    }
}
