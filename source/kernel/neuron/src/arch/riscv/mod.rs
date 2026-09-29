// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RISC-V specific helpers used across the NEURON kernel
//! OWNERS: @kernel-arch-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! PUBLIC API: read_pc(), clear_bss(), read_time(), set_timer()
//! DEPENDS_ON: core arch asm, optional CLINT/SBI
//! INVARIANTS: Host stubs exist; OS path uses inline asm guarded by cfg
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
//!
//! The implementation follows the Sv39 privileged specification and is
//! written such that host builds can still exercise high level logic via
//! the lightweight `#[cfg(not(target_arch = "riscv64"))]` stubs.
#![cfg_attr(any(test, not(target_arch = "riscv64")), allow(dead_code))]

use crate::types::HartId;

/// Returns the current program counter.
#[inline]
#[allow(dead_code)]
pub fn read_pc() -> usize {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        let value: usize;
        core::arch::asm!(
            "auipc {tmp}, 0",
            tmp = out(reg) value,
            options(nomem, nostack, preserves_flags),
        );
        value
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        0
    }
}

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

// Legacy trap/timer functions removed - now handled in trap.rs with SBI

/// Reads the `time` CSR, in ticks of the platform's timebase (`hal::platform::ticks_to_ns`
/// converts; 10 MHz on QEMU virt, 24 MHz on the board).
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

/// Sets `senvcfg` (0x10a, by number like `stimecmp`) to `(old & !clear) | set` —
/// the user-mode environment of this hart (RFC-0098 C4: Zicbom for drivers).
#[inline]
pub fn update_senvcfg(clear: u64, set: u64) {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        let old: u64;
        core::arch::asm!("csrr {0}, 0x10a", out(reg) old, options(nomem, nostack));
        core::arch::asm!("csrw 0x10a, {0}", in(reg) (old & !clear) | set, options(nomem, nostack));
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = (clear, set);
    }
}

/// Writes a CSR by number — `stimecmp` (Sstc, 0x14d) is younger than the pinned
/// assembler's mnemonic table. S-mode arms its own timer here; the CLINT is
/// M-mode's and is never touched from the kernel (RFC-0098 C3).
#[inline]
pub fn write_csr_stimecmp(csr: u16, value: u64) {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        debug_assert_eq!(csr, 0x14d);
        let _ = csr;
        core::arch::asm!("csrw 0x14d, {0}", in(reg) value, options(nomem, nostack));
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        let _ = (csr, value);
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

/// Returns the current stack pointer.
#[inline]
#[allow(dead_code)]
pub fn read_sp() -> usize {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        let value: usize;
        core::arch::asm!("mv {o}, sp", o = out(reg) value, options(nomem, nostack, preserves_flags));
        value
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        0
    }
}

/// Reads the currently executing hart id (`mhartid` CSR).
#[inline]
#[allow(dead_code)]
pub fn read_mhartid() -> HartId {
    #[cfg(target_arch = "riscv64")]
    unsafe {
        let value: usize;
        core::arch::asm!("csrr {0}, mhartid", out(reg) value, options(nomem, nostack, preserves_flags));
        HartId::from_raw(value as u16)
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        HartId::from_raw(0)
    }
}
