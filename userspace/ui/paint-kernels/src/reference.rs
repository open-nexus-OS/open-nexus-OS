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
