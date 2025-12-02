// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: NEURON kernel library – module glue and global allocator
//! OWNERS: @kernel-team
//! PUBLIC API: early_boot_init(), kmain(), exported types
//! DEPENDS_ON: linked_list_allocator, sync (debug), arch/hal/mm/ipc/etc.
//! INVARIANTS: Global allocator lock strategy differs in debug vs release; no_std OS builds
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#![cfg_attr(not(test), no_std)]
#![deny(warnings)]
#![deny(unsafe_op_in_unsafe_fn)] // deny instead of forbid to allow naked functions
#![feature(alloc_error_handler)]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use core::ptr::{self, addr_of_mut};
use linked_list_allocator::Heap;
#[cfg(not(debug_assertions))]
use spin::Mutex;

// Global allocator using spin::Mutex instead of lock_api to avoid HPM CSR access

const HEAP_SIZE: usize = 1024 * 1024; // 1 MiB heap for page tables and stacks

#[repr(align(4096))]
struct HeapRegion([u8; HEAP_SIZE]);

#[cfg_attr(not(test), link_section = ".bss.heap")]
static mut HEAP: HeapRegion = HeapRegion([0; HEAP_SIZE]);

#[cfg(debug_assertions)]
type HeapLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type HeapLock<T> = Mutex<T>;

struct SpinLockedHeap(HeapLock<Heap>);

unsafe impl GlobalAlloc for SpinLockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Debug redzones: add header/tail canaries; release builds use raw allocation.
        #[cfg(debug_assertions)]
        {
            #[repr(C)]
            struct Header {
                size: usize,
                align: usize,
                canary: usize,
            }
            const CANARY: usize = 0xC0FFEE_CAFE_BABE_usize;
            let header_size = core::mem::size_of::<Header>();
            let total_size = header_size
                .checked_add(layout.size())
                .and_then(|s| s.checked_add(core::mem::size_of::<usize>()))
                .unwrap_or(0);
            if total_size == 0 {
                log_error!("ALLOC: fail");
                return ptr::null_mut();
            }
            let full_layout =
                Layout::from_size_align(total_size, core::mem::align_of::<Header>()).unwrap();
            let mut alloc = self.0.lock();
            let base = match alloc.allocate_first_fit(full_layout) {
                Ok(b) => b.as_ptr(),
                Err(_) => ptr::null_mut(),
            };
            if base.is_null() {
                log_error!("ALLOC: fail");
                return base;
            }
            // Write header and tail canary
            let h = base as *mut Header;
            unsafe {
                (*h).size = layout.size();
                (*h).align = layout.align();
                (*h).canary = CANARY;
                // tail canary stored just after payload
                let tail = base.add(header_size + layout.size()) as *mut usize;
                ptr::write_volatile(tail, CANARY);
            }
            let user_ptr = unsafe { base.add(header_size) };
            user_ptr
        }
        #[cfg(not(debug_assertions))]
        {
            // SAFETY: allocate_first_fit is safe to call; caller guarantees layout validity per GlobalAlloc contract
            let mut alloc = self.0.lock();
            let result = alloc
                .allocate_first_fit(layout)
                .ok()
                .map_or(ptr::null_mut(), |allocation| allocation.as_ptr());
            result
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(debug_assertions)]
        {
            if ptr.is_null() {
                return;
            }
            #[repr(C)]
            struct Header {
                size: usize,
                align: usize,
                canary: usize,
            }
            const CANARY: usize = 0xC0FFEE_CAFE_BABE_usize;
            let header_size = core::mem::size_of::<Header>();
            let base = unsafe { ptr.sub(header_size) };
            let h = base as *const Header;
            let (size, _align, canary) = unsafe { ((*h).size, (*h).align, (*h).canary) };
            if canary != CANARY {
                log_error!("HEAP: header canary corrupt");
                panic!("heap header canary");
            }
            let tail = base.wrapping_add(header_size + size) as *const usize;
            let tail_canary = unsafe { ptr::read_volatile(tail) };
            if tail_canary != CANARY {
                log_error!("HEAP: tail canary corrupt");
                panic!("heap tail canary");
            }
            // Poison payload
            for off in 0..size {
                unsafe {
                    ptr::write_volatile(ptr.add(off), 0xA5);
                }
            }
            // Free backing allocation with expanded layout
            let _ = layout; // silence unused in debug path
            let full_size = header_size + size + core::mem::size_of::<usize>();
            let _full_layout =
                Layout::from_size_align(full_size, core::mem::align_of::<Header>()).unwrap();
            // Quarantine: hold a few recently freed blocks before returning to allocator
            #[cfg(debug_assertions)]
            {
                #[derive(Copy, Clone)]
                struct QEntry {
                    base: *mut u8,
                    size: usize,
                    align: usize,
                }
                const QCAP: usize = 8;
                static mut Q_ENTRIES: [Option<QEntry>; QCAP] = [None; QCAP];
                static mut Q_INDEX: usize = 0;
                // evict oldest if occupied
                unsafe {
                    if let Some(ev) = Q_ENTRIES[Q_INDEX].take() {
                        let ev_layout = Layout::from_size_align(ev.size, ev.align).unwrap();
                        let ev_nonnull = NonNull::new_unchecked(ev.base);
                        self.0.lock().deallocate(ev_nonnull, ev_layout);
                    }
                    Q_ENTRIES[Q_INDEX] = Some(QEntry {
                        base,
                        size: full_size,
                        align: core::mem::align_of::<Header>(),
                    });
                    Q_INDEX = (Q_INDEX + 1) % QCAP;
                }
            }
        }
        #[cfg(not(debug_assertions))]
        {
            // SAFETY: Caller guarantees per GlobalAlloc contract that ptr/layout match a prior alloc.
            let non_null = unsafe { NonNull::new_unchecked(ptr) };
            unsafe { self.0.lock().deallocate(non_null, layout) };
        }
    }
}

