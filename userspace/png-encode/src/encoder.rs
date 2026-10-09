// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the public face of png-encode. `Encoder` owns every scratch buffer, sized once
//! from the maximum width; `BgraFrame` describes the caller's pixels; `encode` checks every
//! bound before the first byte reaches the sink, then streams the signature with IHDR, the
//! IDAT chunks and IEND. `encode_rows` is the same encode with the rows PULLED one at a time
//! into the encoder's row scratch — for pixels the caller cannot borrow as one slice (a frame
//! in a VMO, read row by row); `encode` is that path over a borrowed frame. Neither type prints pixel bytes in its `Debug` output (a frame is
//! the user's screen).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (header bytes, IDAT stream inflated in-crate, scrub,
//! scratch size) + tests/encode

use alloc::boxed::Box;
use core::fmt;
use core::mem;

use crate::crc::crc32;
use crate::deflate::Deflater;
use crate::error::EncodeError;
use crate::filter::{filter_row, filtered_len, load_row, row_len};
use crate::idat::{ChunkWriter, STAGING_LEN};
use crate::scratch::{zeroed_array, zeroed_slice};
use crate::sink::Sink;
use crate::MAX_DIMENSION;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// IHDR payload: width, height, bit depth 8, colour type 2 (RGB), deflate, adaptive
/// filtering, no interlace.
const IHDR_LEN: u32 = 13;
/// Signature + IHDR chunk (length, type, 13 bytes, CRC): the first write of every PNG.
const HEADER_LEN: usize = 8 + 4 + 4 + 13 + 4;
/// IEND: no payload, the CRC of the type alone.
const IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];
/// Bytes per input pixel (BGRA8).
const IN_BPP: usize = 4;

/// A borrowed BGRA8 frame: `height` rows of `width` pixels, row `y` starting at byte
/// `y * stride` of `pixels`. Alpha is ignored. The last row needs no padding after it.
#[derive(Clone, Copy)]
pub struct BgraFrame<'a> {
    /// The pixel bytes, B, G, R, A per pixel.
    pub pixels: &'a [u8],
    /// Pixels per row, 1..=`MAX_DIMENSION`.
    pub width: u32,
    /// Rows, 1..=`MAX_DIMENSION`.
    pub height: u32,
    /// Bytes from the start of one row to the next, at least `width * 4`.
    pub stride: usize,
}

impl<'a> BgraFrame<'a> {
    /// Describes `pixels` (checked by `Encoder::encode`, not here).
    #[must_use]
    pub const fn new(pixels: &'a [u8], width: u32, height: u32, stride: usize) -> Self {
        Self { pixels, width, height, stride }
    }

    /// Checks every bound; the result is what `encode` may rely on.
    fn layout<E>(&self, max_width: u32) -> Result<Layout, EncodeError<E>> {
        let (width, height) = (self.width, self.height);
        if !(1..=MAX_DIMENSION).contains(&width) {
            return Err(EncodeError::InvalidWidth(width));
        }
        if !(1..=MAX_DIMENSION).contains(&height) {
            return Err(EncodeError::InvalidHeight(height));
        }
        if width > max_width {
            return Err(EncodeError::WidthExceedsEncoder { width, max_width });
        }
        // Both at most MAX_DIMENSION: lossless, and every product below is small.
        let (w, h) = (width as usize, height as usize);
        let row = w * IN_BPP;
        if self.stride < row {
            return Err(EncodeError::StrideTooSmall { stride: self.stride, min: row });
        }
        let span = self.stride.saturating_mul(h - 1).saturating_add(row);
        if self.pixels.len() < span {
            return Err(EncodeError::PixelsTooShort { len: self.pixels.len(), needed: span });
        }
        Ok(Layout { width, height, stride: self.stride, row })
    }
}

impl fmt::Debug for BgraFrame<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BgraFrame")
            .field("pixels_len", &self.pixels.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .finish()
    }
}

/// A frame's checked geometry.
struct Layout {
    width: u32,
    height: u32,
    stride: usize,
    /// BGRA bytes of one row.
    row: usize,
}

