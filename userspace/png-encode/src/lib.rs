// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the PNG encoder of screenshots (TASK-0068: a capture is saved as PNG to the
//! user's Pictures/Screenshots). A BGRA8 frame goes in, an RGB8 PNG (color type 2; alpha is
//! dropped, screenshots are opaque) comes out, streamed through a [`Sink`]. Built for a
//! service whose heap never frees (a bump allocator): [`Encoder::new`] allocates every
//! scratch buffer ONCE, sized from the widest frame it will see, and [`Encoder::encode`]
//! allocates nothing; the same encoder serves every later frame of that width or narrower.
//!
//! Per scanline: BGRA -> RGB, then the PNG filter (None/Sub/Up/Paeth) with the smallest sum
//! of absolute signed residuals, then zlib/deflate: a greedy LZ77 over a 32 KiB window with a
//! 4096-head hash table (no chains, hash of 3 bytes, matches 3..=258), coded with deflate's
//! FIXED Huffman codes in one final block. Output is staged in a 16 KiB buffer and every full
//! buffer leaves as one IDAT chunk, so no caller ever holds the whole file.
//!
//! Scratch held for the encoder's lifetime: 88 KiB (64 KiB window, 8 KiB hash heads, 16 KiB
//! IDAT staging) + 13 bytes per pixel of the maximum width (three RGB scanlines and the pulled
//! BGRA row): about 112 KiB at 1920, 192 KiB at 8192 (`Encoder::scratch_bytes`). The rows come
//! from a borrowed frame (`encode`) or are pulled one at a time (`encode_rows`) — a frame in a
//! VMO is read row by row, never mapped whole.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 56 tests + 1 doc example. 29 unit tests (CRC-32/Adler-32 vectors, fixed codes against
//! RFC 1951, filters, deflate inflated by an in-crate fixed-block inflater) + 27 in
//! tests/encode (round trips through the `png` crate with checksums on, streaming chunk
//! boundaries, reuse, test_reject_*)
//!
//! PUBLIC API:
//!   - `Encoder::new(max_width)` / `encode(frame, sink)` / `encode_rows(w, h, rows, sink)` /
//!     `reset()` / `scratch_bytes()`
//!   - `BgraFrame`: borrowed BGRA8 pixels + width, height, stride in bytes
//!   - `Sink`: where the PNG bytes go (impls for `&mut S` and `Vec<u8>`)
//!   - `EncodeError<E>`: one typed reject per bound, `OutOfMemory`, `Sink(E)`
//!   - `MAX_DIMENSION`, `IDAT_PAYLOAD_MAX`, `max_encoded_len(w, h)` (the destination to size)
//!
//! DEPENDENCIES: none (CRC-32 and Adler-32 live here); `png` is a dev-dependency for the
//! round-trip tests only.
//!
//! ```
//! use png_encode::{BgraFrame, Encoder};
//!
//! // Once, at service start: all scratch for frames up to 1920 pixels wide.
//! let mut encoder = Encoder::new(1920)?;
//! // Per capture: any frame of that width or narrower, rows `stride` bytes apart.
//! let pixels = vec![0x80u8; 640 * 480 * 4];
//! let mut png = Vec::new(); // or any `Sink`: a file, an IPC stream
//! let written = encoder.encode(BgraFrame::new(&pixels, 640, 480, 640 * 4), &mut png)?;
//! assert_eq!(written, png.len() as u64);
//! encoder.reset(); // scrub the capture's pixels from the scratch
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod adler;
mod crc;
mod deflate;
mod encoder;
mod error;
mod filter;
mod fixed;
mod idat;
#[cfg(test)]
mod inflate_check;
mod scratch;
mod sink;

pub use encoder::{max_encoded_len, BgraFrame, Encoder};
pub use error::EncodeError;
pub use idat::IDAT_PAYLOAD_MAX;
pub use sink::Sink;

/// Largest accepted width and height, in pixels.
pub const MAX_DIMENSION: u32 = 8192;
