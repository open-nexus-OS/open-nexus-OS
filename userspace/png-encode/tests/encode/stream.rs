// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: how a PNG reaches the sink and how one encoder is reused. Writes are exactly:
//! signature + IHDR, one whole IDAT chunk each (16 KiB payloads, only the last shorter),
//! IEND, all with valid CRCs and a returned byte count that matches. Output is
//! deterministic; reuse works with and without `reset`, for narrower frames too, and the
//! scratch never grows.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests

use std::collections::TryReserveError;

use png_encode::{BgraFrame, Encoder, Sink, IDAT_PAYLOAD_MAX};

use crate::support::{
    decode, encode, encode_with, flat, horizontal_gradient, noise, parse_chunks, ui_like,
    vertical_gradient, RecordingSink,
};

#[test]
fn writes_are_the_header_then_whole_idat_chunks_then_iend() {
    // Noise: about 190 KiB of output, a dozen IDAT chunks.
    let image = noise(300, 200, 0x00C0_FFEE);
    let mut sink = RecordingSink::default();
    let written = Encoder::new(300).unwrap().encode(image.frame(), &mut sink).unwrap();
    let writes = &sink.writes;
    assert_eq!(written, writes.iter().map(Vec::len).sum::<usize>() as u64);

    assert_eq!(&writes[0][..8], b"\x89PNG\r\n\x1a\n");
    let header = parse_chunks(&writes[0][8..]);
    assert!(header.len() == 1 && &header[0].kind == b"IHDR" && header[0].crc_ok);
    let trailer = parse_chunks(writes.last().unwrap());
    assert!(trailer.len() == 1 && &trailer[0].kind == b"IEND" && trailer[0].crc_ok);

    let idats = &writes[1..writes.len() - 1];
    assert!(idats.len() >= 10, "{} IDAT writes", idats.len());
    for (i, write) in idats.iter().enumerate() {
        let chunks = parse_chunks(write);
        assert_eq!(chunks.len(), 1, "write {i} is one whole chunk");
        assert!(&chunks[0].kind == b"IDAT" && chunks[0].crc_ok, "write {i}");
        let len = chunks[0].payload.len();
        if i + 1 < idats.len() {
            assert_eq!(len, IDAT_PAYLOAD_MAX, "write {i} is a full chunk");
        } else {
            assert!((1..=IDAT_PAYLOAD_MAX).contains(&len), "last IDAT carries {len}");
        }
    }

    let png = writes.concat();
    assert_eq!(png, encode(&image));
    assert!(decode(&png).2 == image.rgb());
}

#[test]
fn a_one_pixel_image_is_three_writes() {
    let image = flat(1, 1, [1, 2, 3]);
    let mut sink = RecordingSink::default();
    let written = Encoder::new(1).unwrap().encode(image.frame(), &mut sink).unwrap();
    assert_eq!(sink.writes.len(), 3);
    assert_eq!(sink.writes[0].len(), 33);
    assert_eq!(sink.writes[2], [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]);
    let png = sink.writes.concat();
    assert_eq!(written, png.len() as u64);
    assert_eq!(decode(&png), (1, 1, vec![1, 2, 3]));
}

#[test]
fn output_is_deterministic_and_reuse_needs_no_reset() {
    let wide = noise(1281, 40, 99);
    let narrow = ui_like(640, 400);
    let mut encoder = Encoder::new(1281).unwrap();
    let scratch = encoder.scratch_bytes();
    let first = encode_with(&mut encoder, &wide);
    // A narrower frame straight after: no state of the first may leak into it.
    let second = encode_with(&mut encoder, &narrow);
    let third = encode_with(&mut encoder, &wide);
    assert_eq!(first, third);
    assert_eq!(second, encode(&narrow), "same bytes as a fresh encoder");
    assert!(decode(&second).2 == narrow.rgb());
    assert_eq!(encoder.scratch_bytes(), scratch, "scratch is fixed at construction");
}

