// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
    forbid(unsafe_code)
)]
#![deny(clippy::all, missing_docs)]

//! Tiny collection of executable payloads used by execd while the ELF/NXB
//! loaders are under construction.

/// Embedded ELF images bundled with the OS build.
pub mod hello_elf;

use core::hint::spin_loop;

/// Bootstrap message delivered to the child task after [`nexus_abi::spawn`]
/// succeeds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BootstrapMsg {
    /// Number of arguments supplied to the child. Zero for the MVP path.
    pub argc: u32,
    /// Pointer to the argv table in the child's address space. Zero for the MVP.
    pub argv_ptr: u64,
    /// Pointer to the environment table in the child's address space. Zero for the MVP.
    pub env_ptr: u64,
    /// Capability handle for the initial endpoint granted to the child.
    pub cap_seed_ep: u32,
    /// Reserved for future expansion.
    pub flags: u32,
}

/// Entry point invoked by the kernel once the child task is scheduled.
pub extern "C" fn hello_child_entry(bootstrap: *const BootstrapMsg) -> ! {
    let _ = read_bootstrap_once(bootstrap);
    log_line("child: hello");

    loop {
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        {
            if nexus_abi::yield_().is_err() {
                spin_loop();
            }
        }
        #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
        {
            spin_loop();
        }
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn read_bootstrap_once(ptr: *const BootstrapMsg) -> Option<BootstrapMsg> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the kernel passes a valid pointer to an immutable BootstrapMsg.
        unsafe { ptr.as_ref().copied() }
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn read_bootstrap_once(_ptr: *const BootstrapMsg) -> Option<BootstrapMsg> {
    None
}

fn log_line(line: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    uart_write_line(line);

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    let _ = line;
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn uart_write_line(line: &str) {
    for byte in line.bytes() {
        uart_write_byte(byte);
    }
    uart_write_byte(b'\n');
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn uart_write_byte(byte: u8) {
    const UART0_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;

    // SAFETY: interacts with the QEMU virt UART MMIO region; only invoked on OS builds.
    unsafe {
        while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        if byte == b'\n' {
            core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b'\r');
            while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0
            {
            }
        }
        core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, byte);
    }
}
