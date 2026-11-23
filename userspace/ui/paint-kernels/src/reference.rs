// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scalar form of every kernel — what a kernel IS. A vector form is right when it leaves the
//! bytes this module leaves, for every input; the proofs compare against these functions and
//! nothing else.

/// One colour laid over a pixel, source over, in the arithmetic both forms share: a channel
/// becomes `(dst · inv + add) / 255`, rounded down.
///
/// `add` is the colour's share of each channel, `colour · alpha`; the alpha channel's colour is
/// 255, so `(dst · inv + 255 · alpha) / 255` is the `alpha + dst · inv / 255` a source-over
/// alpha is. With `inv = 255 − alpha` no sum passes `255 · 255`: it fits sixteen bits, which
/// is what lets a vector form work in sixteen-bit lanes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Over {
    /// `255 − alpha`: what stays of the destination.
    pub inv: u8,
    /// The colour's share of blue, green, red and alpha.
    pub add: [u16; 4],
}

impl Over {
    /// The straight colour `bgra` (blue, green, red, alpha) as a source-over step.
    #[must_use]
    pub fn of(bgra: [u8; 4]) -> Self {
        let a = u16::from(bgra[3]);
        Self {
            inv: 255 - bgra[3],
            add: [u16::from(bgra[0]) * a, u16::from(bgra[1]) * a, u16::from(bgra[2]) * a, 255 * a],
        }
    }

    /// One channel after the step.
    #[inline]
    #[must_use]
    pub fn channel(&self, dst: u8, k: usize) -> u8 {
        ((u32::from(dst) * u32::from(self.inv) + u32::from(self.add[k])) / 255) as u8
    }
}

/// Every whole pixel of `row` becomes `px`. Bytes past the last whole pixel stay.
pub fn fill(row: &mut [u8], px: [u8; 4]) {
    for dst in row.chunks_exact_mut(4) {
        dst.copy_from_slice(&px);
    }
}

/// The straight colour `bgra` source-over every whole pixel of `row` (premultiplied BGRA).
/// Bytes past the last whole pixel stay.
pub fn blend(row: &mut [u8], bgra: [u8; 4]) {
    let over = Over::of(bgra);
    for dst in row.chunks_exact_mut(4) {
        for (k, c) in dst.iter_mut().enumerate() {
            *c = over.channel(*c, k);
        }
    }
}

/// One colour `bgr` (blue, green, red) source-over the whole pixels of `row`, pixel `i` at the
/// alpha `alpha[i]` — the painter's covered pixel (`RowCanvas::blend` of the colour at the alpha
/// its coverage left), as many pixels as both hold. An alpha of 0 leaves the pixel.
pub fn cover(row: &mut [u8], bgr: [u8; 3], alpha: &[u8]) {
    for (dst, &a) in row.chunks_exact_mut(4).zip(alpha) {
        if a != 0 {
            let over = Over::of([bgr[0], bgr[1], bgr[2], a]);
            for (k, c) in dst.iter_mut().enumerate() {
                *c = over.channel(*c, k);
            }
        }
    }
}

/// The straight pixels of `src` — red, green, blue, alpha, a sprite's byte order
/// (`ShapeKind::Raster`) — source-over the whole BGRA pixels of `row`, pixel by pixel: the
/// painter's sprite blit (`RowCanvas::blend` of each source pixel), as many pixels as both hold.
/// A clear source pixel leaves its destination.
pub fn over(row: &mut [u8], src: &[u8]) {
    for (dst, s) in row.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        if s[3] != 0 {
            let over = Over::of([s[2], s[1], s[0], s[3]]);
            for (k, c) in dst.iter_mut().enumerate() {
                *c = over.channel(*c, k);
            }
        }
    }
}

/// What a colour channel is multiplied by to divide it out by alpha `a`: `255 · 2¹⁶ / a`,
/// rounded (`(c · STRAIGHT[a] + 2¹⁵) >> 16`, at most 255). An opaque or a clear pixel is its
/// own straight form: their entries are 2¹⁶, the multiply that leaves a channel — the scalar
/// form never asks them, the vector form multiplies every pixel by its entry.
pub static STRAIGHT: [u32; 256] = {
    let mut t = [0u32; 256];
    t[0] = 1 << 16;
    let mut a = 1;
    while a < 256 {
        t[a] = ((255u32 << 16) + (a as u32) / 2) / a as u32;
        a += 1;
    }
    t
};

/// Turns the whole pixels of a span of premultiplied BGRA into straight alpha, in place (app-host's
/// stage every row leaves through, RFC-0109 "The surface"). An opaque or a transparent pixel stays
/// as it is; four of them in a row are passed by one test, and four partial pixels equal to the
/// four last divided take their result (a glass tint is one pixel over a whole surface).
pub fn unpremultiply(span: &mut [u8]) {
    // The last quad divided and its result, as words: an equal quad is two compares and two
    // stores (a slice compare is a call per quad on the OS target).
    let (mut seen, mut straight, mut known) = (0u128, 0u128, false);
    let mut quads = span.chunks_exact_mut(16);
    for q in &mut quads {
        let Ok(q) = <&mut [u8; 16]>::try_from(q) else { continue };
        let (a0, a1, a2, a3) = (q[3], q[7], q[11], q[15]);
        if a0 & a1 & a2 & a3 == 255 || a0 | a1 | a2 | a3 == 0 {
            continue;
        }
        let word = u128::from_ne_bytes(*q);
        if known && word == seen {
            *q = straight.to_ne_bytes();
            continue;
        }
        for px in q.chunks_exact_mut(4) {
            divide(px);
        }
        (seen, straight, known) = (word, u128::from_ne_bytes(*q), true);
    }
    for px in quads.into_remainder().chunks_exact_mut(4) {
        divide(px);
    }
}

/// One pixel divided out by its alpha (`c · 255 / a`, rounded, by [`STRAIGHT`]).
#[inline]
fn divide(px: &mut [u8]) {
    let a = px[3];
    if a == 0 || a == 255 {
        return;
    }
    let r = STRAIGHT[usize::from(a)];
    for c in &mut px[..3] {
        *c = ((u32::from(*c) * r + (1 << 15)) >> 16).min(255) as u8;
    }
}
