// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot wrapper for the NEURON kernel. Provides a minimal `_start` entry
//! point that performs the early machine setup before handing execution to
//! the kernel library via `neuron::kmain()`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
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

/// `_start` sets only `sp` and `gp` before jumping here, so the firmware's
/// registers survive as the C-ABI arguments: `a0` = boot hart id, `a1` = the
/// device tree (RFC-0098 C1 — the one hardware truth).
#[no_mangle]
pub extern "C" fn start_rust(hartid: usize, dtb: usize) -> ! {
    // Trimmed early diagnostics for stable, short logs.
    // SAFETY: Early boot runs before the Rust runtime. The kernel guarantees
    // that only a single core executes this path, so calling the raw
    // initialisation routine is sound here.
    // The kernel builds its console from the tree before its first log line
    // (RFC-0098 C3); this wrapper prints nothing itself — it knows no address.
    unsafe { neuron::early_boot_init(hartid, dtb) };
    kmain()
}
