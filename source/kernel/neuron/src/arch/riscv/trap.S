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
    /* Full-context trap entry. CRITICAL: every GPR x1..x31 is saved with its
     * ORIGINAL trapped value so this path is correct for *asynchronous* traps
     * (timer/software interrupts that preempt at an arbitrary instruction), not
     * only synchronous ecalls. The prologue clobbers only t0 and t1 as temps and
     * stashes their originals (orig t0 -> sscratch, orig t1 -> trap-stack red
     * zone); t2..t6 are never touched before they are persisted.
     *
     * sscratch holds the per-hart trap stack top (installed at trap setup).
     * After this swap: t0 = trap_stack_top, sscratch = orig t0. */
    csrrw t0, sscratch, t0
    sd    t1, -8(t0)              /* stash orig t1 in the trap-stack red zone */

    /* Pick the kernel stack based on the trapped privilege (SSTATUS.SPP, bit 8). */
    csrr  t1, sstatus
    andi  t1, t1, 256            /* t1 = SPP (0 => trap from U-mode) */
    bnez  t1, .Ltrap_smode

    /* From U-mode: build the frame on the (empty) trap stack; preserve user sp. */
    sd    sp, -16(t0)            /* stash user sp in the red zone */
    addi  sp, t0, -{TF_SIZE}      /* frame base on the trap stack */
    ld    t1, -16(t0)
    sd    t1, 2*8(sp)            /* frame.x2 = user sp */
    j     .Ltrap_saved_sp

.Ltrap_smode:
    /* From S-mode: keep the current kernel stack so the kernel's own stack depth
     * is never relocated (trap frames stack naturally; traps do not nest, SIE=0). */
    sd    sp, -16(t0)            /* stash kernel sp in the red zone */
    addi  sp, sp, -{TF_SIZE}      /* frame base on the current kernel stack */
    ld    t1, -16(t0)
    sd    t1, 2*8(sp)            /* frame.x2 = kernel sp */

.Ltrap_saved_sp:
    /* sp = frame base. Persist every GPR's ORIGINAL value. */
    sd    ra,   1*8(sp)
    sd    gp,   3*8(sp)
    sd    tp,   4*8(sp)
    /* recover orig t0 (from sscratch) and orig t1 (from the red zone), persist them */
    csrr  t1, sscratch
    sd    t1,   5*8(sp)
    ld    t1, -8(t0)
    sd    t1,   6*8(sp)
    sd    t2,   7*8(sp)          /* orig t2 (never clobbered) */
    /* restore sscratch = trap_stack_top for the next trap */
    csrw  sscratch, t0
    sd    s0,   8*8(sp);  sd  s1,   9*8(sp)
    sd    a0,  10*8(sp);  sd  a1,  11*8(sp);  sd  a2,  12*8(sp);  sd  a3,  13*8(sp)
    sd    a4,  14*8(sp);  sd  a5,  15*8(sp);  sd  a6,  16*8(sp);  sd  a7,  17*8(sp)
    sd    s2,  18*8(sp);  sd  s3,  19*8(sp);  sd  s4,  20*8(sp);  sd  s5,  21*8(sp)
    sd    s6,  22*8(sp);  sd  s7,  23*8(sp);  sd  s8,  24*8(sp);  sd  s9,  25*8(sp)
    sd    s10, 26*8(sp);  sd  s11, 27*8(sp)
    sd    t3,  28*8(sp);  sd  t4,  29*8(sp);  sd  t5,  30*8(sp);  sd  t6,  31*8(sp)

    /* Kernel gp for small-data addressing while in the Rust handler. The trapped
     * gp is already saved at 3*8(sp) and restored on sret. */
    .option push
    .option norelax
    la    gp, __global_pointer$
    .option pop

    /* save CSRs (t0/t1 are now free; their originals are persisted) */
    csrr  t0, sepc
    sd    t0, {OFF_SEPC}(sp)
    csrr  t0, sstatus
    sd    t0, {OFF_SSTATUS}(sp)
    csrr  t0, scause
    sd    t0, {OFF_SCAUSE}(sp)
    csrr  t0, stval
    sd    t0, {OFF_STVAL}(sp)

    /* call Rust handler: a0 = &mut TrapFrame */
    mv    a0, sp
    call  __trap_rust

    /* write back possibly updated sepc/sstatus */
    ld    t0, {OFF_SEPC}(sp)
    csrw  sepc, t0
    ld    t0, {OFF_SSTATUS}(sp)
    csrw  sstatus, t0

    /* restore GPRs; orig sp restored LAST from frame.x2 (handles a frame that the
     * Rust handler swapped to another task during preemption). sscratch already
     * holds trap_stack_top for the next trap. */
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
    ld    sp,   2*8(sp)         /* restore orig sp (must be the final GPR load) */
    sret

    .size __trap_vector, .-__trap_vector

    /*
     * __satp_trampoline(satp_val: usize)
     * Switches SATP to the provided value, flushes TLB, briefly pivots SP to a
     * known-good kernel stack top, then returns.
     *
     * SMP contract note (TASK-0012/RFC-0021):
     * - This trampoline is a boot-hart address-space switch helper.
     * - Multi-hart trap correctness does NOT rely on this path; trap entry uses
     *   per-hart stack tops via sscratch + __hart_trap_stack_tops.
     */
    .section .text.__satp_trampoline, "ax", @progbits
    .globl __satp_trampoline
    .type  __satp_trampoline, @function
    .align 2
/* CONTEXT: boot-hart SATP trampoline; CRITICAL: do not add stack-using code before SP switch */
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
    /* CRITICAL: We restore original SP to avoid clobbering caller's context */
    mv    sp, t1
    ret
    .size __satp_trampoline, .-__satp_trampoline
