// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A mutable BGRA8888 pixel surface — the one target every software rasterizer
//! primitive writes into.
//!
//! The surface borrows its backing bytes (`&mut [u8]`); it never allocates and
//! never owns memory. That lets the same primitives serve both a host `Vec`
//! (the reference backend) and a mapped device VMO (the live GPU driver): the
//! driver builds a slice over its mapping once and hands it in, keeping all the
//! `unsafe` at that single boundary while the rasterizer itself stays safe.

#![forbid(unsafe_code)]

/// Bytes per pixel for the BGRA8888 surfaces this rasterizer targets.
pub const BYTES_PER_PIXEL: usize = 4;

/// A borrowed, row-major BGRA8888 pixel surface (stride == `width * 4`).
pub struct Surface<'a> {
    buf: &'a mut [u8],
    width: u32,
    height: u32,
}

impl<'a> Surface<'a> {
    /// Wrap `buf` as a `width`-wide BGRA surface. The height is derived from the
    /// buffer length (`buf.len() / (width * 4)`), so a surface can cover only the
    /// rows that actually fit — callers may pass a mapping larger than one plane
    /// and address rows absolutely.
    #[must_use]
    pub fn new(buf: &'a mut [u8], width: u32) -> Self {
        let height =
            if width == 0 { 0 } else { (buf.len() / (width as usize * BYTES_PER_PIXEL)) as u32 };
        Self { buf, width, height }
    }

    #[inline]
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline]
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    #[must_use]
    pub fn buf(&self) -> &[u8] {
        self.buf
    }

    #[inline]
    #[must_use]
    pub fn buf_mut(&mut self) -> &mut [u8] {
        self.buf
    }

    /// Byte stride of one pixel row.
    #[inline]
    #[must_use]
    pub fn stride(&self) -> usize {
        self.width as usize * BYTES_PER_PIXEL
    }
}
