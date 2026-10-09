// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the one error type of png-encode. Every bound a frame can break is its own
//! variant (checked before the first byte reaches the sink), allocation failure is a value
//! rather than an abort, and a sink failure comes back unchanged as `Sink(E)`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/encode/reject.rs (one test_reject_* per variant)

use core::convert::Infallible;
use core::fmt;

/// Why nothing (or, for `Sink`, only part of a PNG) was written. `E` is the sink's error;
/// `Encoder::new` touches no sink and uses the default, `Infallible`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError<E = Infallible> {
    /// The width is 0 or above [`crate::MAX_DIMENSION`].
    InvalidWidth(u32),
    /// The height is 0 or above [`crate::MAX_DIMENSION`].
    InvalidHeight(u32),
    /// The frame is wider than the encoder's scratch was sized for (`Encoder::new`).
    WidthExceedsEncoder {
        /// The frame's width.
        width: u32,
        /// The width the encoder was built for.
        max_width: u32,
    },
    /// A row's stride is shorter than `width * 4` bytes.
    StrideTooSmall {
        /// The frame's stride in bytes.
        stride: usize,
        /// `width * 4`.
        min: usize,
    },
    /// The pixel buffer ends before the last row does.
    PixelsTooShort {
        /// The buffer's length.
        len: usize,
        /// `stride * (height - 1) + width * 4` (saturating).
        needed: usize,
    },
    /// The scratch buffers could not be allocated (only from `Encoder::new`).
    OutOfMemory,
    /// The sink refused bytes; what it already holds is a truncated PNG.
    Sink(E),
}

impl<E: fmt::Display> fmt::Display for EncodeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWidth(w) => write!(f, "width {w} outside 1..={}", crate::MAX_DIMENSION),
            Self::InvalidHeight(h) => write!(f, "height {h} outside 1..={}", crate::MAX_DIMENSION),
            Self::WidthExceedsEncoder { width, max_width } => {
                write!(f, "width {width} exceeds the encoder's {max_width}")
            }
            Self::StrideTooSmall { stride, min } => write!(f, "stride {stride} below {min}"),
            Self::PixelsTooShort { len, needed } => {
                write!(f, "pixel buffer of {len} bytes, {needed} needed")
            }
            Self::OutOfMemory => f.write_str("scratch allocation failed"),
            Self::Sink(e) => write!(f, "sink: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> core::error::Error for EncodeError<E> {}
