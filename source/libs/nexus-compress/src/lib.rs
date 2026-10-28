// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! CONTEXT: The page compressor of the memory backing stages (RFC-0100 §5, TASK-0333 6f). Under
//! pressure an anonymous dirty page of a frozen process is squeezed into the kernel's pool and,
//! further down, written to the `swap` partition by memd; both sides speak this one block
//! format. It is the LZ4 block format (a token per sequence: literals, a 16-bit backwards
//! offset, a match of at least four bytes; the last sequence is literals only), chosen because
//! its decoder is a byte copier — a page comes back in a few microseconds on a fault path —
//! and because it needs no table: a blob is self-describing and the format is stable across
//! kernel versions and across a reboot that finds blobs on the swap partition.
//!
//! WHY BOUNDED: a blob read back from a partition or handed over by a service is untrusted
//! bytes. The decoder checks every length and offset against the buffers it was given — a
//! malformed blob is a typed error, never a read past the blob or a write past the page. The
//! encoder is allocation-free: its one hash table is the caller's (`Table`), so the kernel keeps
//! one per hart and a service one per thread, and an input bigger than the table addresses is
//! refused rather than truncated.
//!
//! WHAT IT IS NOT: no frame format, no checksum, no streaming — one page in, one blob out. The
//! blob's length is the caller's record (the pool's slot, the swap slot's header).
//!
//! OWNERS: @kernel-mm-team @runtime
//! STATUS: Functional
//! API_STABILITY: Stable within the workspace — the block format is the contract between the
//!   kernel's pool, memd's pager and the swap partition; a change bumps the pool's slot magic.
//! TEST_COVERAGE: unit tests below — round trips over every content class a page has (zeros,
//!   text, noise, runs, overlapping repeats), the length sweep, and the reject matrix of the
//!   decoder; `KSELFTEST: mem compress ok (` measures ratio and cost per page on QEMU and the
//!   board.

/// A page, the one input size the backing stages feed.
pub const PAGE: usize = 4096;

/// Why a compression or a decompression did not produce a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The output buffer is too small for the result (compress: use `max_compressed_len`;
    /// decompress: the blob describes more bytes than the page holds).
    OutputFull,
    /// The input is longer than the encoder's positions address (`MAX_INPUT`).
    InputTooLong,
    /// The blob ends inside a token, a length, an offset or a literal run.
    Truncated,
    /// A match points before the start of the output.
    OffsetOutOfRange,
    /// A match offset of zero — the format forbids it.
    ZeroOffset,
    /// The blob ends after a match where the format requires a final literal run.
    NoFinalLiterals,
}

/// The longest input one blob may describe: offsets and table positions are 16-bit.
pub const MAX_INPUT: usize = u16::MAX as usize;

/// A match is at least this long; shorter repeats are literals.
const MIN_MATCH: usize = 4;
/// The last bytes of the input are always literals (the decoder's copy needs no tail check).
const LAST_LITERALS: usize = 5;
/// A match may not start closer than this to the end of the input.
const MF_LIMIT: usize = 12;
/// Token nibble value meaning "a longer length follows in extra bytes".
const RUN_MASK: usize = 15;

/// Bits of the hash table index: 2048 heads for a 4 KiB page is a hit rate that costs the
/// ratio little and keeps the table at 4 KiB (one clear per page).
const HASH_BITS: u32 = 11;
const HASH_SIZE: usize = 1 << HASH_BITS;

/// The upper bound of a blob for `n` input bytes: the LZ4 worst case of all literals.
pub const fn max_compressed_len(n: usize) -> usize {
    n + n / 255 + 16
}

/// The encoder's one hash table — newest position per 4-byte hash, `0` meaning none. The caller
/// owns it (the kernel one per hart, a service one per thread); it is cleared per call so the
/// blob of an input never depends on what was compressed before it.
pub struct Table {
    heads: [u16; HASH_SIZE],
}

