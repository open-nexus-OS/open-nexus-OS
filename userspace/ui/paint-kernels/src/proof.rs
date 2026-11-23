// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The proof a machine runs (ADR-0099 rule 4): whatever [`crate::choose`] left, the kernels as
//! a program calls them against the scalar reference, over spans no caller picks — any length
//! up to 1 100 bytes, whole pixels or not, starting at any byte offset, over random pixels,
//! every alpha among the colours and the coverages; the per-pixel kernels' second row (the
//! alphas, the source pixels) starts at its own offset and may be shorter or longer than the
//! span; the straight-alpha spans mix clear, opaque and repeated pixels among partial ones.
//! The spans are the same on every machine, so the digest of what the kernels left is one
//! number ([`DIGEST`], taken from the reference on the host): a machine whose vector unit
//! prints it painted what the host's scalar form paints.
//!
//! One definition for all three who run it: the host test of this crate, the measurement
//! program on the stock system and under the user-mode emulator at several register widths,
//! and `SELFTEST: paint kernels ok` on our kernel.

use crate::reference;

/// How many spans a run paints, a fifth of them with each kernel.
pub const SPANS: u32 = 8192;

/// The digest of a run — every span as the scalar reference leaves it.
pub const DIGEST: u64 = 0xf8b7_1fee_8a99_c1b7;

/// A kernel of this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kernel {
    Fill,
    Blend,
    Cover,
    Over,
    Unpremultiply,
}

impl Kernel {
    /// The kernel a span of the run paints with.
    fn of(span: u32) -> Self {
        match span % 5 {
            0 => Self::Fill,
            1 => Self::Blend,
            2 => Self::Cover,
            3 => Self::Over,
            _ => Self::Unpremultiply,
        }
    }

    /// Its name as a machine prints it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Blend => "blend",
            Self::Cover => "cover",
            Self::Over => "over",
            Self::Unpremultiply => "unpremultiply",
        }
    }
}

/// The first span a kernel painted differently from its reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Differs {
    /// Which span of the run.
    pub span: u32,
    /// The kernel that painted it.
    pub kernel: Kernel,
    /// The span's first byte in the row and its length in bytes.
    pub at: usize,
    pub len: usize,
    /// The colour (blue, green, red, alpha).
    pub colour: [u8; 4],
}

/// xorshift64*: the same spans on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn fill(&mut self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_exact_mut(8) {
            chunk.copy_from_slice(&self.next().to_le_bytes());
        }
    }
}

/// Paints [`SPANS`] spans with the kernels as chosen and with the reference; the digest of
/// what the kernels left, or the first span that differs — in the span or anywhere around it.
pub fn run() -> Result<u64, Differs> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut by_kernel = [0u8; 1200];
    let mut second = [0u8; 1200];
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for span in 0..SPANS {
        rng.fill(&mut by_kernel);
        let r = rng.next();
        let (at, len) = ((r & 63) as usize, ((r >> 8) % 1101) as usize);
        let colour = ((r >> 24) as u32).to_le_bytes();
        let kernel = Kernel::of(span);
        // The second row: where it starts, and how many bytes it has (up to a few pixels more
        // or fewer than the span — a kernel paints as many pixels as both hold).
        let r2 = rng.next();
        let (from, extra) = ((r2 & 63) as usize, ((r2 >> 8) % 41) as usize);
        let other = (len + extra).saturating_sub(20).min(second.len() - from);
        if kernel == Kernel::Unpremultiply {
            mix_alphas(&mut rng, &mut by_kernel[at..at + len]);
        } else {
            rng.fill(&mut second);
        }
        let mut by_reference = by_kernel;
        let (row, reference_row) = (&mut by_kernel[at..at + len], &mut by_reference[at..at + len]);
        match kernel {
            Kernel::Fill => {
                crate::fill(row, colour);
                reference::fill(reference_row, colour);
            }
            Kernel::Blend => {
                crate::blend(row, colour);
                reference::blend(reference_row, colour);
            }
            Kernel::Cover => {
                // A quarter of the alphas whole and a quarter clear, as a shadow's and an
                // edge's rows have them.
                let alpha = &mut second[from..from + other / 4];
                for a in alpha.iter_mut() {
                    *a = match *a & 3 {
                        0 => 0,
                        1 => 255,
                        _ => *a,
                    };
                }
                let bgr = [colour[0], colour[1], colour[2]];
                crate::cover(row, bgr, alpha);
                reference::cover(reference_row, bgr, alpha);
            }
            Kernel::Over => {
                let src = &second[from..from + other];
                crate::over(row, src);
                reference::over(reference_row, src);
            }
            Kernel::Unpremultiply => {
                crate::unpremultiply(row);
                reference::unpremultiply(reference_row);
            }
        }
        if by_kernel != by_reference {
            return Err(Differs { span, kernel, at, len, colour });
        }
        for &b in &by_kernel[at..at + len] {
            digest = (digest ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(digest)
}

/// Makes a straight-alpha span what a painted row is: a quarter of its pixels clear, a quarter
/// opaque, a quarter repeating the pixel before (so the scalar form's remembered quad is met),
/// the rest partial.
fn mix_alphas(rng: &mut Rng, span: &mut [u8]) {
    let mut before = [0u8; 4];
    for px in span.chunks_exact_mut(4) {
        match rng.next() & 3 {
            0 => px[3] = 0,
            1 => px[3] = 255,
            2 => px.copy_from_slice(&before),
            _ => {}
        }
        before.copy_from_slice(px);
    }
}
