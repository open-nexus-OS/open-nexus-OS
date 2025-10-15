ENTRY(_start)

SECTIONS
{
  /*  Standard RISC-V kernel load address (2MB after firmware @ 0x80000000) */
  . = 0x80200000;

  /* Markers for text range (used for runtime exec-range checks) */
  __text_start = .;
  .text : ALIGN(8) {
    KEEP(*(.text._start))
    *(.text .text.*)
  }
  __text_end = .;
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
    /* static page table must be zeroed with BSS as well */
    . = ALIGN(8);
    *(.bss.page_table)
    __bss_end = .;
  }

  /* Private selftest stack with guard pages */
  . = ALIGN(4096);
  __selftest_stack_guard_lo = .;
  . += 0x1000;
  __selftest_stack_base = .;
  KEEP(*(.bss.selftest_stack_body))
  . = ALIGN(4096);
  __selftest_stack_top = .;
  __selftest_stack_guard_hi = __selftest_stack_top + 0x1000;
  . = __selftest_stack_guard_hi;

  /* Dedicated kernel stack region (OS target) */
  . = ALIGN(16);
  __stack_bottom = .;
  . += 0x8000; /* 32 KiB */
  __stack_top = .;
}