#[cfg_attr(all(not(test), target_os = "none"), global_allocator)]
static ALLOC: SpinLockedHeap = SpinLockedHeap(HeapLock::new(Heap::empty()));

fn init_heap() {
    // SAFETY: single-threaded early boot; we only pass a raw pointer + length.
    unsafe {
        let start: *mut u8 = addr_of_mut!(HEAP.0) as *mut u8;
        let mut alloc = ALLOC.0.lock();
        alloc.init(start, HEAP_SIZE);
    }
}

/// Alloc error handler - catches allocation failures and provides diagnostic info
#[cfg(all(not(test), target_os = "none"))]
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    // CRITICAL: Use only raw UART, no allocation allowed here!
    use core::fmt::Write;
    let mut u = crate::uart::raw_writer();
    let _ = u.write_str("\n!!! ALLOC ERROR !!!\n");
    let _ = u.write_str("size=");
    crate::trap::uart_write_hex(&mut u, layout.size());
    let _ = u.write_str(" align=");
    crate::trap::uart_write_hex(&mut u, layout.align());

    // Show current SATP to identify which AS we're in
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let satp = riscv::register::satp::read().bits();
        let _ = u.write_str("\nsatp=");
        crate::trap::uart_write_hex(&mut u, satp);
        let mode = (satp >> 60) & 0xF;
        let asid = (satp >> 44) & 0xFFFF;
        let _ = u.write_str(" mode=");
        crate::trap::uart_write_hex(&mut u, mode);
        let _ = u.write_str(" asid=");
        crate::trap::uart_write_hex(&mut u, asid);
    }

    // Show heap address to verify it's accessible
    let heap_start = unsafe { addr_of_mut!(HEAP.0) as usize };
    let _ = u.write_str("\nheap_start=");
    crate::trap::uart_write_hex(&mut u, heap_start);

    let _ = u.write_str("\n");
    panic!("ALLOC-FAIL");
}

// Modules

#[macro_use]
mod log;
mod arch;
mod boot;
mod bootstrap;
mod cap;
mod determinism;
mod hal;
mod ipc;
mod kmain;
mod liveness;
mod mm;
mod satp;
mod sched;
mod selftest;
#[cfg(debug_assertions)]
pub mod sync;
mod syscall;
mod task;
mod trap;
mod types;
mod uart;

pub use bootstrap::BootstrapMsg;
pub use log::Level as LogLevel;
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
