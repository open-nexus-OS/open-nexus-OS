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
    /* Static PIE (RFC-0098 C6): the link base is 0, so the address this code
     * runs at IS the fixup delta. Walk the R_RISCV_RELATIVE table and add it
     * to every absolute word in the image. `lla` is PC-relative by
     * construction; a0/a1 (hart id, device tree) are never touched. Entries
     * of any other type are skipped here and named by the kernel's image
     * report (`KSELFTEST: kernel image FAIL (reloc type …)`). */
    lla  t0, __image_start
    lla  t1, __rela_dyn_start
    lla  t2, __rela_dyn_end
1:  bgeu t1, t2, 2f
    ld   t3, 0(t1)         /* r_offset */
    ld   t4, 8(t1)         /* r_info: type in the low 32 bits, symbol 0 */
    ld   t5, 16(t1)        /* r_addend */
    addi t1, t1, 24
    li   t6, 3             /* R_RISCV_RELATIVE */
    bne  t4, t6, 1b
    add  t3, t3, t0
    add  t5, t5, t0
    sd   t5, 0(t3)
    j    1b
2:  la   sp, __stack_top
    /* RISC-V ABI: initialize gp for small-data accesses (Rust may rely on it).
     * PC-relative: the image runs wherever it was loaded. */
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
