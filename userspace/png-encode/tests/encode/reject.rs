// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: every bound of png-encode, one test_reject_* each: the typed error comes back
//! and NOTHING reaches the sink. Plus the accept side of each boundary, overflow-free
//! arithmetic on absurd strides, and sink failures that propagate at any write and leave
//! the encoder usable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 10 tests

use core::convert::Infallible;

use png_encode::{BgraFrame, EncodeError, Encoder, MAX_DIMENSION};

use crate::support::{encode, encode_with, noise, FailingSink, RecordingSink, Refused};

/// Encodes into a recording sink, expecting a reject before the first write.
fn rejected(encoder: &mut Encoder, frame: BgraFrame<'_>) -> EncodeError<Infallible> {
    let mut sink = RecordingSink::default();
    let err = encoder.encode(frame, &mut sink).unwrap_err();
    assert!(sink.writes.is_empty(), "{err:?}: bytes reached the sink");
    err
}

fn accepted(encoder: &mut Encoder, frame: BgraFrame<'_>) {
    let mut sink = RecordingSink::default();
    encoder.encode(frame, &mut sink).unwrap();
}

#[test]
fn test_reject_zero_width() {
    let pixels = [0u8; 64];
    let err = rejected(&mut Encoder::new(8).unwrap(), BgraFrame::new(&pixels, 0, 4, 16));
    assert_eq!(err, EncodeError::InvalidWidth(0));
    assert_eq!(err.to_string(), "width 0 outside 1..=8192");
}

#[test]
fn test_reject_zero_height() {
    let pixels = [0u8; 64];
    let err = rejected(&mut Encoder::new(8).unwrap(), BgraFrame::new(&pixels, 4, 0, 16));
    assert_eq!(err, EncodeError::InvalidHeight(0));
}

#[test]
fn test_reject_oversize_width() {
    let width = MAX_DIMENSION + 1;
    let pixels = vec![0u8; width as usize * 4];
    let mut encoder = Encoder::new(MAX_DIMENSION).unwrap();
    let err = rejected(&mut encoder, BgraFrame::new(&pixels, width, 1, width as usize * 4));
    assert_eq!(err, EncodeError::InvalidWidth(8193));
    accepted(&mut encoder, BgraFrame::new(&pixels, MAX_DIMENSION, 1, MAX_DIMENSION as usize * 4));
}

#[test]
fn test_reject_oversize_height() {
    let height = MAX_DIMENSION + 1;
    let pixels = vec![0u8; height as usize * 4];
    let mut encoder = Encoder::new(1).unwrap();
    assert_eq!(
        rejected(&mut encoder, BgraFrame::new(&pixels, 1, height, 4)),
        EncodeError::InvalidHeight(8193)
    );
    accepted(&mut encoder, BgraFrame::new(&pixels, 1, MAX_DIMENSION, 4));
}

#[test]
fn test_reject_width_beyond_the_encoder() {
    let pixels = vec![0u8; 65 * 4];
    let mut encoder = Encoder::new(64).unwrap();
    let err = rejected(&mut encoder, BgraFrame::new(&pixels, 65, 1, 65 * 4));
    assert_eq!(err, EncodeError::WidthExceedsEncoder { width: 65, max_width: 64 });
    accepted(&mut encoder, BgraFrame::new(&pixels, 64, 1, 64 * 4));
}

#[test]
fn test_reject_stride_below_width() {
    let pixels = vec![0u8; 40 * 5];
    let mut encoder = Encoder::new(10).unwrap();
    let err = rejected(&mut encoder, BgraFrame::new(&pixels, 10, 5, 39));
    assert_eq!(err, EncodeError::StrideTooSmall { stride: 39, min: 40 });
    assert_eq!(
        rejected(&mut encoder, BgraFrame::new(&pixels, 1, 5, 0)),
        EncodeError::StrideTooSmall { stride: 0, min: 4 }
    );
    accepted(&mut encoder, BgraFrame::new(&pixels, 10, 5, 40));
}

#[test]
fn test_reject_short_pixel_buffer() {
    // 5 rows, 48 bytes apart, 40 bytes each: the last row needs no padding.
    let needed = 48 * 4 + 40;
    let pixels = vec![0u8; needed];
    let mut encoder = Encoder::new(10).unwrap();
    let err = rejected(&mut encoder, BgraFrame::new(&pixels[..needed - 1], 10, 5, 48));
    assert_eq!(err, EncodeError::PixelsTooShort { len: needed - 1, needed });
    accepted(&mut encoder, BgraFrame::new(&pixels, 10, 5, 48));
}

#[test]
fn test_reject_absurd_stride_without_overflow() {
    let pixels = [0u8; 16];
    let err = rejected(&mut Encoder::new(1).unwrap(), BgraFrame::new(&pixels, 1, 3, usize::MAX));
    assert_eq!(err, EncodeError::PixelsTooShort { len: 16, needed: usize::MAX });
}

#[test]
fn test_reject_encoder_of_invalid_width() {
    assert_eq!(Encoder::new(0).unwrap_err(), EncodeError::InvalidWidth(0));
    assert_eq!(Encoder::new(MAX_DIMENSION + 1).unwrap_err(), EncodeError::InvalidWidth(8193));
    assert_eq!(Encoder::new(MAX_DIMENSION).unwrap().max_width(), MAX_DIMENSION);
}

#[test]
fn test_reject_failing_sink_at_any_write() {
    let image = noise(300, 200, 5);
    let mut probe = RecordingSink::default();
    Encoder::new(300).unwrap().encode(image.frame(), &mut probe).unwrap();
    let writes = probe.writes.len();
    let mut encoder = Encoder::new(300).unwrap();
    // The header, the first IDAT, one in the middle, the last IDAT, IEND.
    for fail_at in [0, 1, writes / 2, writes - 2, writes - 1] {
        let mut sink = FailingSink { fail_at, accepted: 0, attempts: 0 };
        let err = encoder.encode(image.frame(), &mut sink).unwrap_err();
        assert_eq!(err, EncodeError::Sink(Refused { at: fail_at }));
        assert_eq!(sink.attempts, fail_at + 1, "nothing is written after a refusal");
        // The same encoder still produces the complete PNG afterwards.
        assert_eq!(encode_with(&mut encoder, &image), encode(&image));
    }
}
