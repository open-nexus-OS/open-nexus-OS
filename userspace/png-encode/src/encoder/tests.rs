// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests of the encoder's internals: the exact signature + IHDR bytes, the
//! IDAT stream inflated by the in-crate inflater (Adler-32 checked) against the filtered
//! rows, the scrub of `reset`, the scratch budget, and `Debug` output free of pixel bytes.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests

use alloc::format;
use alloc::vec::Vec;

use super::{header, BgraFrame, Encoder};
use crate::filter::{filter_row, filtered_len, load_row, row_len};
use crate::inflate_check::inflate_zlib;

/// (type, payload) of every chunk after the signature.
fn chunks(png: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut rest = &png[8..];
    let mut out = Vec::new();
    while !rest.is_empty() {
        let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
        out.push((rest[4..8].try_into().unwrap(), &rest[8..8 + len]));
        rest = &rest[12 + len..];
    }
    out
}

#[test]
fn header_is_the_signature_and_the_ihdr_of_a_1x1_rgb8_png() {
    // The IHDR CRC of every 1x1 RGB8 PNG is 0x907753DE.
    let expected = [
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0,
        0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xDE,
    ];
    assert_eq!(header(1, 1), expected);
}

#[test]
fn idat_stream_inflates_to_the_filtered_rows() {
    let (w, h, stride) = (7usize, 5usize, 7 * 4 + 4);
    let pixels: Vec<u8> = (0..stride * h).map(|i| (i * 37 % 251) as u8).collect();
    let mut png = Vec::new();
    let mut encoder = Encoder::new(16).unwrap();
    encoder.encode(BgraFrame::new(&pixels, w as u32, h as u32, stride), &mut png).unwrap();

    let zlib: Vec<u8> = chunks(&png)
        .into_iter()
        .filter(|(kind, _)| kind == b"IDAT")
        .flat_map(|(_, payload)| payload.iter().copied())
        .collect();
    let inflated = inflate_zlib(&zlib).expect("valid zlib stream with matching Adler-32");

    let mut expected = Vec::new();
    let mut prev = vec![0u8; row_len(w)];
    for row in pixels.chunks(stride) {
        let mut cur = vec![0u8; row_len(w)];
        load_row(&row[..w * 4], &mut cur);
        let mut filtered = vec![0u8; filtered_len(w)];
        filter_row(&cur, &prev, &mut filtered);
        expected.extend_from_slice(&filtered);
        prev = cur;
    }
    assert_eq!(inflated, expected);
}

#[test]
fn reset_scrubs_the_rows_and_the_staging_buffer() {
    let pixels: Vec<u8> = (0..64 * 64 * 4).map(|i| (i % 253) as u8 | 1).collect();
    let mut encoder = Encoder::new(64).unwrap();
    encoder.encode(BgraFrame::new(&pixels, 64, 64, 256), &mut Vec::new()).unwrap();
    assert!(encoder.prev.iter().chain(encoder.cur.iter()).any(|&b| b != 0));
    assert!(encoder.staging.iter().any(|&b| b != 0));
    encoder.reset();
    let all_zero = |buf: &[u8]| buf.iter().all(|&b| b == 0);
    assert!(all_zero(&encoder.prev) && all_zero(&encoder.cur) && all_zero(&encoder.filtered));
    assert!(all_zero(&encoder.row), "the pulled row holds pixels too");
    assert!(all_zero(&encoder.staging[..]));
}

#[test]
fn scratch_is_88_kib_plus_13_bytes_per_pixel_of_width() {
    // Window 65 536 + heads 8 192 + staging 16 396 + two row pads 6 + filter byte 1 = 90 131;
    // per pixel: two padded RGB rows (6), the filtered row (3) and the pulled BGRA row (4).
    for (width, bytes) in [(1, 90_144), (1280, 106_771), (1920, 115_091), (8192, 196_627)] {
        let encoder = Encoder::new(width).unwrap();
        assert_eq!(encoder.scratch_bytes(), 90_131 + 13 * width as usize);
        assert_eq!(encoder.scratch_bytes(), bytes);
    }
}

#[test]
fn debug_output_carries_no_pixel_bytes() {
    let pixels = [0xABu8; 64];
    let frame = format!("{:?}", BgraFrame::new(&pixels, 4, 4, 16));
    assert!(frame.contains("pixels_len: 64"), "{frame}");
    assert!(!frame.contains("171"), "{frame}");
    let encoder = format!("{:?}", Encoder::new(4).unwrap());
    assert!(encoder.contains("max_width: 4"), "{encoder}");
}
