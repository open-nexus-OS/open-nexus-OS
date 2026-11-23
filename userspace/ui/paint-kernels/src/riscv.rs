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

// The one place of this crate that leaves safe Rust: leaf functions over a caller's slices.
#![allow(unsafe_code)]

use crate::reference::{Over, STRAIGHT};

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
    // ——— The per-pixel kernels: a pixel is one 32-bit lane (blue lowest), so a step asked
    // for in PIXELS at e32 is whole pixels whatever the hart answers; the bytes still cross
    // memory as byte lanes of the same register group.
    //
    // __paint_cover(a0 = dst, a1 = pixels above zero, a2 = one alpha byte a pixel,
    //               a3 = the colour: blue | green << 8 | red << 16).
    // Per channel: ((d * (255 - a) + colour * a) * 0x8081) >> 23 — the sum is at most
    // 255 * 255, so this is the division by 255; the alpha channel's colour is 255.
    ".balign 4",
    ".globl __paint_cover",
    ".type __paint_cover, @function",
    "__paint_cover:",
    "andi t4, a3, 255",
    "srli t5, a3, 8",
    "andi t5, t5, 255",
    "srli t6, a3, 16",
    "andi t6, t6, 255",
    "li t2, 255",
    "li t3, 0x8081",
    "1:",
    "vsetvli t0, a1, e32, m4, ta, ma",
    "slli t1, t0, 2",
    // As many alpha bytes as pixels: an m1 group of byte lanes holds what an m4 group of
    // 32-bit lanes does.
    "vsetvli x0, t0, e8, m1, ta, ma",
    "vle8.v v28, (a2)",
    "vsetvli x0, t1, e8, m4, ta, ma",
    "vle8.v v8, (a0)",
    "vsetvli x0, t0, e32, m4, ta, ma",
    "vzext.vf4 v12, v28",
    "vrsub.vx v16, v12, t2",
    // Blue.
    "vand.vx v24, v8, t2",
    "vmul.vv v24, v24, v16",
    "vmacc.vx v24, t4, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v20, v24, 23",
    // Green.
    "vsrl.vi v24, v8, 8",
    "vand.vx v24, v24, t2",
    "vmul.vv v24, v24, v16",
    "vmacc.vx v24, t5, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v24, v24, 23",
    "vsll.vi v24, v24, 8",
    "vor.vv v20, v20, v24",
    // Red.
    "vsrl.vi v24, v8, 16",
    "vand.vx v24, v24, t2",
    "vmul.vv v24, v24, v16",
    "vmacc.vx v24, t6, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v24, v24, 23",
    "vsll.vi v24, v24, 16",
    "vor.vv v20, v20, v24",
    // Alpha.
    "vsrl.vi v24, v8, 24",
    "vmul.vv v24, v24, v16",
    "vmacc.vx v24, t2, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v24, v24, 23",
    "vsll.vi v24, v24, 24",
    "vor.vv v20, v20, v24",
    "vsetvli x0, t1, e8, m4, ta, ma",
    "vse8.v v20, (a0)",
    "add a0, a0, t1",
    "add a2, a2, t0",
    "sub a1, a1, t0",
    "bnez a1, 1b",
    "ret",
    ".size __paint_cover, . - __paint_cover",
    // __paint_over(a0 = dst, a1 = pixels above zero, a2 = the straight source pixels, red
    // lowest: a sprite's byte order). Per channel as the cover, the colour and the alpha each
    // pixel's own: the row's blue takes the source's third byte, its red the first.
    ".balign 4",
    ".globl __paint_over",
    ".type __paint_over, @function",
    "__paint_over:",
    "li t2, 255",
    "li t3, 0x8081",
    "1:",
    "vsetvli t0, a1, e32, m4, ta, ma",
    "slli t1, t0, 2",
    "vsetvli x0, t1, e8, m4, ta, ma",
    "vle8.v v8, (a0)",
    "vle8.v v4, (a2)",
    "vsetvli x0, t0, e32, m4, ta, ma",
    "vsrl.vi v12, v4, 24",
    "vrsub.vx v16, v12, t2",
    // Blue: the source's third byte.
    "vand.vx v24, v8, t2",
    "vmul.vv v24, v24, v16",
    "vsrl.vi v28, v4, 16",
    "vand.vx v28, v28, t2",
    "vmacc.vv v24, v28, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v20, v24, 23",
    // Green.
    "vsrl.vi v24, v8, 8",
    "vand.vx v24, v24, t2",
    "vmul.vv v24, v24, v16",
    "vsrl.vi v28, v4, 8",
    "vand.vx v28, v28, t2",
    "vmacc.vv v24, v28, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v24, v24, 23",
    "vsll.vi v24, v24, 8",
    "vor.vv v20, v20, v24",
    // Red: the source's first byte.
    "vsrl.vi v24, v8, 16",
    "vand.vx v24, v24, t2",
    "vmul.vv v24, v24, v16",
    "vand.vx v28, v4, t2",
    "vmacc.vv v24, v28, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v24, v24, 23",
    "vsll.vi v24, v24, 16",
    "vor.vv v20, v20, v24",
    // Alpha: the alpha channel's colour is 255.
    "vsrl.vi v24, v8, 24",
    "vmul.vv v24, v24, v16",
    "vmacc.vx v24, t2, v12",
    "vmul.vx v24, v24, t3",
    "vsrl.vi v24, v24, 23",
    "vsll.vi v24, v24, 24",
    "vor.vv v20, v20, v24",
    "vsetvli x0, t1, e8, m4, ta, ma",
    "vse8.v v20, (a0)",
    "add a0, a0, t1",
    "add a2, a2, t1",
    "sub a1, a1, t0",
    "bnez a1, 1b",
    "ret",
    ".size __paint_over, . - __paint_over",
    // __paint_unpremultiply(a0 = dst, a1 = pixels above zero, a2 = `STRAIGHT`, 256 words).
    // Per colour channel: min(255, (c * STRAIGHT[a] + 2^15) >> 16); the product stays below
    // 2^32. An opaque or clear pixel's entry is 2^16, which leaves the channel; the alpha
    // byte is the source's.
    ".balign 4",
    ".globl __paint_unpremultiply",
    ".type __paint_unpremultiply, @function",
    "__paint_unpremultiply:",
    "li t2, 255",
    "li t3, 0x8000",
    "li t4, 0xff000000",
    "1:",
    "vsetvli t0, a1, e32, m4, ta, ma",
    "slli t1, t0, 2",
    "vsetvli x0, t1, e8, m4, ta, ma",
    "vle8.v v8, (a0)",
    "vsetvli x0, t0, e32, m4, ta, ma",
    // Each pixel's entry: its alpha as a byte offset into the table.
    "vsrl.vi v12, v8, 24",
    "vsll.vi v12, v12, 2",
    "vluxei32.v v16, (a2), v12",
    "vand.vx v20, v8, t4",
    // Blue.
    "vand.vx v24, v8, t2",
    "vmul.vv v24, v24, v16",
    "vadd.vx v24, v24, t3",
    "vsrl.vi v24, v24, 16",
    "vminu.vx v24, v24, t2",
    "vor.vv v20, v20, v24",
    // Green.
    "vsrl.vi v24, v8, 8",
    "vand.vx v24, v24, t2",
    "vmul.vv v24, v24, v16",
    "vadd.vx v24, v24, t3",
    "vsrl.vi v24, v24, 16",
    "vminu.vx v24, v24, t2",
    "vsll.vi v24, v24, 8",
    "vor.vv v20, v20, v24",
    // Red.
    "vsrl.vi v24, v8, 16",
    "vand.vx v24, v24, t2",
    "vmul.vv v24, v24, v16",
    "vadd.vx v24, v24, t3",
    "vsrl.vi v24, v24, 16",
    "vminu.vx v24, v24, t2",
    "vsll.vi v24, v24, 16",
    "vor.vv v20, v20, v24",
    "vsetvli x0, t1, e8, m4, ta, ma",
    "vse8.v v20, (a0)",
    "add a0, a0, t1",
    "sub a1, a1, t0",
    "bnez a1, 1b",
    "ret",
    ".size __paint_unpremultiply, . - __paint_unpremultiply",
    ".option pop",
    ".popsection",
);

