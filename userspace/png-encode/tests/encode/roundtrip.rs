// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pixel-exact round trips through the `png` crate for every image class a
//! screenshot contains, odd widths and padded strides, plus the measured compression of a
//! flat and a UI-like 1280x800 frame.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 10 tests

use crate::support::{
    assert_round_trip, flat, horizontal_gradient, noise, ui_like, vertical_gradient, Image,
};

#[test]
fn flat_round_trips() {
    assert_round_trip(&flat(97, 61, [0x20, 0x40, 0x80]));
    assert_round_trip(&flat(64, 64, [0xFF, 0xFF, 0xFF]));
    assert_round_trip(&flat(64, 64, [0, 0, 0]));
}

#[test]
fn horizontal_gradient_round_trips() {
    assert_round_trip(&horizontal_gradient(640, 120));
}

#[test]
fn vertical_gradient_round_trips() {
    assert_round_trip(&vertical_gradient(320, 480));
}

#[test]
fn noise_round_trips_and_grows_by_at_most_an_eighth() {
    // Literals cost 8 or 9 bits: incompressible input grows (1280x800 measured: 105.6 % of
    // the RGB bytes), never by more than 1/8 plus framing.
    let image = noise(257, 129, 0x1234_5678);
    let png = assert_round_trip(&image);
    let rgb = image.bgra_len() / 4 * 3;
    assert!(png.len() < rgb * 9 / 8 + 1024, "{} bytes for {rgb} RGB bytes", png.len());
}

#[test]
fn ui_like_round_trips() {
    assert_round_trip(&ui_like(640, 400));
}

#[test]
fn odd_widths_round_trip() {
    for width in [1, 2, 3, 5, 1281] {
        assert_round_trip(&noise(width, 9, width));
        assert_round_trip(&horizontal_gradient(width, 7));
    }
}

#[test]
fn padded_strides_round_trip_and_ignore_the_padding() {
    // Junk in the padding and in alpha; the buffer ends right after the last row.
    for (width, pad) in [(1, 4), (3, 1), (100, 12), (1281, 3)] {
        let stride = width as usize * 4 + pad;
        let mut rng = crate::support::XorShift(width);
        let image =
            Image::with_stride(width, 11, stride, |_, _| [rng.byte(), rng.byte(), rng.byte()]);
        assert_eq!(image.pixels.len(), stride * 10 + width as usize * 4);
        assert_round_trip(&image);
    }
}

#[test]
fn largest_dimensions_round_trip() {
    assert_round_trip(&horizontal_gradient(8192, 2));
    assert_round_trip(&vertical_gradient(1, 8192));
}

/// One colour, 1280x800: measured 21 082 bytes = 0.515 % of the 4 096 000-byte BGRA frame
/// (0.686 % of its 3 072 000 RGB bytes). Fixed Huffman codes spend at least 13 bits per
/// 258-byte match, so about 0.47 % is the floor for this encoder.
#[test]
fn flat_1280x800_compresses_to_0_515_percent_of_raw_bgra() {
    let image = flat(1280, 800, [0x20, 0x40, 0x80]);
    let png = assert_round_trip(&image);
    let percent = png.len() as f64 * 100.0 / image.bgra_len() as f64;
    assert!(percent < 0.55, "{} bytes = {percent:.3} % of raw BGRA (measured 0.515 %)", png.len());
}

/// A 1280x800 desktop (wallpaper, two windows full of glyph-like specks, taskbar): measured
/// 206 300 bytes = 5.04 % of raw BGRA (6.72 % of RGB).
#[test]
fn ui_like_1280x800_compresses_to_5_04_percent_of_raw_bgra() {
    let image = ui_like(1280, 800);
    let png = assert_round_trip(&image);
    let percent = png.len() as f64 * 100.0 / image.bgra_len() as f64;
    assert!(percent < 5.5, "{} bytes = {percent:.3} % of raw BGRA (measured 5.04 %)", png.len());
}