/// A reusable PNG encoder for frames up to `max_width` pixels wide.
///
/// All memory is allocated by [`Encoder::new`]; [`Encoder::encode`] allocates nothing and
/// can be called any number of times (each call is a complete, independent PNG).
pub struct Encoder {
    max_width: u32,
    /// The row above, padded RGB (the filters' "up"); zeros above the first row.
    prev: Box<[u8]>,
    /// The current row, padded RGB (see `filter::LEFT_PAD`).
    cur: Box<[u8]>,
    /// `[filter type, residuals...]` of the current row.
    filtered: Box<[u8]>,
    /// One BGRA row as the row source delivers it.
    row: Box<[u8]>,
    deflater: Deflater,
    staging: Box<[u8; STAGING_LEN]>,
}

impl Encoder {
    /// Allocates the scratch for frames up to `max_width` pixels wide (any height up to
    /// `MAX_DIMENSION`): 88 KiB + 13 bytes per pixel of `max_width`.
    ///
    /// # Errors
    /// `InvalidWidth` unless `max_width` is in 1..=`MAX_DIMENSION`; `OutOfMemory` when the
    /// heap cannot serve the scratch.
    pub fn new(max_width: u32) -> Result<Self, EncodeError> {
        if !(1..=MAX_DIMENSION).contains(&max_width) {
            return Err(EncodeError::InvalidWidth(max_width));
        }
        let width = max_width as usize;
        Ok(Self {
            max_width,
            prev: zeroed_slice(row_len(width))?,
            cur: zeroed_slice(row_len(width))?,
            filtered: zeroed_slice(filtered_len(width))?,
            row: zeroed_slice(width * IN_BPP)?,
            deflater: Deflater::new()?,
            staging: zeroed_array()?,
        })
    }

    /// The widest frame this encoder accepts.
    #[must_use]
    pub const fn max_width(&self) -> u32 {
        self.max_width
    }

    /// Heap bytes held for the encoder's whole lifetime.
    #[must_use]
    pub fn scratch_bytes(&self) -> usize {
        self.prev.len()
            + self.cur.len()
            + self.filtered.len()
            + self.row.len()
            + Deflater::SCRATCH_BYTES
            + STAGING_LEN
    }

    /// Forgets the last frame: zeroes every scratch buffer, which otherwise keeps pixels of
    /// the previous capture (the window, the rows, the staged output). Allocates and frees
    /// nothing. Each `encode` starts a fresh stream by itself; `reset` is the scrub between
    /// captures, not a precondition.
    pub fn reset(&mut self) {
        self.prev.fill(0);
        self.cur.fill(0);
        self.filtered.fill(0);
        self.row.fill(0);
        self.deflater.scrub();
        self.staging.fill(0);
    }

    /// Encodes `frame` as an RGB8 PNG into `sink` and returns the bytes written.
    ///
    /// # Errors
    /// A bound of the frame (`InvalidWidth`, `InvalidHeight`, `WidthExceedsEncoder`,
    /// `StrideTooSmall`, `PixelsTooShort`), found before anything is written; or
    /// `Sink(e)` when the sink fails, after which the sink holds a truncated PNG.
    pub fn encode<S: Sink + ?Sized>(
        &mut self,
        frame: BgraFrame<'_>,
        sink: &mut S,
    ) -> Result<u64, EncodeError<S::Error>> {
        let layout = frame.layout(self.max_width)?;
        // `layout` proves every row exists; a miss would be a bug, never a panic.
        let pixels = frame.pixels;
        let (stride, row) = (layout.stride, layout.row);
        let source = |y: u32, out: &mut [u8]| {
            let at = y as usize * stride;
            if let Some(src) = pixels.get(at..at + row) {
                out.copy_from_slice(src);
            }
            Ok(())
        };
        self.encode_rows(layout.width, layout.height, source, sink)
    }

