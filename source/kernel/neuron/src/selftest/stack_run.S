/*
 * CONTEXT: Run selftest function on a private stack
 * OWNERS: @kernel-team
 * ABI: a0=func(void*), a1=arg, a2=new_sp
 * INVARIANTS: Save/restore caller SP/RA/S0/S1; no extra clobbers; non-functional annotation only
 */
.section .text.stack_run, "ax", @progbits
.globl run_selftest_on_stack
.align 4
// void run_selftest_on_stack(void (*func)(void*), void* arg, void* new_sp)
// a0 = func, a1 = arg, a2 = new_sp
run_selftest_on_stack:
    addi    sp, sp, -24
    sd      ra, 16(sp)
    sd      s0, 8(sp)
    sd      s1, 0(sp)
    mv      s1, sp           // save old sp in s1
    mv      sp, a2           // switch to new stack
    mv      t0, a0           // preserve func pointer in t0
    mv      a0, a1           // move arg into a0 (ABI)
    jalr    ra, t0, 0        // call func(arg)
    mv      sp, s1           // restore old sp
    ld      s1, 0(sp)
    ld      s0, 8(sp)
    ld      ra, 16(sp)
    addi    sp, sp, 24
    ret
