// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the zlib stream of the image data (RFC 1950/1951). A greedy LZ77 over a 32 KiB
//! sliding window with ONE hash head per 3-byte hash (4096 heads, no chains): at each byte
//! the newest earlier position with the same hash is the only candidate; a match of 3..=258
//! bytes is taken, anything else is a literal. Every symbol uses deflate's fixed Huffman
//! code, all in one final block (BTYPE=01), behind the zlib header 0x78 0x01 and ahead of
//! the Adler-32 of the uncompressed bytes. Window and heads are allocated once
//! (`Deflater::new`) and reused for every stream.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (inflated by the test-only `inflate_check` decoder: edge
//! sizes, slides, out-of-reach repeats, split-independent output) + tests/encode

use alloc::boxed::Box;

use crate::adler::Adler32;
use crate::fixed::{match_bits, END_OF_BLOCK, LITERALS};
use crate::idat::ChunkWriter;
use crate::scratch::zeroed_array;
use crate::sink::Sink;
use crate::EncodeError;

/// Deflate's largest distance: how far back a match may reach.
const WSIZE: usize = 32 * 1024;
/// History plus room for new input. Input lands at `end`; when the buffer is full the older
/// half is dropped (`slide`), so a match is always one contiguous slice compare.
pub(crate) const WINDOW_LEN: usize = 2 * WSIZE;
const HASH_BITS: u32 = 12;
pub(crate) const HASH_LEN: usize = 1 << HASH_BITS;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// Input held back until `finish`: a maximal match plus the bytes that hash its last
/// position, so every decision sees the same bytes however the input was split.
const LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH;

/// CM 8 (deflate), CINFO 7 (32 KiB window), FLEVEL 0 (fastest), FCHECK making it % 31 == 0.
const ZLIB_HEADER: [u8; 2] = [0x78, 0x01];
/// BFINAL = 1, then BTYPE = 01 (fixed Huffman), packed LSB first.
const FINAL_FIXED_BLOCK: u64 = 0b011;

/// What a slide subtracts from every head slot.
const WSIZE_SLOT: u16 = WSIZE as u16;

const _: () = assert!((((ZLIB_HEADER[0] as u32) << 8) | ZLIB_HEADER[1] as u32) % 31 == 0);
// Head slots hold `window index + 1` in a u16, with 0 meaning "none".
const _: () = assert!(WINDOW_LEN <= u16::MAX as usize + 1);
// When the window is full, `compress` has left fewer than LOOKAHEAD bytes unencoded, so the
// cursor is past WSIZE and a slide never drops input that was not encoded yet.
const _: () = assert!(WINDOW_LEN - LOOKAHEAD >= WSIZE);

pub(crate) struct Deflater {
    window: Box<[u8; WINDOW_LEN]>,
    /// Per hash: the newest window position with that hash, plus 1; 0 = none.
    head: Box<[u16; HASH_LEN]>,
    /// Next byte to encode (window index).
    pos: usize,
    /// End of the buffered input (window index).
    end: usize,
    adler: Adler32,
}

impl Deflater {
    /// Heap bytes a deflater holds.
    pub(crate) const SCRATCH_BYTES: usize = WINDOW_LEN + HASH_LEN * 2;

    pub(crate) fn new() -> Result<Self, EncodeError> {
        Ok(Self {
            window: zeroed_array()?,
            head: zeroed_array()?,
            pos: 0,
            end: 0,
            adler: Adler32::new(),
        })
    }

    /// Zeroes the window and the heads: the previous stream's bytes are gone.
    pub(crate) fn scrub(&mut self) {
        self.window.fill(0);
        self.head.fill(0);
        self.pos = 0;
        self.end = 0;
        self.adler = Adler32::new();
    }

    /// Begins a stream: empty heads (a position of an earlier stream must never be matched)
    /// and the zlib header + the header of the one fixed-Huffman block.
    pub(crate) fn start<S: Sink + ?Sized>(
        &mut self,
        out: &mut ChunkWriter<'_, S>,
    ) -> Result<(), S::Error> {
        self.head.fill(0);
        self.pos = 0;
        self.end = 0;
        self.adler = Adler32::new();
        out.bytes(&ZLIB_HEADER)?;
        out.bits(FINAL_FIXED_BLOCK, 3)
    }

    /// Takes uncompressed bytes and encodes all of them that have enough lookahead.
    pub(crate) fn write<S: Sink + ?Sized>(
        &mut self,
        mut data: &[u8],
        out: &mut ChunkWriter<'_, S>,
    ) -> Result<(), S::Error> {
        self.adler.update(data);
        while !data.is_empty() {
            if self.end >= WINDOW_LEN {
                self.slide();
            }
            let free = self.window.get_mut(self.end..).unwrap_or_default();
            let n = free.len().min(data.len());
            let (now, rest) = data.split_at(n);
            if let Some(dst) = free.get_mut(..n) {
                dst.copy_from_slice(now);
            }
            self.end += n;
            data = rest;
            self.compress(out, false)?;
        }
        Ok(())
    }

