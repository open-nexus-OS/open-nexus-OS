// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Per-pixel alpha compositing — the single blend math every fill, blit, and
//! cursor path shares.
//!
//! Both use the fixed-point `(x * 257 + 32768) >> 16` approximation of `x / 255`
//! (257/65536 ≈ 1/255 with < 0.002 % error and correct 8-bit rounding), so the
//! reference backend and the live GPU driver composite bit-for-bit identically.
//!
//! The math lives in the pure `*_px` cores (`[u8; 4] → [u8; 4]`); the slice
//! helpers wrap them for the in-crate rasterizer, while the GPU driver's
//! `*mut u8` / `write_volatile` VMO path calls the same cores directly.

#![forbid(unsafe_code)]

/// `1/255 ≈ 257/65536` with round-to-nearest (`+ 32768` before the `>> 16`).
#[inline]
fn div255(numer: u32) -> u32 {
    (numer * 257 + 32768) >> 16
}

/// Straight-alpha source-over of one pixel: `out = src·a + dst·(1 - a)`,
/// accumulating the destination's own coverage in the alpha channel (the correct
/// over-operator, not a clamp of `dst_a + src_a`).
#[inline]
#[must_use]
pub fn blend_over_px(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let alpha = src[3] as u32;
    if alpha == 0 {
        return dst;
    }
    if alpha >= 255 {
        return src;
    }
    let inv = 255 - alpha;
    [
        div255(alpha * src[0] as u32 + inv * dst[0] as u32) as u8,
        div255(alpha * src[1] as u32 + inv * dst[1] as u32) as u8,
        div255(alpha * src[2] as u32 + inv * dst[2] as u32) as u8,
        src[3].saturating_add(div255(inv * dst[3] as u32) as u8),
    ]
}

/// Premultiplied-alpha source-over of one pixel: `out = src + dst·(1 - a)`.
/// Source colour channels are assumed already multiplied by `a` (e.g. an SVG
/// cursor sprite). Returns an opaque destination alpha.
#[inline]
#[must_use]
pub fn blend_premultiplied_px(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let inv = 255u32 - src[3] as u32;
    [
        (src[0] as u32 + div255(inv * dst[0] as u32)).min(255) as u8,
        (src[1] as u32 + div255(inv * dst[1] as u32)).min(255) as u8,
        (src[2] as u32 + div255(inv * dst[2] as u32)).min(255) as u8,
        255,
    ]
}

/// Straight-alpha source-over into a BGRA8888 slice at byte offset `idx`.
#[inline]
pub fn blend_over(buf: &mut [u8], idx: usize, src: &[u8; 4]) {
    if idx + 4 > buf.len() || src[3] == 0 {
        return;
    }
    let dst = [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]];
    buf[idx..idx + 4].copy_from_slice(&blend_over_px(dst, *src));
}

/// Premultiplied-alpha source-over into a BGRA8888 slice at byte offset `idx`.
#[inline]
pub fn blend_premultiplied(buf: &mut [u8], idx: usize, src: &[u8; 4]) {
    if idx + 4 > buf.len() {
        return;
    }
    let dst = [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]];
    buf[idx..idx + 4].copy_from_slice(&blend_premultiplied_px(dst, *src));
}