    /// Encodes `width` x `height` BGRA8 rows as an RGB8 PNG into `sink`, PULLING row `y` (top
    /// to bottom, each once) from `rows(y, out)` into `out` — exactly `width * 4` bytes of the
    /// encoder's own scratch. Alpha is ignored. Returns the bytes written.
    ///
    /// # Errors
    /// `InvalidWidth`, `InvalidHeight` or `WidthExceedsEncoder`, found before anything is
    /// written; `Sink(e)` when the sink or the row source fails — the sink then holds a
    /// truncated PNG.
    pub fn encode_rows<S, R>(
        &mut self,
        width: u32,
        height: u32,
        mut rows: R,
        sink: &mut S,
    ) -> Result<u64, EncodeError<S::Error>>
    where
        S: Sink + ?Sized,
        R: FnMut(u32, &mut [u8]) -> Result<(), S::Error>,
    {
        if !(1..=MAX_DIMENSION).contains(&width) {
            return Err(EncodeError::InvalidWidth(width));
        }
        if !(1..=MAX_DIMENSION).contains(&height) {
            return Err(EncodeError::InvalidHeight(height));
        }
        let too_wide = || EncodeError::WidthExceedsEncoder { width, max_width: self.max_width };
        if width > self.max_width {
            return Err(too_wide());
        }
        let w = width as usize;
        let mut prev = self.prev.get_mut(..row_len(w)).ok_or_else(too_wide)?;
        let mut cur = self.cur.get_mut(..row_len(w)).ok_or_else(too_wide)?;
        let filtered = self.filtered.get_mut(..filtered_len(w)).ok_or_else(too_wide)?;
        let bgra = self.row.get_mut(..w * IN_BPP).ok_or_else(too_wide)?;
        prev.fill(0);

        let mut out = ChunkWriter::new(&mut self.staging, sink);
        out.raw(&header(width, height)).map_err(EncodeError::Sink)?;
        self.deflater.start(&mut out).map_err(EncodeError::Sink)?;
        for y in 0..height {
            rows(y, bgra).map_err(EncodeError::Sink)?;
            load_row(bgra, cur);
            filter_row(cur, prev, filtered);
            self.deflater.write(filtered, &mut out).map_err(EncodeError::Sink)?;
            mem::swap(&mut prev, &mut cur);
        }
        self.deflater.finish(&mut out).map_err(EncodeError::Sink)?;
        out.flush().map_err(EncodeError::Sink)?;
        out.raw(&IEND).map_err(EncodeError::Sink)?;
        Ok(out.written())
    }
}

impl fmt::Debug for Encoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Encoder")
            .field("max_width", &self.max_width)
            .field("scratch_bytes", &self.scratch_bytes())
            .finish_non_exhaustive()
    }
}

/// The most bytes a `width` x `height` PNG of this encoder can take — what a fixed destination
/// (a VMO) must hold. Deflate's fixed codes spend at most 9 bits on an input byte (a literal
/// above 143; every match is cheaper per byte), plus the block's 10 bits of framing; the zlib
/// header and checksum, 12 bytes per IDAT chunk, the signature with IHDR and IEND come on top.
/// Saturating, so an absurd size answers a huge length (no destination fits it), never a
/// wrapped small one.
#[must_use]
pub const fn max_encoded_len(width: u32, height: u32) -> usize {
    let raw =
        (height as usize).saturating_mul((width as usize).saturating_mul(3).saturating_add(1));
    let deflate = raw.saturating_mul(9).saturating_add(10).div_ceil(8);
    let zlib = deflate.saturating_add(2 + 4);
    let chunks = zlib.div_ceil(crate::IDAT_PAYLOAD_MAX).saturating_mul(12);
    zlib.saturating_add(chunks).saturating_add(HEADER_LEN + IEND.len())
}

/// The signature and the IHDR chunk of a `width` x `height` RGB8 PNG.
fn header(width: u32, height: u32) -> [u8; HEADER_LEN] {
    let [w0, w1, w2, w3] = width.to_be_bytes();
    let [h0, h1, h2, h3] = height.to_be_bytes();
    // Type + payload: what the chunk CRC covers.
    let ihdr = [b'I', b'H', b'D', b'R', w0, w1, w2, w3, h0, h1, h2, h3, 8, 2, 0, 0, 0];
    let crc = crc32(&ihdr).to_be_bytes();
    let len = IHDR_LEN.to_be_bytes();
    let mut out = [0u8; HEADER_LEN];
    let parts = SIGNATURE.iter().chain(&len).chain(&ihdr).chain(&crc);
    for (dst, src) in out.iter_mut().zip(parts) {
        *dst = *src;
    }
    out
}

#[cfg(test)]
mod tests;
