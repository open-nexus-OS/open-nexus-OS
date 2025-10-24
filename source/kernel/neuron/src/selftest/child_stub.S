/*
 * CONTEXT: Minimal child entry stub for selftests
 * OWNERS: @kernel-team
 * PURPOSE: Ensure a defined return path (ra) even if test code returns
 * INVARIANTS: Do not add stack usage; keep within text; non-functional annotation only
 */
.section .text.child_stub, "ax", @progbits
.globl child_entry_asm
.align 2
child_entry_asm:
// set up a safe return address within text to avoid sepc=0
    auipc   ra, 0         // ra := here
    addi    ra, ra, 8     // skip over the next instruction block
    nop
    ret
