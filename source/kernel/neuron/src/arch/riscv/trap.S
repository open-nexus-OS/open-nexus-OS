/* SPDX-License-Identifier: Apache-2.0 */
/* Trap vector entry for RISC-V S-mode.
 *
 * Responsibilities:
 *  - allocate a TrapFrame on the current stack
 *  - save GPRs x1..x31 and key CSRs (sepc, sstatus, scause, stval)
 *  - call __trap_rust(&mut TrapFrame)
 *  - restore CSRs/GPRs and sret
 *
 * Notes:
 *  - x0 is constant zero; we don’t store it.
 *  - We deallocate the frame via addi sp,sp,+TF_SIZE (no need to reload x2).
 *  - Offsets and size are injected by Rust via global_asm! (TF_SIZE, OFF_*).
 */

    .section .text.__trap_vector, "ax", @progbits
    .globl __trap_vector
    .type  __trap_vector, @function
    .align 4

__trap_vector:
    /* allocate TrapFrame */
    addi  sp, sp, -{TF_SIZE}

    /* save GPRs x1..x31 into frame.x[i] (x0 is implicit) */
    sd    ra,   1*8(sp)
    sd    sp,   2*8(sp)         /* saved for completeness; we restore via addi */
    sd    gp,   3*8(sp)
    sd    tp,   4*8(sp)
    sd    t0,   5*8(sp);  sd  t1,   6*8(sp);  sd  t2,   7*8(sp)
    sd    s0,   8*8(sp);  sd  s1,   9*8(sp)
    sd    a0,  10*8(sp);  sd  a1,  11*8(sp);  sd  a2,  12*8(sp);  sd  a3,  13*8(sp)
    sd    a4,  14*8(sp);  sd  a5,  15*8(sp);  sd  a6,  16*8(sp);  sd  a7,  17*8(sp)
    sd    s2,  18*8(sp);  sd  s3,  19*8(sp);  sd  s4,  20*8(sp);  sd  s5,  21*8(sp)
    sd    s6,  22*8(sp);  sd  s7,  23*8(sp);  sd  s8,  24*8(sp);  sd  s9,  25*8(sp)
    sd    s10, 26*8(sp);  sd  s11, 27*8(sp)
    sd    t3,  28*8(sp);  sd  t4,  29*8(sp);  sd  t5,  30*8(sp);  sd  t6,  31*8(sp)

    /* save CSRs */
    csrr  t0, sepc
    sd    t0, {OFF_SEPC}(sp)
    csrr  t1, sstatus
    sd    t1, {OFF_SSTATUS}(sp)
    csrr  t2, scause
    sd    t2, {OFF_SCAUSE}(sp)
    csrr  t3, stval
    sd    t3, {OFF_STVAL}(sp)

    /* call Rust handler: a0 = &mut TrapFrame */
    mv    a0, sp
    call  __trap_rust

    /* write back possibly updated sepc/sstatus */
    ld    t0, {OFF_SEPC}(sp)
    csrw  sepc, t0
    ld    t1, {OFF_SSTATUS}(sp)
    csrw  sstatus, t1

    /* restore GPRs (sp via dealloc below) */
    ld    ra,   1*8(sp)
    ld    gp,   3*8(sp)
    ld    tp,   4*8(sp)
    ld    t0,   5*8(sp);  ld  t1,   6*8(sp);  ld  t2,   7*8(sp)
    ld    s0,   8*8(sp);  ld  s1,   9*8(sp)
    ld    a0,  10*8(sp);  ld  a1,  11*8(sp);  ld  a2,  12*8(sp);  ld  a3,  13*8(sp)
    ld    a4,  14*8(sp);  ld  a5,  15*8(sp);  ld  a6,  16*8(sp);  ld  a7,  17*8(sp)
    ld    s2,  18*8(sp);  ld  s3,  19*8(sp);  ld  s4,  20*8(sp);  ld  s5,  21*8(sp)
    ld    s6,  22*8(sp);  ld  s7,  23*8(sp);  ld  s8,  24*8(sp);  ld  s9,  25*8(sp)
    ld    s10, 26*8(sp);  ld  s11, 27*8(sp)
    ld    t3,  28*8(sp);  ld  t4,  29*8(sp);  ld  t5,  30*8(sp);  ld  t6,  31*8(sp)

    /* deallocate frame and return */
    addi  sp, sp, {TF_SIZE}
    sret

    .size __trap_vector, .-__trap_vector

    /*
     * __satp_trampoline(satp_val: usize)
     * Switches SATP to the provided value, flushes TLB, sets SP to a known-good
     * kernel stack top, emits a few UART bytes to signal liveness, then returns.
     */
    .section .text.__satp_trampoline, "ax", @progbits
    .globl __satp_trampoline
    .type  __satp_trampoline, @function
    .align 2
__satp_trampoline:
    mv    t1, sp
    csrw  satp, a0
    sfence.vma x0, x0
    .option push
    .option norelax
    la    t0, __stack_top
    .option pop
    addi  t0, t0, -64
    mv    sp, t0
    mv    sp, t1
    ret
    .size __satp_trampoline, .-__satp_trampoline
