// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of the deflate core, decoded by the in-crate `inflate_check`
//! inflater: exact bytes of the empty stream, edge sizes, window slides, repeats out of
//! reach, output independent of how the input is split, and reuse of one deflater.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 9 tests

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::{common_prefix, Deflater, WINDOW_LEN, WSIZE};
use crate::idat::{ChunkWriter, STAGING_LEN};
use crate::inflate_check::inflate_zlib;

/// Deterministic noise (xorshift32).
fn noise(len: usize, mut seed: u32) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed.to_le_bytes()[0]
        })
        .collect()
}

/// The IDAT payloads of a run of chunks, concatenated: the zlib stream.
fn idat_payloads(mut chunks: &[u8]) -> Vec<u8> {
    let mut zlib = Vec::new();
    while !chunks.is_empty() {
        let len = u32::from_be_bytes(chunks[..4].try_into().unwrap()) as usize;
        assert_eq!(&chunks[4..8], b"IDAT");
        zlib.extend_from_slice(&chunks[8..8 + len]);
        chunks = &chunks[12 + len..];
    }
    zlib
}

/// One zlib stream of `pieces`, fed to `deflater` in exactly that split.
fn zlib_of(deflater: &mut Deflater, pieces: &[&[u8]]) -> Vec<u8> {
    let mut staging = Box::new([0u8; STAGING_LEN]);
    let mut sink = Vec::new();
    let mut out = ChunkWriter::new(&mut staging, &mut sink);
    deflater.start(&mut out).unwrap();
    for piece in pieces {
        deflater.write(piece, &mut out).unwrap();
    }
    deflater.finish(&mut out).unwrap();
    out.flush().unwrap();
    idat_payloads(&sink)
}

fn round_trip(data: &[u8]) -> Vec<u8> {
    let zlib = zlib_of(&mut Deflater::new().unwrap(), &[data]);
    assert_eq!(inflate_zlib(&zlib).as_deref(), Some(data), "round trip of {} bytes", data.len());
    zlib
}

#[test]
fn empty_input_is_the_canonical_ten_bit_block() {
    // Header, BFINAL=1 + BTYPE=01 + end-of-block (10 bits, two bytes), Adler-32 of nothing.
    assert_eq!(round_trip(&[]), [0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
}

#[test]
fn tiny_inputs_round_trip() {
    for len in [1, 2, 3, 4, 7, 8, 9, 258, 259, 260, 261, 262] {
        round_trip(&noise(len, 0x1234_5678 + len as u32));
        round_trip(&vec![0xA5; len]);
    }
}

#[test]
fn flat_run_across_many_slides_is_all_maximal_matches() {
    // 300 000 equal bytes: one literal, then length-258 matches (13 bits each).
    let data = vec![7u8; 300_000];
    let zlib = round_trip(&data);
    let matches = (data.len() - 1).div_ceil(258);
    assert!(zlib.len() <= 2 + 4 + 2 + (matches * 13).div_ceil(8) + 1, "{} bytes", zlib.len());
}

#[test]
fn noise_round_trips_as_literals() {
    let data = noise(3 * WINDOW_LEN + 123, 0x9E37_79B9);
    let zlib = round_trip(&data);
    // Fixed codes: 8 or 9 bits per literal byte, so noise grows by at most 1/8.
    assert!(zlib.len() <= data.len() * 9 / 8 + 16);
}

#[test]
fn repeats_are_referenced_up_to_32_kib_back_and_never_beyond() {
    // Noise followed by itself, `gap` bytes later. One head per hash and no chains: a
    // repeat is found only where its head survived the `gap` inserts in between
    // ((1 - 1/4096)^gap per byte), and every hit then runs 258 bytes.
    let ratio = |gap: usize, seed: u32| {
        let block = noise(gap, seed);
        let data = [block.as_slice(), block.as_slice()].concat();
        round_trip(&data).len() as f64 / data.len() as f64
    };
    // 20 000 back: about two thirds of the copy become matches (measured 0.709).
    let near = ratio(20_000, 8);
    assert!(near < 0.8, "near {near}");
    // Exactly 32 768 back: still in reach and decodable (hits are rare at this distance).
    ratio(32_768, 9);
    // 40 000 back is beyond the window: the copy costs what the original did (8-9 bits
    // per noise byte, measured 1.05).
    let far = ratio(40_000, 7);
    assert!(far > 1.0, "far {far}");
}

#[test]
fn output_does_not_depend_on_how_the_input_is_split() {
    let mut data = noise(50_000, 3);
    data.extend(core::iter::repeat_n(0u8, 40_000));
    data.extend(noise(30_000, 4).iter().map(|b| b & 0x0F));
    let whole = zlib_of(&mut Deflater::new().unwrap(), &[&data]);
    for piece in [1, 7, 258, 3841, WSIZE + 1, WINDOW_LEN + 3] {
        let pieces: Vec<&[u8]> = data.chunks(piece).collect();
        assert_eq!(zlib_of(&mut Deflater::new().unwrap(), &pieces), whole, "pieces of {piece}");
    }
    assert_eq!(inflate_zlib(&whole), Some(data));
}

#[test]
fn a_reused_deflater_forgets_the_previous_stream() {
    let first = noise(70_000, 11);
    let second = [noise(1000, 12), first[..5000].to_vec()].concat();
    let mut deflater = Deflater::new().unwrap();
    zlib_of(&mut deflater, &[&first]);
    // The second stream must not reference the first one's bytes (still in the window).
    let reused = zlib_of(&mut deflater, &[&second]);
    assert_eq!(reused, zlib_of(&mut Deflater::new().unwrap(), &[&second]));
    assert_eq!(inflate_zlib(&reused), Some(second));
}

#[test]
fn scrub_zeroes_the_window() {
    let mut deflater = Deflater::new().unwrap();
    zlib_of(&mut deflater, &[&noise(10_000, 5)]);
    assert!(deflater.window.iter().any(|&b| b != 0));
    deflater.scrub();
    assert!(deflater.window.iter().all(|&b| b == 0));
    assert!(deflater.head.iter().all(|&s| s == 0));
}

#[test]
fn common_prefix_agrees_with_a_byte_loop() {
    let base = noise(300, 21);
    for len in [0, 1, 7, 8, 9, 15, 16, 17, 258] {
        for flip in 0..=len {
            let a = &base[..len];
            let mut b = a.to_vec();
            if flip < len {
                b[flip] ^= 0x40;
            }
            let expected = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
            assert_eq!(common_prefix(a, &b), expected, "len {len}, flip {flip}");
        }
    }
}
