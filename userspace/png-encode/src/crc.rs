// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CRC-32 as PNG chunks carry it (ISO-HDLC: reflected polynomial 0xEDB88320,
//! initial value and final XOR 0xFFFFFFFF), one table lookup per byte. The table is built
//! at compile time and lives in read-only data, not on the heap.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (check value, IEND constant, split updates)

/// Remainders of every byte value, LSB-first.
static TABLE: [u32; 256] = table();

const fn table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 0 { c >> 1 } else { 0xEDB8_8320 ^ (c >> 1) };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

/// A running CRC-32 over bytes fed in any number of pieces.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Crc32(u32);

impl Crc32 {
    pub(crate) const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        let mut c = self.0;
        for &b in bytes {
            // The low byte of `c` XOR the input selects the remainder; a `u8` index into a
            // 256-entry table cannot go out of bounds.
            c = TABLE[usize::from(c.to_le_bytes()[0] ^ b)] ^ (c >> 8);
        }
        self.0 = c;
    }

    pub(crate) const fn finish(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

/// The CRC-32 of `bytes` in one call.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.finish()
}

#[cfg(test)]
mod tests {
    use super::{crc32, Crc32};

    #[test]
    fn check_value_matches_the_catalogue() {
        // The CRC-32/ISO-HDLC check value (CRC of the ASCII digits 1..9).
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn iend_chunk_crc_is_the_well_known_constant() {
        // Every PNG ends with IEND, whose CRC covers only the type.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn split_updates_equal_one_shot() {
        let data: Vec<u8> =
            (0..10_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        for split in [0, 1, 7, 4096, 9_999, 10_000] {
            let (a, b) = data.split_at(split);
            let mut crc = Crc32::new();
            crc.update(a);
            crc.update(b);
            assert_eq!(crc.finish(), crc32(&data), "split at {split}");
        }
    }
}
