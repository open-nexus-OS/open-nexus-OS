ENTRY(_start)

SECTIONS
{
  /*  Low memory base for -kernel with -bios default (OpenSBI relocates) */
 . = 0x00010000;

  .text : ALIGN(8) {
    KEEP(*(.text._start))
    *(.text .text.*)
  }
  .rodata : ALIGN(8) { *(.rodata .rodata.*) }

  .data : ALIGN(8) { *(.sdata .sdata.*) *(.data .data.*) }

  .bss (NOLOAD) : ALIGN(8) {
    __bss_start = .;
    *(.sbss .sbss.*)
    *(.bss .bss.*)
    *(COMMON)
    /* important: Heap lies in .bss.heap */
    *(.bss.heap)
    __bss_end = .;
  }
}
