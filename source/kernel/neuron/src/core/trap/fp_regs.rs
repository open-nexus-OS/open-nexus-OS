// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the routines that move a unit's registers between a hart and a thread's state
//! (RFC-0119) — the only floating-point and vector instructions of the kernel. Each routine
//! turns its unit on in the live status word, moves the registers, and turns it off again:
//! outside them the kernel runs with both units off, and under one vector configuration —
//! the one after reset (`__vector_rest` at a vector thread's trap entry, `__vector_resume`
//! at its return).
//! A float state is `f0`–`f31` then `fcsr` (33 words). A vector state is `vstart`, `vcsr`,
//! `vl`, `vtype` (4 words) then `v0`–`v31`, each as wide as the hart's registers; the
//! registers move whole, whatever length and type the thread has set.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU and board: `SELFTEST: fpsoak ok`, `SELFTEST: vsoak ok` (every register
//!   of both sets compared after every hand-over and across migrations)
//! INVARIANTS: a routine leaves its unit off; a vector state's memory holds 4 words and 32
//!   registers of the width `__vector_bytes` returns on this hart
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

core::arch::global_asm!(
    ".option push",
    ".option arch, +f, +d, +v",
    // __fp_store(a0 = words of a float state): the hart's registers into the state.
    ".globl __fp_store",
    ".type __fp_store, @function",
    "__fp_store:",
    "li t0, 0x6000",
    "csrs sstatus, t0",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"fsd f\n, 8*\n(a0)",
    ".endr",
    "frcsr t1",
    "sd t1, 256(a0)",
    "csrc sstatus, t0",
    "ret",
    // __fp_load(a0 = words of a float state): the state into the hart's registers.
    ".globl __fp_load",
    ".type __fp_load, @function",
    "__fp_load:",
    "li t0, 0x6000",
    "csrs sstatus, t0",
    "ld t1, 256(a0)",
    "fscsr t1",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"fld f\n, 8*\n(a0)",
    ".endr",
    "csrc sstatus, t0",
    "ret",
    // __vector_store(a0 = words of a vector state). The control registers by number:
    // 0x008 vstart, 0x00f vcsr, 0xc20 vl, 0xc21 vtype, 0xc22 vlenb. A whole-register store
    // starts at `vstart`, so it is zero for the stores; the thread's own start, length and
    // type come back from the state at its return (`__vector_resume`).
    ".globl __vector_store",
    ".type __vector_store, @function",
    "__vector_store:",
    "li t0, 0x600",
    "csrs sstatus, t0",
    "csrr t1, 0x008",
    "sd t1, 0(a0)",
    "csrr t1, 0x00f",
    "sd t1, 8(a0)",
    "csrr t1, 0xc20",
    "sd t1, 16(a0)",
    "csrr t1, 0xc21",
    "sd t1, 24(a0)",
    "csrw 0x008, x0",
    "csrr t2, 0xc22",
    "addi a1, a0, 32",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"vs1r.v v\n, (a1)",
    "add a1, a1, t2",
    ".endr",
    "csrc sstatus, t0",
    "ret",
    // __vector_rest(): the hart's start, length and type as after reset — start 0, length 0,
    // no type. The registers and the control register stay what they are.
    ".globl __vector_rest",
    ".type __vector_rest, @function",
    "__vector_rest:",
    "li t0, 0x600",
    "csrs sstatus, t0",
    "li t1, 0",
    "li t3, 1",
    "slli t3, t3, 63",
    "vsetvl x0, t1, t3",
    "csrw 0x008, x0",
    "csrc sstatus, t0",
    "ret",
    // __vector_resume(a0 = words of a vector state): the thread's length, type and start
    // again, on a hart that still holds its registers.
    ".globl __vector_resume",
    ".type __vector_resume, @function",
    "__vector_resume:",
    "li t0, 0x600",
    "csrs sstatus, t0",
    "ld t1, 16(a0)",
    "ld t3, 24(a0)",
    "vsetvl x0, t1, t3",
    "ld t1, 0(a0)",
    "csrw 0x008, t1",
    "csrc sstatus, t0",
    "ret",
    // __vector_load(a0 = words of a vector state): the registers whole, then the length and
    // the type as one setting (an illegal type stays illegal, with length 0), then the
    // control register and the start index — which the setting had cleared.
    ".globl __vector_load",
    ".type __vector_load, @function",
    "__vector_load:",
    "li t0, 0x600",
    "csrs sstatus, t0",
    "csrw 0x008, x0",
    "csrr t2, 0xc22",
    "addi a1, a0, 32",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"vl1re8.v v\n, (a1)",
    "add a1, a1, t2",
    ".endr",
    "ld t1, 16(a0)",
    "ld t3, 24(a0)",
    "vsetvl x0, t1, t3",
    "ld t1, 8(a0)",
    "csrw 0x00f, t1",
    "ld t1, 0(a0)",
    "csrw 0x008, t1",
    "csrc sstatus, t0",
    "ret",
    // __vector_bytes() -> a0: the width of this hart's vector registers in bytes.
    ".globl __vector_bytes",
    ".type __vector_bytes, @function",
    "__vector_bytes:",
    "li t0, 0x600",
    "csrs sstatus, t0",
    "csrr a0, 0xc22",
    "csrc sstatus, t0",
    "ret",
    // __units_status() -> a0: the live status word.
    ".globl __units_status",
    ".type __units_status, @function",
    "__units_status:",
    "csrr a0, sstatus",
    "ret",
    // __units_off(): both units off in the live status word.
    ".globl __units_off",
    ".type __units_off, @function",
    "__units_off:",
    "li t0, 0x6600",
    "csrc sstatus, t0",
    "ret",
    ".option pop",
);

extern "C" {
    /// Stores `f0`–`f31` and `fcsr` into 33 words.
    pub(super) fn __fp_store(words: *mut u64);
    /// Loads `fcsr` and `f0`–`f31` from 33 words.
    pub(super) fn __fp_load(words: *const u64);
    /// Stores `vstart`, `vcsr`, `vl`, `vtype` and `v0`–`v31` whole.
    pub(super) fn __vector_store(words: *mut u64);
    /// Loads `v0`–`v31` whole, then `vl` and `vtype`, `vcsr`, `vstart`.
    pub(super) fn __vector_load(words: *const u64);
    /// Sets this hart's `vstart`, `vl` and `vtype` as after reset. Only on a hart whose node
    /// states the vector extension.
    pub(super) fn __vector_rest();
    /// Sets `vl`, `vtype` and `vstart` from a vector state whose registers the hart holds.
    pub(super) fn __vector_resume(words: *const u64);
    /// The width of this hart's vector registers in bytes. Only on a hart whose node states
    /// the vector extension — elsewhere the read is an illegal instruction in the kernel.
    pub(super) fn __vector_bytes() -> usize;
    /// The live status word of this hart.
    pub(super) fn __units_status() -> usize;
    /// Turns both units off in the live status word.
    pub(super) fn __units_off();
}
