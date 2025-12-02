ENTRY(_start)

PHDRS
{
  text PT_LOAD FLAGS(5);  /* R | X */
  data PT_LOAD FLAGS(6);  /* R | W */
}

SECTIONS
{
  /*  Standard RISC-V kernel load address (2MB after firmware @ 0x80000000) */
  . = 0x80200000;

  /* Markers for text range (used for runtime exec-range checks) */
  __text_start = .;
  .text ALIGN(16) : {
    KEEP(*(.text._start))
    *(.text .text.*)
    *(.text.stack_run)
  } : text
  __text_end = .;

  . = ALIGN(16);
  .rodata ALIGN(16) : { *(.rodata .rodata.*) } : text

  . = ALIGN(16);
  .data ALIGN(16) : { *(.sdata .sdata.*) *(.data .data.*) } : data

  . = ALIGN(4096);
  .bss (NOLOAD) : {
    __bss_start = .;
    *(.sbss .sbss.*)
    /* Private selftest stack with guard pages lives at a 4K boundary */
    . = ALIGN(4096);
    __selftest_stack_guard_lo = .;
    . += 0x1000;
    __selftest_stack_base = .;
    *(.bss.selftest_stack_body)
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

  /* (no standalone selftest stack section) */

  /* Dedicated kernel stack region (OS target) */
  . = ALIGN(16);
  __stack_bottom = .;
  . += 0x8000; /* 32 KiB */
  __stack_top = .;
}