    /// Encodes the held-back tail, ends the block, aligns, appends the Adler-32.
    pub(crate) fn finish<S: Sink + ?Sized>(
        &mut self,
        out: &mut ChunkWriter<'_, S>,
    ) -> Result<(), S::Error> {
        self.compress(out, true)?;
        out.bits(u64::from(END_OF_BLOCK.bits), END_OF_BLOCK.len)?;
        out.bytes(&self.adler.finish().to_be_bytes())
    }

    /// Drops the older half of a full window; head slots pointing into it become empty.
    fn slide(&mut self) {
        self.window.copy_within(WSIZE.., 0);
        self.pos = self.pos.saturating_sub(WSIZE);
        self.end = self.end.saturating_sub(WSIZE);
        for slot in self.head.iter_mut() {
            *slot = slot.saturating_sub(WSIZE_SLOT);
        }
    }

    /// Greedy LZ77 from `pos`: while `need` bytes are buffered (all of them when `last`).
    fn compress<S: Sink + ?Sized>(
        &mut self,
        out: &mut ChunkWriter<'_, S>,
        last: bool,
    ) -> Result<(), S::Error> {
        let need = if last { 1 } else { LOOKAHEAD };
        let window: &[u8; WINDOW_LEN] = &self.window;
        let head: &mut [u16; HASH_LEN] = &mut self.head;
        let end = self.end;
        let mut pos = self.pos;
        while end.saturating_sub(pos) >= need {
            // Look the position up and make it the newest one for its hash.
            let found = hash_at(window, pos, end)
                .and_then(|h| head.get_mut(h))
                .and_then(|s| match_at(window, core::mem::replace(s, slot(pos)), pos, end));
            if let Some((len, dist)) = found {
                // len <= MAX_MATCH and dist <= WSIZE: both fit u32 losslessly.
                let (bits, n) = match_bits(len as u32, dist as u32);
                out.bits(bits, n)?;
                // Every position inside the match becomes a candidate for later bytes.
                for p in pos + 1..pos + len {
                    if let Some(s) = hash_at(window, p, end).and_then(|h| head.get_mut(h)) {
                        *s = slot(p);
                    }
                }
                pos += len;
            } else {
                let code = LITERALS[usize::from(window.get(pos).copied().unwrap_or(0))];
                out.bits(u64::from(code.bits), code.len)?;
                pos += 1;
            }
        }
        self.pos = pos;
        Ok(())
    }
}

/// The head slot of the 3 bytes at `p`, when all three are buffered (multiplicative hash,
/// top `HASH_BITS` bits, so the slot is always in range).
fn hash_at(window: &[u8; WINDOW_LEN], p: usize, end: usize) -> Option<usize> {
    if end.saturating_sub(p) < MIN_MATCH {
        return None;
    }
    match window.get(p..p + MIN_MATCH) {
        Some(&[a, b, c]) => {
            let v = u32::from(a) | (u32::from(b) << 8) | (u32::from(c) << 16);
            Some((v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize)
        }
        _ => None,
    }
}

/// The head value for window position `p` (0 is "none"; `p + 1` always fits, see the
/// assertion on `WINDOW_LEN`).
fn slot(p: usize) -> u16 {
    u16::try_from(p + 1).unwrap_or(0)
}

/// The match between `pos` and the slot's position: `(length, distance)` when the slot is
/// set, within reach, and the bytes agree for at least `MIN_MATCH`.
fn match_at(
    window: &[u8; WINDOW_LEN],
    slot: u16,
    pos: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let candidate = usize::from(slot).checked_sub(1)?;
    let dist = pos.checked_sub(candidate).filter(|d| (1..=WSIZE).contains(d))?;
    let max = end.saturating_sub(pos).min(MAX_MATCH);
    let len = common_prefix(window.get(candidate..candidate + max)?, window.get(pos..pos + max)?);
    (len >= MIN_MATCH).then_some((len, dist))
}

/// How many leading bytes two equal-length slices share, compared eight at a time.
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let mut words_a = a.chunks_exact(8);
    let mut words_b = b.chunks_exact(8);
    let mut n = 0;
    for (x, y) in (&mut words_a).zip(&mut words_b) {
        let diff = word(x) ^ word(y);
        if diff != 0 {
            // Little-endian words: the lowest set bit is in the first differing byte.
            return n + (diff.trailing_zeros() / 8) as usize;
        }
        n += 8;
    }
    n + words_a.remainder().iter().zip(words_b.remainder()).take_while(|(x, y)| x == y).count()
}

fn word(bytes: &[u8]) -> u64 {
    <[u8; 8]>::try_from(bytes).map_or(0, u64::from_le_bytes)
}

#[cfg(test)]
mod tests;
