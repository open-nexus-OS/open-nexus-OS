// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: deflate's fixed Huffman code (RFC 1951 §3.2.6) as bit patterns ready for an
//! LSB-first bit writer, and the length/distance symbols of §3.2.5. The symbols are computed
//! arithmetically (no table lookups that could go out of bounds); the tests check them
//! exhaustively against the RFC's base/extra-bits tables.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (canonical code rebuild, every length 3..=258, every
//! distance 1..=32768)

/// A Huffman code of `len` bits, already bit-reversed for LSB-first packing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Code {
    pub(crate) bits: u32,
    pub(crate) len: u32,
}

/// The codes of the literal bytes 0..=255 (built at compile time, read-only data).
pub(crate) static LITERALS: [Code; 256] = literal_table();

/// End of block (symbol 256): seven zero bits.
pub(crate) const END_OF_BLOCK: Code = lit_len(256);

/// Huffman codes are defined most-significant bit first, deflate packs bits least
/// significant first: reverse the code within its length (1..=32 bits).
const fn reversed(code: u32, len: u32) -> u32 {
    code.reverse_bits() >> (32 - len)
}

/// The fixed code of literal/length symbol `sym` (0..=287).
pub(crate) const fn lit_len(sym: u32) -> Code {
    let (code, len) = if sym < 144 {
        (0x30 + sym, 8)
    } else if sym < 256 {
        (0x190 + (sym - 144), 9)
    } else if sym < 280 {
        (sym - 256, 7)
    } else {
        (0xC0 + (sym - 280), 8)
    };
    Code { bits: reversed(code, len), len }
}

const fn literal_table() -> [Code; 256] {
    let mut table = [Code { bits: 0, len: 0 }; 256];
    let mut i = 0;
    while i < 256 {
        table[i] = lit_len(i as u32);
        i += 1;
    }
    table
}

/// Length 3..=258 as (symbol 257..=285, extra-bits value, extra-bit count).
pub(crate) fn length_symbol(len: u32) -> (u32, u32, u32) {
    if len >= 258 {
        return (285, 0, 0);
    }
    let y = len.saturating_sub(3);
    if y < 8 {
        return (257 + y, 0, 0);
    }
    // From 11 on, four symbols per power of two; each carries `log - 2` extra bits.
    let log = y.ilog2();
    let extra = log - 2;
    (257 + 4 * (log - 1) + ((y >> extra) & 3), y & ((1 << extra) - 1), extra)
}

/// Distance 1..=32768 as (symbol 0..=29, extra-bits value, extra-bit count).
pub(crate) fn distance_symbol(dist: u32) -> (u32, u32, u32) {
    let x = dist.saturating_sub(1);
    if x < 4 {
        return (x, 0, 0);
    }
    // From 5 on, two symbols per power of two; each carries `log - 1` extra bits.
    let log = x.ilog2();
    let extra = log - 1;
    (2 * log + ((x >> extra) & 1), x & ((1 << extra) - 1), extra)
}

/// A whole match as one bit run: length code, its extra bits, the 5-bit distance code, its
/// extra bits. At most 8 + 5 + 5 + 13 = 31 bits.
pub(crate) fn match_bits(len: u32, dist: u32) -> (u64, u32) {
    let (lsym, lextra, lextra_bits) = length_symbol(len);
    let (dsym, dextra, dextra_bits) = distance_symbol(dist);
    let lcode = lit_len(lsym);
    let mut value = u64::from(lcode.bits);
    let mut n = lcode.len;
    value |= u64::from(lextra) << n;
    n += lextra_bits;
    value |= u64::from(reversed(dsym, 5)) << n;
    n += 5;
    value |= u64::from(dextra) << n;
    n += dextra_bits;
    (value, n)
}

/// RFC 1951 §3.2.5, verbatim: the reference the arithmetic above is tested against (and the
/// test inflater decodes with).
#[cfg(test)]
pub(crate) const LEN_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
#[cfg(test)]
pub(crate) const LEN_EXTRA: [u32; 29] =
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
#[cfg(test)]
pub(crate) const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
#[cfg(test)]
pub(crate) const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The lengths RFC 1951 §3.2.6 assigns to the 288 literal/length symbols.
    fn fixed_length(sym: u32) -> u32 {
        match sym {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        }
    }

    #[test]
    fn literal_codes_equal_the_canonical_code_of_the_rfc_lengths() {
        // Rebuild the canonical code from the code lengths (RFC 1951 §3.2.2) and compare
        // every symbol, reversed for LSB-first packing.
        let mut count = [0u32; 10];
        for sym in 0..288 {
            count[fixed_length(sym) as usize] += 1;
        }
        let mut next = [0u32; 10];
        let mut code = 0;
        for bits in 1..10 {
            code = (code + count[bits - 1]) << 1;
            next[bits] = code;
        }
        for sym in 0..288u32 {
            let len = fixed_length(sym);
            let canonical = next[len as usize];
            next[len as usize] += 1;
            let expected = Code { bits: reversed(canonical, len), len };
            assert_eq!(lit_len(sym), expected, "symbol {sym}");
            if sym < 256 {
                assert_eq!(LITERALS[sym as usize], expected, "literal {sym}");
            }
        }
        assert_eq!(END_OF_BLOCK, Code { bits: 0, len: 7 });
    }

    #[test]
    fn every_length_maps_to_its_rfc_symbol() {
        for len in 3..=258u32 {
            let (sym, value, bits) = length_symbol(len);
            let i = (sym - 257) as usize;
            assert!((257..=285).contains(&sym), "length {len}");
            assert_eq!(bits, LEN_EXTRA[i], "length {len}");
            assert!(value < 1 << bits, "length {len}");
            assert_eq!(LEN_BASE[i] + value, len, "length {len}");
        }
        // 258 has its own symbol; 284 + 31 extra would be the non-canonical spelling.
        assert_eq!(length_symbol(258), (285, 0, 0));
        assert_eq!(length_symbol(257), (284, 30, 5));
    }

    #[test]
    fn every_distance_maps_to_its_rfc_symbol() {
        for dist in 1..=32_768u32 {
            let (sym, value, bits) = distance_symbol(dist);
            let i = sym as usize;
            assert!(sym < 30, "distance {dist}");
            assert_eq!(bits, DIST_EXTRA[i], "distance {dist}");
            assert!(value < 1 << bits, "distance {dist}");
            assert_eq!(DIST_BASE[i] + value, dist, "distance {dist}");
        }
    }

    #[test]
    fn match_bits_concatenate_the_four_fields() {
        // Length 258 (symbol 285: 8 bits 11000101), distance 1 (symbol 0: 5 bits 00000).
        let (value, n) = match_bits(258, 1);
        assert_eq!(n, 13);
        assert_eq!(value, u64::from(reversed(0xC5, 8)));
        // Longest run: length 257 (8 + 5 bits) at distance 32768 (5 + 13 bits).
        assert_eq!(match_bits(257, 32_768).1, 31);
    }
}