impl Table {
    /// An empty table.
    pub const fn new() -> Self {
        Self { heads: [0; HASH_SIZE] }
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

/// After this many failed probes the search steps two bytes, then three … (LZ4's skip): an
/// incompressible stretch costs a fraction of a probe per byte instead of one.
const SKIP_TRIGGER: u32 = 6;

#[inline]
fn read_u32(input: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(input[at..at + 4].try_into().unwrap_or([0; 4]))
}

#[inline]
fn hash(v: u32) -> usize {
    (v.wrapping_mul(2654435761) >> (32 - HASH_BITS)) as usize
}

/// How many leading bytes `a` and `b` share, `b` being the shorter (the match may not run into
/// the input's last literals); eight bytes a step, the differing byte found in the xor.
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let mut n = 0;
    while n + 8 <= b.len() {
        let x = u64::from_le_bytes(a[n..n + 8].try_into().unwrap_or([0; 8]));
        let y = u64::from_le_bytes(b[n..n + 8].try_into().unwrap_or([0; 8]));
        if x != y {
            return n + ((x ^ y).trailing_zeros() / 8) as usize;
        }
        n += 8;
    }
    while n < b.len() && a[n] == b[n] {
        n += 1;
    }
    n
}

/// Writes a length the LZ4 way: the token nibble holds up to 14, 15 means extra bytes of 255
/// each and one final byte. Returns the new write position.
fn put_length(out: &mut [u8], mut op: usize, mut len: usize) -> Result<usize, Error> {
    while len >= 255 {
        *out.get_mut(op).ok_or(Error::OutputFull)? = 255;
        op += 1;
        len -= 255;
    }
    *out.get_mut(op).ok_or(Error::OutputFull)? = len as u8;
    Ok(op + 1)
}

/// One sequence: `literals` then, unless it is the last, a match of `match_len` at `offset`.
fn put_sequence(
    out: &mut [u8],
    mut op: usize,
    literals: &[u8],
    tail: Option<(usize, usize)>,
) -> Result<usize, Error> {
    let lit = literals.len();
    let match_code = tail.map_or(0, |(_, len)| (len - MIN_MATCH).min(RUN_MASK));
    let token_at = op;
    op += 1;
    if lit >= RUN_MASK {
        op = put_length(out, op, lit - RUN_MASK)?;
    }
    out.get_mut(op..op + lit).ok_or(Error::OutputFull)?.copy_from_slice(literals);
    op += lit;
    *out.get_mut(token_at).ok_or(Error::OutputFull)? =
        ((lit.min(RUN_MASK) as u8) << 4) | match_code as u8;
    if let Some((offset, len)) = tail {
        let off = (offset as u16).to_le_bytes();
        out.get_mut(op..op + 2).ok_or(Error::OutputFull)?.copy_from_slice(&off);
        op += 2;
        if len - MIN_MATCH >= RUN_MASK {
            op = put_length(out, op, len - MIN_MATCH - RUN_MASK)?;
        }
    }
    Ok(op)
}

/// Compresses `input` into `out`, returning the blob's length. `out` of `max_compressed_len`
/// always fits; a smaller `out` (a page, when only a blob smaller than the page is worth
/// keeping) makes an incompressible input `OutputFull`, which the caller treats as "keep the
/// page as it is".
pub fn compress(input: &[u8], out: &mut [u8], table: &mut Table) -> Result<usize, Error> {
    if input.len() > MAX_INPUT {
        return Err(Error::InputTooLong);
    }
    table.heads.fill(0);
    let mut op = 0;
    let mut anchor = 0;
    if input.len() >= MF_LIMIT {
        let limit = input.len() - MF_LIMIT;
        let match_end = input.len() - LAST_LITERALS;
        let mut ip = 0;
        let mut probes = 1 << SKIP_TRIGGER;
        while ip <= limit {
            let here = read_u32(input, ip);
            let h = hash(here);
            let candidate = table.heads[h] as usize;
            table.heads[h] = (ip + 1) as u16;
            if candidate == 0 || read_u32(input, candidate - 1) != here {
                ip += (probes >> SKIP_TRIGGER) as usize;
                probes += 1;
                continue;
            }
            let candidate = candidate - 1;
            let len = MIN_MATCH
                + common_prefix(&input[candidate + MIN_MATCH..], &input[ip + MIN_MATCH..match_end]);
            op = put_sequence(out, op, &input[anchor..ip], Some((ip - candidate, len)))?;
            ip += len;
            anchor = ip;
            probes = 1 << SKIP_TRIGGER;
        }
    }
    put_sequence(out, op, &input[anchor..], None)
}

/// Reads a length's extra bytes after a nibble of 15. Returns `(length, position)`.
fn get_length(blob: &[u8], mut ip: usize, mut len: usize) -> Result<(usize, usize), Error> {
    loop {
        let b = *blob.get(ip).ok_or(Error::Truncated)?;
        ip += 1;
        len += b as usize;
        if b != 255 {
            return Ok((len, ip));
        }
    }
}

/// Decompresses `blob` into `out`, returning the number of bytes written. Every length and
/// offset is checked against both buffers; a malformed blob is an error and `out` is then
/// partially written and must not be used. The format has no length header, so a blob cut
/// after a literal run decodes to fewer bytes: the caller compares the returned length with
/// the length it recorded (a page).
pub fn decompress(blob: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    let mut ip = 0;
    let mut op = 0;
    loop {
        let token = *blob.get(ip).ok_or(Error::Truncated)? as usize;
        ip += 1;
        let mut lit = token >> 4;
        if lit == RUN_MASK {
            (lit, ip) = get_length(blob, ip, lit)?;
        }
        let src = blob.get(ip..ip + lit).ok_or(Error::Truncated)?;
        out.get_mut(op..op + lit).ok_or(Error::OutputFull)?.copy_from_slice(src);
        ip += lit;
        op += lit;
        if ip == blob.len() {
            return Ok(op);
        }
        let off = blob.get(ip..ip + 2).ok_or(Error::Truncated)?;
        let offset = u16::from_le_bytes([off[0], off[1]]) as usize;
        ip += 2;
        if offset == 0 {
            return Err(Error::ZeroOffset);
        }
        if offset > op {
            return Err(Error::OffsetOutOfRange);
        }
        let mut len = token & RUN_MASK;
        if len == RUN_MASK {
            (len, ip) = get_length(blob, ip, len)?;
        }
        len += MIN_MATCH;
        if op + len > out.len() {
            return Err(Error::OutputFull);
        }
        // `[base, op + done)` is periodic with period `offset` once the first `offset` bytes
        // are copied, so each round may copy up to `offset + done` bytes from `base` without
        // the source reaching into what it writes: a doubling copy, a run of one byte included.
        let base = op - offset;
        let mut done = 0;
        while done < len {
            let chunk = (len - done).min(offset + done);
            out.copy_within(base..base + chunk, op + done);
            done += chunk;
        }
        op += len;
        if ip >= blob.len() {
            return Err(Error::NoFinalLiterals);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(input: &[u8]) -> usize {
        let mut table = Table::new();
        let mut blob = vec![0u8; max_compressed_len(input.len())];
        let n = compress(input, &mut blob, &mut table).expect("compress");
        let mut back = vec![0u8; input.len()];
        let m = decompress(&blob[..n], &mut back).expect("decompress");
        assert_eq!(m, input.len());
        assert_eq!(&back[..], input);
        n
    }

    fn noise(len: usize, mut seed: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn zero_page_shrinks_to_a_few_bytes() {
        let n = round_trip(&[0u8; PAGE]);
        assert!(n < 32, "zero page blob {n}");
    }

    #[test]
    fn text_page_shrinks_below_half() {
        let text = b"the quick brown fox jumps over the lazy dog; the dog sleeps. ";
        let page: Vec<u8> = text.iter().cycle().take(PAGE).copied().collect();
        let n = round_trip(&page);
        assert!(n < PAGE / 2, "text blob {n}");
    }

    #[test]
    fn noise_page_round_trips_within_the_bound() {
        let page = noise(PAGE, 0x9e37_79b9_7f4a_7c15);
        let n = round_trip(&page);
        assert!(n <= max_compressed_len(PAGE));
        assert!(n >= PAGE, "noise must not shrink: {n}");
    }

    #[test]
    fn overlapping_repeat_and_long_runs_round_trip() {
        let mut page = vec![0u8; PAGE];
        for (i, b) in page.iter_mut().enumerate() {
            *b = (i % 3) as u8; // offset 3 < match length: the overlapping copy
        }
        round_trip(&page);
        let mut mixed = noise(PAGE, 7);
        mixed[1000..3000].fill(0xAB); // a run longer than 255 + a long literal run
        round_trip(&mixed);
    }

    #[test]
    fn every_length_round_trips() {
        for len in 0..=300usize {
            round_trip(&noise(len, len as u64 + 1));
            let runs: Vec<u8> = (0..len).map(|i| (i / 7) as u8).collect();
            round_trip(&runs);
        }
        round_trip(&noise(MAX_INPUT, 3));
    }

    #[test]
    fn the_blob_is_deterministic() {
        let page = noise(PAGE, 11);
        let mut table = Table::new();
        let mut a = vec![0u8; max_compressed_len(PAGE)];
        let mut b = vec![0u8; max_compressed_len(PAGE)];
        let na = compress(&page, &mut a, &mut table).unwrap();
        let _ = compress(&[0x55u8; PAGE], &mut b, &mut table).unwrap();
        let nb = compress(&page, &mut b, &mut table).unwrap();
        assert_eq!(&a[..na], &b[..nb]);
    }

    #[test]
    fn test_reject_input_too_long() {
        let input = vec![0u8; MAX_INPUT + 1];
        let mut out = vec![0u8; max_compressed_len(input.len())];
        assert_eq!(compress(&input, &mut out, &mut Table::new()), Err(Error::InputTooLong));
    }

    #[test]
    fn test_reject_output_full_on_an_incompressible_page() {
        let page = noise(PAGE, 5);
        let mut out = vec![0u8; PAGE];
        assert_eq!(compress(&page, &mut out, &mut Table::new()), Err(Error::OutputFull));
    }

    #[test]
    fn test_reject_truncated_blob() {
        let text: Vec<u8> = b"abcabcabcabcabcabcabcabcabcabcabcabc".to_vec();
        let mut table = Table::new();
        let mut blob = vec![0u8; max_compressed_len(text.len())];
        let n = compress(&text, &mut blob, &mut table).unwrap();
        let mut out = vec![0u8; text.len()];
        // A cut inside a token, a length, an offset or a literal run is an error; a cut right
        // after a literal run is a shorter valid block (the format has no length header), so
        // the caller's check — the returned length equals the page — is what catches it.
        for cut in 0..n {
            match decompress(&blob[..cut], &mut out) {
                Err(_) => {}
                Ok(len) => assert!(len < text.len(), "cut {cut} came back whole"),
            }
        }
        assert_eq!(decompress(&[], &mut out), Err(Error::Truncated));
    }

    #[test]
    fn test_reject_offset_beyond_output() {
        // token: 1 literal, match len 4; literal 'a'; offset 2 (> 1 byte written); final literal.
        let blob = [0x10, b'a', 0x02, 0x00, 0x10, b'b'];
        let mut out = [0u8; 16];
        assert_eq!(decompress(&blob, &mut out), Err(Error::OffsetOutOfRange));
        let zero = [0x10, b'a', 0x00, 0x00, 0x10, b'b'];
        assert_eq!(decompress(&zero, &mut out), Err(Error::ZeroOffset));
    }

    #[test]
    fn test_reject_output_over_the_page() {
        // 1 literal then a match of 4+15+255+10 bytes into a 16-byte output.
        let blob = [0x1F, b'a', 0x01, 0x00, 0xFF, 0x0A, 0x10, b'b'];
        let mut out = [0u8; 16];
        assert_eq!(decompress(&blob, &mut out), Err(Error::OutputFull));
        // A literal run longer than the output.
        let mut lit = vec![0xF0, 0x05];
        lit.extend_from_slice(&[7u8; 20]);
        assert_eq!(decompress(&lit, &mut out), Err(Error::OutputFull));
    }

    #[test]
    fn test_reject_blob_ending_after_a_match() {
        let blob = [0x10, b'a', 0x01, 0x00];
        let mut out = [0u8; 16];
        assert_eq!(decompress(&blob, &mut out), Err(Error::NoFinalLiterals));
    }

    #[test]
    fn test_reject_literal_run_over_the_blob() {
        let blob = [0x50, b'a', b'b'];
        let mut out = [0u8; 16];
        assert_eq!(decompress(&blob, &mut out), Err(Error::Truncated));
    }
}