#[test]
fn reset_then_encode_a_second_image() {
    let mut encoder = Encoder::new(800).unwrap();
    let first = ui_like(800, 600);
    assert!(decode(&encode_with(&mut encoder, &first)).2 == first.rgb());
    encoder.reset();
    let second = vertical_gradient(333, 77);
    let png = encode_with(&mut encoder, &second);
    let (width, height, rgb) = decode(&png);
    assert_eq!((width, height), (333, 77));
    assert!(rgb == second.rgb());
    assert_eq!(png, encode(&second), "same bytes as a fresh encoder");
}

#[test]
fn a_dyn_sink_and_a_borrowed_sink_work() {
    let image = horizontal_gradient(50, 20);
    let mut encoder = Encoder::new(50).unwrap();
    let mut png = Vec::new();
    let sink: &mut dyn Sink<Error = TryReserveError> = &mut png;
    encoder.encode(image.frame(), sink).unwrap();
    assert_eq!(png, encode(&image));
    let mut again = Vec::new();
    let mut borrowed = &mut again;
    encoder.encode(image.frame(), &mut borrowed).unwrap();
    assert_eq!(again, png);
}

/// Pulled rows encode to exactly the bytes of the borrowed frame, and a row source that fails
/// stops the encode with its own error (the sink then holds a truncated PNG).
#[test]
fn pulled_rows_match_the_borrowed_frame_and_source_errors_stop_the_encode() {
    let (w, h) = (37u32, 23u32);
    let stride = w as usize * 4 + 8;
    let mut pixels = vec![0u8; stride * h as usize];
    for (i, b) in pixels.iter_mut().enumerate() {
        *b = (i.wrapping_mul(31) ^ (i >> 3)) as u8;
    }
    let mut enc = Encoder::new(64).expect("scratch");
    let mut borrowed = Vec::new();
    enc.encode(BgraFrame::new(&pixels, w, h, stride), &mut borrowed).expect("encodes");
    let mut pulled = Vec::new();
    let mut asked = Vec::new();
    let rows = |y: u32, out: &mut [u8]| {
        asked.push(y);
        let at = y as usize * stride;
        out.copy_from_slice(&pixels[at..at + out.len()]);
        Ok(())
    };
    enc.encode_rows(w, h, rows, &mut pulled).expect("encodes");
    assert_eq!(pulled, borrowed, "one encode, two ways to feed it");
    assert_eq!(asked, (0..h).collect::<Vec<_>>(), "each row once, top to bottom");

    struct Refusing;
    impl Sink for Refusing {
        type Error = u8;
        fn write(&mut self, _bytes: &[u8]) -> Result<(), u8> {
            Ok(())
        }
    }
    let failing = |y: u32, _out: &mut [u8]| if y == 5 { Err(9u8) } else { Ok(()) };
    let err = enc.encode_rows(w, h, failing, &mut Refusing).expect_err("the source failed");
    assert_eq!(err, png_encode::EncodeError::Sink(9));
}

/// The worst case holds: incompressible noise never outgrows `max_encoded_len`, and the bound
/// stays close to the raw RGB size (a destination sized by it is not wasteful).
#[test]
fn noise_never_outgrows_the_encoded_bound() {
    let (w, h) = (257u32, 129u32);
    let mut state = 0x9E37_79B9u32;
    let pixels: Vec<u8> = (0..w as usize * h as usize * 4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let mut enc = Encoder::new(w).expect("scratch");
    let mut png = Vec::new();
    let n = enc.encode(BgraFrame::new(&pixels, w, h, w as usize * 4), &mut png).expect("encodes");
    let bound = png_encode::max_encoded_len(w, h);
    assert!(n as usize <= bound, "{n} > {bound}");
    let raw = h as usize * (1 + 3 * w as usize);
    assert!(bound < raw + raw / 7, "the bound is about 9/8 of the raw rows");
    let absurd = png_encode::max_encoded_len(u32::MAX, u32::MAX);
    assert!(absurd >= usize::MAX / 8, "an absurd size never wraps to a small one: {absurd}");
}
