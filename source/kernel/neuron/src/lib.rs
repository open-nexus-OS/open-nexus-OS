// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! NEURON kernel library – no binary entry here.

#![cfg_attr(not(test), no_std)]
#![deny(warnings)]
#![deny(unsafe_op_in_unsafe_fn)] // deny instead of forbid to allow naked functions

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, addr_of_mut};
use linked_list_allocator::Heap;
use spin::Mutex;

// Global allocator using spin::Mutex instead of lock_api to avoid HPM CSR access

const HEAP_SIZE: usize = 64 * 1024; // 64KB instead of 1MB

#[cfg_attr(not(test), link_section = ".bss.heap")]
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

struct SpinLockedHeap(Mutex<Heap>);

unsafe impl GlobalAlloc for SpinLockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: allocate_first_fit is safe to call; caller guarantees layout validity per GlobalAlloc contract
        self.0
            .lock()
            .allocate_first_fit(layout)
            .ok()
            .map_or(ptr::null_mut(), |allocation| allocation.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Caller guarantees per GlobalAlloc contract that:
        // - ptr was allocated by this allocator with the same layout
        // - ptr has not been deallocated yet
        // - ptr is non-null (required by GlobalAlloc::dealloc precondition)
        unsafe {
            let non_null = ptr::NonNull::new_unchecked(ptr);
            self.0.lock().deallocate(non_null, layout);
        }
    }
}

#[cfg_attr(all(not(test), target_os = "none"), global_allocator)]
static ALLOC: SpinLockedHeap = SpinLockedHeap(Mutex::new(Heap::empty()));

fn init_heap() {
    uart::write_line("B1: entering init_heap");
    // SAFETY: single-threaded early boot; we only pass a raw pointer + length.
    unsafe {
        uart::write_line("B2: getting heap pointer");
        let start: *mut u8 = addr_of_mut!(HEAP) as *mut u8;
        uart::write_line("B3: locking allocator");
        let mut alloc = ALLOC.0.lock();
        uart::write_line("B4: calling init");
        alloc.init(start, HEAP_SIZE);
        uart::write_line("B5: heap initialized");
    }
    uart::write_line("B6: leaving init_heap");
}

// Modules

mod arch;
mod boot;
mod bootstrap;
mod cap;
mod determinism;
mod hal;
mod ipc;
mod kmain;
mod mm;
mod sched;
mod selftest;
mod syscall;
mod task;
mod trap;
mod uart;

pub use bootstrap::BootstrapMsg;
pub use task::{Pid, TaskTable, TransferError};
// compile the kernel panic handler automatically for no_std targets (OS = "none")
#[cfg(all(not(test), target_os = "none"))]
mod panic;

// Constants

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
    uart::write_line("K0: entering lib::kmain");
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
