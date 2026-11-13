// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The proof a machine runs (ADR-0099 rule 4): whatever [`crate::choose`] left, the kernels as
//! a program calls them against the scalar reference, over spans no caller picks — any length
//! up to 1 100 bytes, whole pixels or not, starting at any byte offset, over random pixels,
//! every alpha among the colours. The spans are the same on every machine, so the digest of
//! what the kernels left is one number ([`DIGEST`], taken from the reference on the host): a
//! machine whose vector unit prints it painted what the host's scalar form paints.
//!
//! One definition for all three who run it: the host test of this crate, the measurement
//! program on the stock system and under the user-mode emulator at several register widths,
//! and `SELFTEST: paint kernels ok` on our kernel.

use crate::reference;

/// How many spans a run paints, half of them with each kernel.
pub const SPANS: u32 = 8192;

/// The digest of a run — every span as the scalar reference leaves it.
pub const DIGEST: u64 = 0x287e_0815_b292_fd61;

/// The first span a kernel painted differently from its reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Differs {
    /// Which span of the run.
    pub span: u32,
    /// The kernel: `true` the blend, `false` the fill.
    pub blend: bool,
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
}

/// Paints [`SPANS`] spans with the kernels as chosen and with the reference; the digest of
/// what the kernels left, or the first span that differs — in the span or anywhere around it.
pub fn run() -> Result<u64, Differs> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut by_kernel = [0u8; 1200];
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    for span in 0..SPANS {
        for chunk in by_kernel.chunks_exact_mut(8) {
            chunk.copy_from_slice(&rng.next().to_le_bytes());
        }
        let mut by_reference = by_kernel;
        let r = rng.next();
        let (at, len) = ((r & 63) as usize, ((r >> 8) % 1101) as usize);
        let colour = ((r >> 24) as u32).to_le_bytes();
        let blend = span % 2 == 1;
        if blend {
            crate::blend(&mut by_kernel[at..at + len], colour);
            reference::blend(&mut by_reference[at..at + len], colour);
        } else {
            crate::fill(&mut by_kernel[at..at + len], colour);
            reference::fill(&mut by_reference[at..at + len], colour);
        }
        if by_kernel != by_reference {
            return Err(Differs { span, blend, at, len, colour });
        }
        for &b in &by_kernel[at..at + len] {
            digest = (digest ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(digest)
}
