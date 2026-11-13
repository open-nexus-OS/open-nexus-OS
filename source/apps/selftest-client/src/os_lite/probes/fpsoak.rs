// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the floating-point side of the unit soak (`unitsoak`, RFC-0119): a thread's
//!   pattern in `f0`–`f31` and the control register, written and compared by the two
//!   routines below. The program is built for the hard-float target (ADR-0099), but the
//!   harness computes with integers only between a seed and its check, so nothing else
//!   touches the registers there. The pattern covers the registers a caller keeps across a
//!   call (`fs0`–`fs11`, the rounding mode): the soak takes them aside first and gives them
//!   back at its end (`keep`, `give_back`).
//!   Run only when the kernel's answer names the floating-point unit.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) and the board — `SELFTEST: fpsoak ok`.
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

use super::unitsoak::Who;

core::arch::global_asm!(
    ".option push",
    ".option arch, +f, +d",
    // __fpsoak_seed(a0 = base, a1 = control): f<n> = the bits base + n, fcsr = control.
    ".globl __fpsoak_seed",
    ".type __fpsoak_seed, @function",
    "__fpsoak_seed:",
    "fscsr a1",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"fmv.d.x f\n, a0",
    "addi a0, a0, 1",
    ".endr",
    "ret",
    // __fpsoak_check(a0 = base, a1 = control) -> a0: bit n = f<n> differs, bit 32 = fcsr.
    ".globl __fpsoak_check",
    ".type __fpsoak_check, @function",
    "__fpsoak_check:",
    "li t2, 0",
    "frcsr t0",
    "beq t0, a1, 1f",
    "li t2, 1",
    "slli t2, t2, 32",
    "1:",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"fmv.x.d t0, f\n",
    "beq t0, a0, 2f",
    "li t1, 1",
    r"slli t1, t1, \n",
    "or t2, t2, t1",
    "2:",
    "addi a0, a0, 1",
    ".endr",
    "mv a0, t2",
    "ret",
    // __fpsoak_keep(a0 = 13 words): fs0-fs11 (f8, f9, f18-f27) and fcsr into memory.
    ".globl __fpsoak_keep",
    ".type __fpsoak_keep, @function",
    "__fpsoak_keep:",
    "fsd f8, 0(a0)",
    "fsd f9, 8(a0)",
    ".irp n, 18,19,20,21,22,23,24,25,26,27",
    r"fsd f\n, 8*(\n-16)(a0)",
    ".endr",
    "frcsr t0",
    "sd t0, 96(a0)",
    "ret",
    // __fpsoak_give_back(a0 = 13 words): the same registers from memory.
    ".globl __fpsoak_give_back",
    ".type __fpsoak_give_back, @function",
    "__fpsoak_give_back:",
    "fld f8, 0(a0)",
    "fld f9, 8(a0)",
    ".irp n, 18,19,20,21,22,23,24,25,26,27",
    r"fld f\n, 8*(\n-16)(a0)",
    ".endr",
    "ld t0, 96(a0)",
    "fscsr t0",
    "ret",
    ".option pop",
);

extern "C" {
    fn __fpsoak_seed(base: u64, control: u64);
    fn __fpsoak_check(base: u64, control: u64) -> u64;
    fn __fpsoak_keep(words: *mut [u64; 13]);
    fn __fpsoak_give_back(words: *const [u64; 13]);
}

/// What the soak's caller holds in the registers a call must preserve.
pub(super) struct Kept([u64; 13]);

/// Takes the caller's `fs0`–`fs11` and control register aside.
pub(super) fn keep() -> Kept {
    let mut kept = Kept([0; 13]);
    // SAFETY: stores twelve registers and the control register into the array.
    unsafe { __fpsoak_keep(&mut kept.0) };
    kept
}

/// Gives the caller its registers back.
pub(super) fn give_back(kept: &Kept) {
    // SAFETY: loads the registers `keep` stored.
    unsafe { __fpsoak_give_back(&kept.0) }
}

/// A thread's pattern: the bits of its registers and its control register (a rounding mode
/// other than the default, and flags) — the two threads share no value.
#[derive(Clone, Copy)]
struct Pattern {
    base: u64,
    control: u64,
}

const fn pattern(who: Who) -> Pattern {
    match who {
        Who::Main => Pattern { base: 0x5A5A_F0F0_0000_0100, control: 0x3F },
        Who::Partner => Pattern { base: 0xC3C3_0F0F_0000_0200, control: 0x4A },
    }
}

/// Writes `who`'s pattern into `f0`–`f31` and the control register.
pub(super) fn seed(who: Who) {
    let Pattern { base, control } = pattern(who);
    // SAFETY: writes the floating-point registers only.
    unsafe { __fpsoak_seed(base, control) }
}

/// The registers that differ from `who`'s pattern (bit n = `f<n>`, bit 32 = the control
/// register).
pub(super) fn differs(who: Who) -> u64 {
    let Pattern { base, control } = pattern(who);
    // SAFETY: reads the floating-point registers only.
    unsafe { __fpsoak_check(base, control) }
}
