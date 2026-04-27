// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Owned BGRA8888 frame storage and checked pixel access.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::{RenderError, RenderResult};
use crate::geometry::Rect;
use crate::limits::{BYTES_PER_PIXEL, MAX_FRAME_PIXELS};
use crate::math::{checked_buffer_len, ensure_row_arithmetic};
use crate::pixel::PixelBgra;
use crate::units::{StrideBytes, SurfaceHeight, SurfaceWidth};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub(crate) width: SurfaceWidth,
    pub(crate) height: SurfaceHeight,
    pub(crate) stride: StrideBytes,
    pub(crate) buffer: Vec<u8>,
}

impl Frame {
    pub fn new_checked(width: u32, height: u32) -> RenderResult<Self> {
        ensure_row_arithmetic(width)?;
        let width = SurfaceWidth::new(width)?;
        let height = SurfaceHeight::new(height)?;
        let pixels = u64::from(width.get())
            .checked_mul(u64::from(height.get()))
            .ok_or(RenderError::ArithmeticOverflow)?;
        if pixels > MAX_FRAME_PIXELS {
            return Err(RenderError::FrameTooLarge);
        }
        let stride = StrideBytes::for_width(width)?;
        let len = checked_buffer_len(stride, height.get())?;
        Ok(Self {
            width,
            height,
            stride,
            buffer: vec![0; len],
        })
    }

    pub fn from_bgra_buffer_checked(
        width: u32,
        height: u32,
        stride: u32,
        buffer: Vec<u8>,
    ) -> RenderResult<Self> {
        ensure_row_arithmetic(width)?;
        let width = SurfaceWidth::new(width)?;
        let height = SurfaceHeight::new(height)?;
        let stride = StrideBytes::new_for_frame(width, stride)?;
        let expected = checked_buffer_len(stride, height.get())?;
        if buffer.len() != expected {
            return Err(RenderError::InvalidStride);
        }
        Ok(Self {
            width,
            height,
            stride,
            buffer,
        })
    }

    #[must_use]
    pub const fn width(&self) -> SurfaceWidth {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> SurfaceHeight {
        self.height
    }

    #[must_use]
    pub const fn stride(&self) -> StrideBytes {
        self.stride
    }

    #[must_use]
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    pub fn logical_bgra_bytes(&self) -> RenderResult<Vec<u8>> {
        let row_len = usize::try_from(self.width.get() * BYTES_PER_PIXEL)
            .map_err(|_| RenderError::ArithmeticOverflow)?;
        let height =
            usize::try_from(self.height.get()).map_err(|_| RenderError::ArithmeticOverflow)?;
        let mut out = Vec::with_capacity(
            row_len
                .checked_mul(height)
                .ok_or(RenderError::ArithmeticOverflow)?,
        );
        let stride =
            usize::try_from(self.stride.get()).map_err(|_| RenderError::ArithmeticOverflow)?;
        for y in 0..height {
            let row_start = y * stride;
            out.extend_from_slice(&self.buffer[row_start..row_start + row_len]);
        }
        Ok(out)
    }

    pub fn pixel(&self, x: u32, y: u32) -> RenderResult<PixelBgra> {
        if x >= self.width.get() || y >= self.height.get() {
            return Err(RenderError::InvalidRect);
        }
        let offset = self.pixel_offset(x, y)?;
        Ok(PixelBgra::new(
            self.buffer[offset],
            self.buffer[offset + 1],
            self.buffer[offset + 2],
            self.buffer[offset + 3],
        ))
    }

    pub(crate) fn bounds_rect(&self) -> RenderResult<Rect> {
        Rect::new(0, 0, self.width.get(), self.height.get())
    }

    pub(crate) fn fill_clipped_rect(&mut self, rect: Rect, color: PixelBgra) -> RenderResult<()> {
        for y in rect.y..i32::try_from(rect.bottom()).map_err(|_| RenderError::InvalidRect)? {
            for x in rect.x..i32::try_from(rect.right()).map_err(|_| RenderError::InvalidRect)? {
                self.set_pixel_checked(x, y, color)?;
            }
        }
        Ok(())
    }

    pub(crate) fn set_pixel_checked(
        &mut self,
        x: i32,
        y: i32,
        color: PixelBgra,
    ) -> RenderResult<()> {
        if x < 0 || y < 0 {
            return Ok(());
        }
        let x = u32::try_from(x).map_err(|_| RenderError::InvalidRect)?;
        let y = u32::try_from(y).map_err(|_| RenderError::InvalidRect)?;
        if x >= self.width.get() || y >= self.height.get() {
            return Ok(());
        }
        let offset = self.pixel_offset(x, y)?;
        self.buffer[offset..offset + 4].copy_from_slice(&color.bytes());
        Ok(())
    }

    fn pixel_offset(&self, x: u32, y: u32) -> RenderResult<usize> {
        let row = u64::from(y)
            .checked_mul(u64::from(self.stride.get()))
            .ok_or(RenderError::ArithmeticOverflow)?;
        let col = u64::from(x)
            .checked_mul(u64::from(BYTES_PER_PIXEL))
            .ok_or(RenderError::ArithmeticOverflow)?;
        usize::try_from(
            row.checked_add(col)
                .ok_or(RenderError::ArithmeticOverflow)?,
        )
        .map_err(|_| RenderError::ArithmeticOverflow)
    }
}
