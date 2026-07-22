OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY {
  /* User VA window for the embedded service bundle. 8M -> 24M (RFC-0075
     Phase 8d): the baked CJK glyph atlases ride in windowd AND app-host
     (~4.2M each); the next neighbor is the stack region at 0x2000_0000,
     so the window has ample VA headroom. Follow-up: share ONE atlas via a
     read-only VMO instead of two embedded copies. */
  RAM (rwx) : ORIGIN = 0x10400000, LENGTH = 24M
}

SECTIONS {
  .text : {
    __text_start = .;
    *(.text._start)
    *(.text .text.*)
    __text_end = .;
  } > RAM

  .rodata : {
    __rodata_start = .;
    *(.rodata .rodata.*)
    __rodata_end = .;
  } > RAM

  .data : {
    __data_start = .;
    *(.data .data.*)
    __data_end = .;
  } > RAM

  .sdata : {
    __sdata_start = .;
    *(.sdata .sdata.*)
    __sdata_end = .;
  } > RAM

  .bss : {
    __bss_start = .;
    *(.bss .bss.* COMMON)
    *(.sbss .sbss.*)
    __bss_end = .;
  } > RAM

  .guard (NOLOAD) : {
    . = ALIGN(0x1000);
    __small_data_guard = .;
    . += 0x20000;
  } > RAM

  .image_end : {
    __image_end = .;
    BYTE(0);
  } > RAM
}
