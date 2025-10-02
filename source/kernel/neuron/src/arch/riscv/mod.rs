// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RISC-V specific helpers used across the NEURON kernel.
//!
//! The implementation follows the Sv39 privileged specification and is
//! written such that host builds can still exercise high level logic via
//! the lightweight `#[cfg(not(target_arch = "riscv64"))]` stubs.

/// Clears the `.bss` region defined by the linker.
#[inline]
pub fn clear_bss(start: *mut u8, end: *mut u8) {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        let mut ptr = start;
        while ptr < end {
            core::ptr::write_volatile(ptr, 0);
            ptr = ptr.add(1);
        }
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let len = end as usize - start as usize;
        let slice = unsafe { core::slice::from_raw_parts_mut(start, len) };
        for byte in slice {
            *byte = 0;
        }
    }
}

/// Installs the trap vector address for supervisor mode.
#[inline]
pub fn configure_traps(trap_vector: usize) {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("csrw stvec, {0}", in(reg) trap_vector, options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = trap_vector;
    }
}

/// Enables supervisor timer interrupts.
#[inline]
pub fn enable_timer_interrupts() {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        const STIE: usize = 1 << 5;
        core::arch::asm!(
            "csrr t0, sie", "ori t0, t0, {stie}", "csrw sie, t0",
            stie = const STIE,
            out("t0") _,
            options(nostack)
        );
    }
}

/// Reads the timer CSR (nsec on virt is based on a 10 MHz counter).
#[inline]
pub fn read_time() -> u64 {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        let value: u64;
        core::arch::asm!("csrr {0}, time", out(reg) value, options(nomem, nostack, preserves_flags));
        value
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        0
    }
}

/// Programs the CLINT timer compare register.
#[inline]
pub fn set_timer(deadline: u64) {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        const CLINT_BASE: usize = 0x0200_0000;
        const MTIMECMP: *mut u64 = (CLINT_BASE + 0x4000) as *mut u64;
        core::ptr::write_volatile(MTIMECMP, deadline);
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = deadline;
    }
}

/// Issues a WFI instruction or yields on the host.
#[inline]
pub fn wait_for_interrupt() {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        core::hint::spin_loop();
    }
}
