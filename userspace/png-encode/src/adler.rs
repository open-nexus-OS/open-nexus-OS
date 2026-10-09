// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Adler-32 (RFC 1950 §8.2), the checksum that closes a zlib stream, over the
//! uncompressed bytes. Sums are reduced once per `NMAX` bytes instead of per byte.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (reference vector, worst-case input against a per-byte
//! reference)

/// The largest prime below 2^16.
const MOD: u32 = 65_521;

/// The most bytes that can be summed before reducing without overflowing `u32`, starting
/// from reduced sums: 255·n(n+1)/2 + (n+1)(MOD−1) ≤ 2^32−1 (zlib's bound).
const NMAX: usize = 5552;

/// A running Adler-32 over bytes fed in any number of pieces.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Adler32 {
    a: u32,
    b: u32,
}

impl Adler32 {
    pub(crate) const fn new() -> Self {
        Self { a: 1, b: 0 }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        let (mut a, mut b) = (self.a, self.b);
        for chunk in bytes.chunks(NMAX) {
            for &x in chunk {
                a += u32::from(x);
                b += a;
            }
            a %= MOD;
            b %= MOD;
        }
        self.a = a;
        self.b = b;
    }

    pub(crate) const fn finish(self) -> u32 {
        (self.b << 16) | self.a
    }
}

#[cfg(test)]
pub(crate) fn adler32(bytes: &[u8]) -> u32 {
    let mut adler = Adler32::new();
    adler.update(bytes);
    adler.finish()
}

#[cfg(test)]
mod tests {
    use super::{adler32, Adler32, MOD};

    /// The definition, reduced after every byte.
    fn reference(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in bytes {
            a = (a + u32::from(x)) % MOD;
            b = (b + a) % MOD;
        }
        (b << 16) | a
    }

    #[test]
    fn reference_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn worst_case_bytes_never_overflow_and_match_the_definition() {
        // All-0xFF input maximises both sums between reductions.
        let ones = vec![0xFFu8; 3 * 5552 + 17];
        assert_eq!(adler32(&ones), reference(&ones));
        let mixed: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + i / 255) as u8).collect();
        assert_eq!(adler32(&mixed), reference(&mixed));
    }

    #[test]
    fn split_updates_equal_one_shot() {
        let data: Vec<u8> = (0..20_000u32).map(|i| (i ^ (i >> 3)) as u8).collect();
        for split in [0, 1, 5551, 5552, 5553, 19_999] {
            let (x, y) = data.split_at(split);
            let mut adler = Adler32::new();
            adler.update(x);
            adler.update(y);
            assert_eq!(adler.finish(), reference(&data), "split at {split}");
        }
    }
}
