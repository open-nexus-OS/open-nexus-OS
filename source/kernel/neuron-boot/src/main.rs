// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot wrapper for the NEURON kernel. Provides a minimal `_start` entry
//! point that performs the early machine setup before handing execution to
//! the kernel library via `neuron::kmain()`. The image is a static PIE that
//! ends up in the Sv39 high half (RFC-0098 C4/C6): `_start` fixes the image up
//! at its load address, lets the kernel build the boot table there, writes
//! `satp`, jumps to the high alias, fixes the image up a second time with the
//! high base (`R_RISCV_RELATIVE` is `addend + base`: idempotent), and only then
//! enters the kernel proper.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: QEMU selftests + boot markers
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
#![no_std]
#![no_main]

use neuron::kmain;

/// Where physical memory is aliased in the kernel half (RFC-0098 C4).
const PHYS_OFFSET: usize = neuron::phys::PHYS_OFFSET;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(
    r#"
    .section .text._start, "ax", @progbits
    .globl _start
    .align 4
    /* Walk the R_RISCV_RELATIVE table with t0 = the image base to fix up
     * against. `lla` is PC-relative by construction; a0/a1 (hart id, device
     * tree) are never touched. Entries of any other type are skipped here and
     * named by the kernel's image report (`KSELFTEST: kernel image FAIL`). */
    .macro fixup
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
2:
    .endm
_start:
    /* Static PIE (RFC-0098 C6): the link base is 0, so the address this code
     * runs at IS the first fixup delta. */
    lla  t0, __image_start
    fixup
    lla  sp, __stack_top
    /* RISC-V ABI: initialize gp for small-data accesses (Rust may rely on it).
     * PC-relative: the image runs wherever it was loaded. */
    .option push
    .option norelax
    la   gp, __global_pointer$
    .option pop
    /* a0 = the boot table's satp, 0 = stay low */
    call start_rust
    beqz a0, 3f
    /* The switch (RFC-0098 C4): paging on through the boot table, whose
     * identity leaf keeps the next instructions reachable; then the high
     * alias of the label below, then the second fixup pass with the high
     * base. GOT and data pointers hold physical addresses until it ran, so
     * no Rust executes in between. */
    mv   s0, a0
    csrw satp, a0
    sfence.vma x0, x0
    lla  t0, 4f
    li   t1, {phys_offset}
    add  t0, t0, t1
    jr   t0
4:  lla  t0, __image_start
    fixup
    lla  sp, __stack_top
    .option push
    .option norelax
    la   gp, __global_pointer$
    .option pop
    mv   a0, s0
    j    start_high
3:  j    start_low
"#,
    phys_offset = const PHYS_OFFSET,
);

/// `_start` sets only `sp` and `gp` before jumping here, so the firmware's
/// registers survive as the C-ABI arguments: `a0` = boot hart id, `a1` = the
/// device tree (RFC-0098 C1 — the one hardware truth). Runs at the load
/// address with paging off; returns the boot table's `satp`.
#[no_mangle]
pub extern "C" fn start_rust(hartid: usize, dtb: usize) -> usize {
    // SAFETY: Early boot runs before the Rust runtime. The kernel guarantees
    // that only a single core executes this path, so calling the raw
    // initialisation routine is sound here.
    // The kernel builds its console from the tree before its first log line
    // (RFC-0098 C3); this wrapper prints nothing itself — it knows no address.
    unsafe { neuron::early_boot_init(hartid, dtb) }
}

/// The high half: fixups re-applied, `sp`/`gp` high. Never returns.
#[no_mangle]
pub extern "C" fn start_high(boot_satp: usize) -> ! {
    // SAFETY: exactly once, right after the switch `start_rust` prepared.
    unsafe { neuron::high_boot_init(boot_satp) };
    kmain()
}

/// No boot table could be built (no tree, or memory beyond the direct map):
/// the kernel cannot run — stay put with the console's verdict already out.
#[no_mangle]
pub extern "C" fn start_low() -> ! {
    loop {
        // SAFETY: nothing to do; wait for a debugger or a reset.
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}
