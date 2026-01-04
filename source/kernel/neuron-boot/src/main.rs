// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Boot wrapper for the NEURON kernel. Provides a minimal `_start` entry
//! point that performs the early machine setup before handing execution to
//! the kernel library via `neuron::kmain()`.
#![no_std]
#![no_main]

use neuron::kmain;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(
    r#"
    .section .text._start, "ax", @progbits
    .globl _start
    .align 4
_start:
    la   sp, __stack_top
    /* RISC-V ABI: initialize gp for small-data accesses (Rust may rely on it).
     * Use PC-relative addressing (kernel is linked above 2GiB). */
    .option push
    .option norelax
    la   gp, __global_pointer$
    .option pop
    j    start_rust
"#
);

#[inline]
fn uart_write_line(msg: &str) {
    const UART0_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;
    unsafe {
        for &b in msg.as_bytes() {
            while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0
            {
            }
            core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b);
            if b == b'\n' {
                while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE
                    == 0
                {}
                core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b'\r');
            }
        }
        while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b'\n');
    }
}

#[no_mangle]
pub extern "C" fn start_rust() -> ! {
    // Trimmed early diagnostics for stable, short logs.
    // SAFETY: Early boot runs before the Rust runtime. The kernel guarantees
    // that only a single core executes this path, so calling the raw
    // initialisation routine is sound here.
    unsafe { neuron::early_boot_init() };
    uart_write_line("W0: calling kmain");
    kmain()
}
