// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The vector forms (RISC-V "V"). Assembly, because the vector extension is no build flag of
//! any program (ADR-0099 rule 4): the compiler never emits a vector instruction, these
//! functions are the only ones in a program, and they run only when the kernel reported the
//! unit ([`crate::choose`]).
//!
//! WIDTH: nothing here names one. `vsetvli` answers how many bytes a register group holds on
//! this hart, and every loop takes its step from that answer (16 bytes a register on the
//! emulator, 32 on the board).
//!
//! A STEP IS WHOLE PIXELS: a kernel hands `vsetvli` at most a group's size, never the whole
//! rest — asked for more than a group holds, a hart may answer with any length down to half
//! the request, and a step that is no multiple of four bytes would shift the colour's channels
//! against the pixels. Asked for no more than a group holds, it answers with the request.
//!
//! BYTES, NOT WORDS, CROSS MEMORY: loads and stores are byte-wide, so a row needs no alignment.

// The one place of this crate that leaves safe Rust: two leaf functions over a caller's slice.
#![allow(unsafe_code)]

use crate::reference::Over;

core::arch::global_asm!(
    ".pushsection .text.__paint_kernels,\"ax\",@progbits",
    ".option push",
    ".option arch, +v",
    // __paint_fill(a0 = dst, a1 = bytes: a multiple of four above zero, a2 = the pixel).
    ".balign 4",
    ".globl __paint_fill",
    ".type __paint_fill, @function",
    "__paint_fill:",
    // The pixel in every 32-bit lane of v8..v15, the whole group.
    "vsetvli t0, x0, e32, m8, ta, ma",
    "vmv.v.x v8, a2",
    // The group's size in bytes — as byte lanes it is the same registers.
    "vsetvli t1, x0, e8, m8, ta, ma",
    "1:",
    "mv t0, a1",
    "bleu a1, t1, 2f",
    "mv t0, t1",
    "2:",
    "vsetvli t0, t0, e8, m8, ta, ma",
    "vse8.v v8, (a0)",
    "add a0, a0, t0",
    "sub a1, a1, t0",
    "bnez a1, 1b",
    "ret",
    ".size __paint_fill, . - __paint_fill",
    // __paint_blend(a0 = dst, a1 = bytes: a multiple of four above zero, a2 = inv,
    //               a3 = the four 16-bit shares of the colour, blue lowest).
    // Per byte: (dst * inv + share) / 255, the division as the high half of a multiply by
    // 0x8081 shifted right by seven — exact for every 16-bit sum.
    ".balign 4",
    ".globl __paint_blend",
    ".type __paint_blend, @function",
    "__paint_blend:",
    // The shares in every 64-bit lane of v24..v31: as 16-bit lanes, lane i holds share i mod 4.
    "vsetvli t0, x0, e64, m8, ta, ma",
    "vmv.v.x v24, a3",
    // Bytes a step takes: an m4 group of byte lanes, widened into an m8 group of 16-bit lanes.
    "vsetvli t1, x0, e8, m4, ta, ma",
    "li t2, 0x8081",
    "1:",
    "mv t0, a1",
    "bleu a1, t1, 2f",
    "mv t0, t1",
    "2:",
    "vsetvli t0, t0, e8, m4, ta, ma",
    "vle8.v v8, (a0)",
    "vwmulu.vx v16, v8, a2",
    // Same count of lanes, twice as wide (the ratio of lane to group stays, so the length does).
    "vsetvli x0, x0, e16, m8, ta, ma",
    "vadd.vv v16, v16, v24",
    "vmulhu.vx v16, v16, t2",
    "vsetvli x0, x0, e8, m4, ta, ma",
    "vnsrl.wi v8, v16, 7",
    "vse8.v v8, (a0)",
    "add a0, a0, t0",
    "sub a1, a1, t0",
    "bnez a1, 1b",
    "ret",
    ".size __paint_blend, . - __paint_blend",
    ".option pop",
    ".popsection",
);

extern "C" {
    fn __paint_fill(dst: *mut u8, bytes: usize, px: u32);
    fn __paint_blend(dst: *mut u8, bytes: usize, inv: usize, shares: u64);
}

/// [`crate::reference::fill`] on the vector unit.
pub(crate) fn fill(row: &mut [u8], px: [u8; 4]) {
    let bytes = row.len() & !3;
    if bytes == 0 {
        return;
    }
    // SAFETY: writes exactly `bytes` bytes from the start of `row`, which holds at least that
    // many and is borrowed exclusively; the function keeps nothing. The caller chose the vector
    // unit from the kernel's report, so the instructions exist on every hart this thread runs on.
    unsafe { __paint_fill(row.as_mut_ptr(), bytes, u32::from_le_bytes(px)) }
}

/// [`crate::reference::blend`] on the vector unit.
pub(crate) fn blend(row: &mut [u8], over: &Over) {
    let bytes = row.len() & !3;
    if bytes == 0 {
        return;
    }
    let [b, g, r, a] = over.add.map(u64::from);
    let shares = b | g << 16 | r << 32 | a << 48;
    // SAFETY: reads and writes exactly `bytes` bytes from the start of `row` (see `fill`).
    unsafe { __paint_blend(row.as_mut_ptr(), bytes, usize::from(over.inv), shares) }
}