extern "C" {
    fn __paint_fill(dst: *mut u8, bytes: usize, px: u32);
    fn __paint_blend(dst: *mut u8, bytes: usize, inv: usize, shares: u64);
    fn __paint_cover(dst: *mut u8, pixels: usize, alpha: *const u8, bgr: u32);
    fn __paint_over(dst: *mut u8, pixels: usize, src: *const u8);
    fn __paint_unpremultiply(dst: *mut u8, pixels: usize, straight: *const u32);
}

/// [`crate::reference::cover`] on the vector unit.
pub(crate) fn cover(row: &mut [u8], bgr: [u8; 3], alpha: &[u8]) {
    let pixels = (row.len() / 4).min(alpha.len());
    if pixels == 0 {
        return;
    }
    let colour = u32::from_le_bytes([bgr[0], bgr[1], bgr[2], 0]);
    // SAFETY: reads and writes exactly `4 · pixels` bytes from the start of `row` and reads
    // `pixels` bytes from the start of `alpha`; both hold at least that many (the minimum
    // above), `row` is borrowed exclusively and so cannot overlap `alpha`, and the function keeps
    // nothing. The vector unit was chosen from the kernel's report (see `fill`).
    unsafe { __paint_cover(row.as_mut_ptr(), pixels, alpha.as_ptr(), colour) }
}

/// [`crate::reference::over`] on the vector unit.
pub(crate) fn over(row: &mut [u8], src: &[u8]) {
    let pixels = row.len().min(src.len()) / 4;
    if pixels == 0 {
        return;
    }
    // SAFETY: reads and writes exactly `4 · pixels` bytes from the start of `row` and reads as
    // many from the start of `src`; both hold at least that many, `row` is borrowed exclusively
    // and so cannot overlap `src`, and the function keeps nothing (see `fill`).
    unsafe { __paint_over(row.as_mut_ptr(), pixels, src.as_ptr()) }
}

/// [`crate::reference::unpremultiply`] on the vector unit.
pub(crate) fn unpremultiply(span: &mut [u8]) {
    let pixels = span.len() / 4;
    if pixels == 0 {
        return;
    }
    // SAFETY: reads and writes exactly `4 · pixels` bytes from the start of `span`, which holds
    // them and is borrowed exclusively. It reads the table only at byte offsets `4 · a` for an
    // alpha byte `a` ≤ 255, inside the 256 words of `STRAIGHT`, a `static` that outlives the call
    // and is never written. The function keeps nothing (see `fill`).
    unsafe { __paint_unpremultiply(span.as_mut_ptr(), pixels, STRAIGHT.as_ptr()) }
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
