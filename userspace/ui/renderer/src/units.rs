// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Checked renderer newtypes for dimensions, stride, and damage count.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::{RenderError, RenderResult};
use crate::limits::{
    BYTES_PER_PIXEL, MAX_DAMAGE_RECTS, MAX_FRAME_HEIGHT, MAX_FRAME_WIDTH, MAX_IMAGE_HEIGHT,
    MAX_IMAGE_WIDTH, STRIDE_ALIGNMENT,
};
use crate::math::{align_up, checked_row_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct SurfaceWidth(u32);

impl SurfaceWidth {
    pub fn new(raw: u32) -> RenderResult<Self> {
        if raw == 0 {
            return Err(RenderError::InvalidDimensions);
        }
        if raw > MAX_FRAME_WIDTH {
            return Err(RenderError::FrameTooLarge);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct SurfaceHeight(u32);

impl SurfaceHeight {
    pub fn new(raw: u32) -> RenderResult<Self> {
        if raw == 0 {
            return Err(RenderError::InvalidDimensions);
        }
        if raw > MAX_FRAME_HEIGHT {
            return Err(RenderError::FrameTooLarge);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct StrideBytes(u32);

impl StrideBytes {
    pub fn for_width(width: SurfaceWidth) -> RenderResult<Self> {
        let row_bytes = checked_row_bytes(width.get())?;
        let stride = align_up(row_bytes, STRIDE_ALIGNMENT)?;
        Ok(Self(stride))
    }

    pub fn new_for_frame(width: SurfaceWidth, raw: u32) -> RenderResult<Self> {
        let min = checked_row_bytes(width.get())?;
        if raw < min || !is_multiple_of(raw, STRIDE_ALIGNMENT) {
            return Err(RenderError::InvalidStride);
        }
        Ok(Self(raw))
    }

    pub fn new_for_image(width: ImageWidth, raw: u32) -> RenderResult<Self> {
        let min = checked_row_bytes(width.get())?;
        if raw < min || !is_multiple_of(raw, BYTES_PER_PIXEL) {
            return Err(RenderError::InvalidStride);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

fn is_multiple_of(value: u32, divisor: u32) -> bool {
    divisor != 0 && value.checked_rem(divisor) == Some(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ImageWidth(u32);

impl ImageWidth {
    pub fn new(raw: u32) -> RenderResult<Self> {
        if raw == 0 {
            return Err(RenderError::InvalidDimensions);
        }
        if raw > MAX_IMAGE_WIDTH {
            return Err(RenderError::ImageTooLarge);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ImageHeight(u32);

impl ImageHeight {
    pub fn new(raw: u32) -> RenderResult<Self> {
        if raw == 0 {
            return Err(RenderError::InvalidDimensions);
        }
        if raw > MAX_IMAGE_HEIGHT {
            return Err(RenderError::ImageTooLarge);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct DamageRectCount(u16);

impl DamageRectCount {
    pub fn new(raw: u16) -> RenderResult<Self> {
        if raw == 0 || raw > MAX_DAMAGE_RECTS {
            return Err(RenderError::DamageOverflow);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}
