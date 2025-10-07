ENTRY(_start)

SECTIONS
{
  /*  Standard RISC-V kernel load address (2MB after firmware @ 0x80000000) */
  . = 0x80200000;

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
    . = ALIGN(64);
    *(.bss.heap)
    __bss_end = .;
  }
}
