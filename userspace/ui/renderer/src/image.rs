// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Checked in-memory BGRA source image for frame blits.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::{RenderError, RenderResult};
use crate::limits::{BYTES_PER_PIXEL, MAX_IMAGE_PIXELS};
use crate::math::{checked_raw_buffer_len, ensure_row_arithmetic};
use crate::pixel::PixelBgra;
use crate::units::{ImageHeight, ImageWidth, StrideBytes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub(crate) width: ImageWidth,
    pub(crate) height: ImageHeight,
    stride: StrideBytes,
    pixels: Vec<u8>,
}

impl Image {
    pub fn from_bgra_checked(
        width: u32,
        height: u32,
        stride: u32,
        pixels: Vec<u8>,
    ) -> RenderResult<Self> {
        ensure_row_arithmetic(width)?;
        let width = ImageWidth::new(width)?;
        let height = ImageHeight::new(height)?;
        let pixel_count = u64::from(width.get())
            .checked_mul(u64::from(height.get()))
            .ok_or(RenderError::ArithmeticOverflow)?;
        if pixel_count > MAX_IMAGE_PIXELS {
            return Err(RenderError::ImageTooLarge);
        }
        let stride = StrideBytes::new_for_image(width, stride)?;
        let expected = checked_raw_buffer_len(stride, height.get())?;
        if pixels.len() != expected {
            return Err(RenderError::InvalidStride);
        }
        Ok(Self {
            width,
            height,
            stride,
            pixels,
        })
    }

    #[must_use]
    pub const fn width(&self) -> ImageWidth {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> ImageHeight {
        self.height
    }

    pub fn pixel(&self, x: u32, y: u32) -> RenderResult<PixelBgra> {
        if x >= self.width.get() || y >= self.height.get() {
            return Err(RenderError::InvalidRect);
        }
        let row = u64::from(y)
            .checked_mul(u64::from(self.stride.get()))
            .ok_or(RenderError::ArithmeticOverflow)?;
        let col = u64::from(x)
            .checked_mul(u64::from(BYTES_PER_PIXEL))
            .ok_or(RenderError::ArithmeticOverflow)?;
        let offset = usize::try_from(
            row.checked_add(col)
                .ok_or(RenderError::ArithmeticOverflow)?,
        )
        .map_err(|_| RenderError::ArithmeticOverflow)?;
        Ok(PixelBgra::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ))
    }
}
