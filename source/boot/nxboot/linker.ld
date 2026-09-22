/* Copyright 2026 Open Nexus OS Contributors
 * SPDX-License-Identifier: Apache-2.0
 *
 * CONTEXT: nxboot bare-metal link script (ADR-0059 as amended by RFC-0098
 * C6, TASK-0245 P2). The loader is a static PIE linked at 0: it runs at
 * whatever address the firmware loads it (the -kernel payload on QEMU, the
 * FIT payload on the board) after `_start` applies the R_RISCV_RELATIVE
 * table between `__rela_dyn_start` and `__rela_dyn_end`; it never moves.
 * The kernel window is chosen from the device tree at run time (the lowest
 * free 2 MiB-aligned window of the first bank that avoids the reserved
 * ranges, this image and the tree). The 256 KiB budget is a HARD ASSERT
 * here and is additionally gated in scripts/check-image-budgets.sh.
 */

ENTRY(_start)

SECTIONS
{
  . = 0;
  __image_start = .;

  .text ALIGN(16) : {
    KEEP(*(.text._start))
    *(.text .text.*)
  }

  . = ALIGN(16);
  .rodata : {
    *(.rodata .rodata.*)
    *(.srodata .srodata.*)
  }

  /* The fixup table `_start` walks, plus the read-only furniture of a PIE link. */
  . = ALIGN(8);
  .rela.dyn : {
    __rela_dyn_start = .;
    *(.rela.dyn)
    *(.rela.*)
    __rela_dyn_end = .;
  }
  .dynsym   : { *(.dynsym) }
  .dynstr   : { *(.dynstr) }
  .gnu.hash : { *(.gnu.hash) }
  .hash     : { *(.hash) }

  . = ALIGN(16);
  .data : {
    /* RISC-V psABI small-data anchor (PC-relative `la gp, ...`). */
    __global_pointer$ = . + 0x800;
    *(.sdata .sdata.*)
    *(.data .data.*)
    *(.got .got.*)
  }
  .dynamic : { *(.dynamic) }
  . = ALIGN(8);
  __load_end = .;

  . = ALIGN(16);
  .bss (NOLOAD) : {
    __bss_start = .;
    *(.sbss .sbss.*)
    *(.bss .bss.*)
    . = ALIGN(16);
    __boot_stack_base = .;
    . += 0x4000; /* 16 KiB loader stack */
    __boot_stack_top = .;
    __bss_end = .;
  }
  __image_end = .;

  /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}

/* ADR-0059: nxboot.bin (loadable bytes) must stay within 256 KiB. */
ASSERT(__load_end - __image_start <= 256K,
       "nxboot exceeds the 256 KiB budget (ADR-0059) — trim before raising")
