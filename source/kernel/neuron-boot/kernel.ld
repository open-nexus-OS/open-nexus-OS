/* Copyright 2024 Open Nexus OS Contributors
 * SPDX-License-Identifier: Apache-2.0
 *
 * CONTEXT: the kernel's link script. The image is a static PIE (RFC-0098 C6,
 * TASK-0245 P2): linked at 0, loaded wherever the previous stage puts it
 * (nxboot: the lowest free 2 MiB-aligned window of the first memory bank;
 * a direct `-kernel` boot: the firmware's supervisor entry), and fixed up by
 * its own `_start` from the R_RISCV_RELATIVE table between
 * `__rela_dyn_start` and `__rela_dyn_end`. Code is PC-relative (medany);
 * nothing in this file is a machine address.
 */
ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);  /* R | X */
  data PT_LOAD FLAGS(6);  /* R | W */
}

SECTIONS
{
  . = 0;
  __image_start = .;

  /* Markers for text range (used for runtime exec-range checks) */
  __text_start = .;
  .text ALIGN(16) : {
    KEEP(*(.text._start))
    *(.text .text.*)
    *(.text.stack_run)
  } : text
  __text_end = .;

  . = ALIGN(16);
  .rodata ALIGN(16) : { *(.rodata .rodata.*) *(.srodata .srodata.*) } : text

  /* The fixup table the entry code walks, plus the dynamic-section furniture a
   * PIE link emits (read-only; never consulted at run time). */
  . = ALIGN(8);
  .rela.dyn : {
    __rela_dyn_start = .;
    *(.rela.dyn)
    *(.rela.*)
    __rela_dyn_end = .;
  } : text
  .dynsym   : { *(.dynsym) } : text
  .dynstr   : { *(.dynstr) } : text
  .gnu.hash : { *(.gnu.hash) } : text
  .hash     : { *(.hash) } : text

  . = ALIGN(16);
  .data ALIGN(16) : {
    /* RISC-V psABI: gp points 0x800 bytes past the start of the small-data region.
     * We define this symbol explicitly so PC-relative `la gp, __global_pointer$` works even when
     * the toolchain would otherwise default it to a low address (breaking kernel statics). */
    __global_pointer$ = . + 0x800;
    *(.sdata .sdata.*)
    *(.data .data.*)
    *(.got .got.*)
  } : data
  .dynamic : { *(.dynamic) } : data

  . = ALIGN(4096);
  .bss (NOLOAD) : {
    __bss_start = .;
    *(.sbss .sbss.*)
    /* Private selftest stack with guard pages lives at a 4K boundary */
    . = ALIGN(4096);
    __selftest_stack_guard_lo = .;
    . += 0x1000;
    __selftest_stack_base = .;
    KEEP(*(.bss.selftest_stack_body))
    . = ALIGN(4096);
    __selftest_stack_top = .;
    __selftest_stack_guard_hi = __selftest_stack_top + 0x1000;
    /* SATP island stack (4 KiB) used during SATP switching */
    . = ALIGN(4096);
    __satp_island_stack_base = .;
    . += 0x1000;
    __satp_island_stack_top = .;
    /* Rest of BSS */
    *(.bss .bss.*)
    *(COMMON)
    /* important: Heap lies in .bss.heap */
    . = ALIGN(64);
    *(.bss.heap)
    /* static page table must be zeroed with BSS as well */
    . = ALIGN(8);
    *(.bss.page_table)
    __bss_end = .;
  } : data

  /* Dedicated kernel stack region (OS target).
   * NOTE: the boot hart's U-mode trap frames + full syscall dispatch run on
   * this stack, and the last .bss symbols (incl. the heap allocator struct)
   * sit directly below __stack_bottom — an overflow silently zeroes them
   * (symptom: ALLOC-FAIL with heap used=0/free=0). Keep generous headroom. */
  . = ALIGN(16);
  __stack_bottom = .;
  . += 0x10000; /* 64 KiB */
  __stack_top = .;
  __image_end = .;
}
