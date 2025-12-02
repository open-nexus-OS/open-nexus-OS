OUTPUT_ARCH(riscv)
ENTRY(_start)

MEMORY {
  RAM (rwx) : ORIGIN = 0x10400000, LENGTH = 2M
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
