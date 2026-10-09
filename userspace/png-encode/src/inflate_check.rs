// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: test-only. A minimal inflater for zlib streams of fixed-Huffman blocks, written
//! from RFC 1950/1951 and decoding with the RFC's own base tables (not the encoder's
//! arithmetic), so the unit tests can check the deflate stream and its Adler-32 byte for
//! byte without a dependency. Rejects what this crate never emits: stored or dynamic
//! blocks, preset dictionaries, distances before the start, trailing bytes.
//! OWNERS: @ui
//! STATUS: Functional (test support)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: used by src/deflate/tests.rs and src/encoder.rs tests

use alloc::vec::Vec;

use crate::adler::adler32;
use crate::fixed::{DIST_BASE, DIST_EXTRA, LEN_BASE, LEN_EXTRA};

struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl BitReader<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.bit / 8)?;
        let b = (byte >> (self.bit % 8)) & 1;
        self.bit += 1;
        Some(u32::from(b))
    }

    /// `n` bits, least significant first (header fields, extra bits).
    fn value(&mut self, n: u32) -> Option<u32> {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Some(v)
    }

    /// `n` more bits of a Huffman code after `prefix`, most significant first.
    fn code(&mut self, mut prefix: u32, n: u32) -> Option<u32> {
        for _ in 0..n {
            prefix = (prefix << 1) | self.bit()?;
        }
        Some(prefix)
    }
}

/// One literal/length symbol of the fixed code: 7, 8 or 9 bits (RFC 1951 §3.2.6).
fn lit_len(r: &mut BitReader<'_>) -> Option<u32> {
    let v = r.code(0, 7)?;
    if v <= 0b001_0111 {
        return Some(256 + v);
    }
    let v = r.code(v, 1)?;
    if (0x30..=0xBF).contains(&v) {
        return Some(v - 0x30);
    }
    if (0xC0..=0xC7).contains(&v) {
        return Some(280 + v - 0xC0);
    }
    let v = r.code(v, 1)?;
    (0x190..=0x1FF).contains(&v).then(|| 144 + v - 0x190)
}

/// The bytes of a zlib stream, or `None` for anything malformed or unexpected.
pub(crate) fn inflate_zlib(data: &[u8]) -> Option<Vec<u8>> {
    let (&cmf, &flg) = (data.first()?, data.get(1)?);
    let check = ((u32::from(cmf) << 8) | u32::from(flg)) % 31;
    if cmf != 0x78 || flg & 0x20 != 0 || check != 0 {
        return None;
    }
    let mut r = BitReader { data: data.get(2..)?, bit: 0 };
    let mut out = Vec::new();
    loop {
        let last = r.value(1)?;
        if r.value(2)? != 1 {
            return None;
        }
        loop {
            let sym = lit_len(&mut r)?;
            match sym {
                0..=255 => out.push(u8::try_from(sym).ok()?),
                256 => break,
                _ => {
                    let i = usize::try_from(sym - 257).ok()?;
                    let len = LEN_BASE.get(i)? + r.value(*LEN_EXTRA.get(i)?)?;
                    let d = usize::try_from(r.code(0, 5)?).ok()?;
                    let dist = DIST_BASE.get(d)? + r.value(*DIST_EXTRA.get(d)?)?;
                    let dist = usize::try_from(dist).ok()?;
                    if dist > out.len() || len > 258 {
                        return None;
                    }
                    for _ in 0..len {
                        let byte = *out.get(out.len() - dist)?;
                        out.push(byte);
                    }
                }
            }
        }
        if last == 1 {
            break;
        }
    }
    let trailer: [u8; 4] = data.get(2 + r.bit.div_ceil(8)..)?.try_into().ok()?;
    (u32::from_be_bytes(trailer) == adler32(&out)).then_some(out)
}
