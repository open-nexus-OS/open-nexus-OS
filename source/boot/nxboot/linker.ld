/* Copyright 2026 Open Nexus OS Contributors
 * SPDX-License-Identifier: Apache-2.0
 *
 * CONTEXT: nxboot bare-metal link script (ADR-0059 memory contract).
 * The firmware loads the -kernel payload at 0x8020_0000; nxboot is LINKED
 * for its high home 0x8E00_0000 and self-relocates there in early asm so
 * it can later load the verified boot image to 0x8020_0000 without
 * overlapping itself. The 256 KiB budget is a HARD ASSERT here and is
 * additionally gated in scripts/check-image-budgets.sh once the boot flip
 * lands (TASK-0289 A4).
 */

ENTRY(_start)

SECTIONS
{
  . = 0x8E000000;
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

  . = ALIGN(16);
  .data : {
    /* RISC-V psABI small-data anchor (PC-relative `la gp, ...`). */
    __global_pointer$ = . + 0x800;
    *(.sdata .sdata.*)
    *(.data .data.*)
  }
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
