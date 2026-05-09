// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounds and arithmetic guards for dimensions, stride, and damage rectangles.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::error::{Result, WindowdError};

pub(crate) const MAX_SURFACES: usize = 32;
pub(crate) const MAX_LAYERS: usize = 16;
pub(crate) const MAX_DAMAGE_RECTS: usize = 16;
pub(crate) const MAX_DIMENSION: u32 = 4096;
pub(crate) const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }
}

pub(crate) fn validate_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(WindowdError::InvalidDimensions);
    }
    let stride = checked_stride(width)?;
    let _ = checked_len(stride, height)?;
    Ok(())
}

pub(crate) fn validate_damage(width: u32, height: u32, damage: &[Rect]) -> Result<()> {
    if damage.is_empty() || damage.len() > MAX_DAMAGE_RECTS {
        return Err(WindowdError::TooManyDamageRects);
    }
    for rect in damage {
        if rect.width == 0 || rect.height == 0 {
            return Err(WindowdError::InvalidDamage);
        }
        let end_x = rect.x.checked_add(rect.width).ok_or(WindowdError::ArithmeticOverflow)?;
        let end_y = rect.y.checked_add(rect.height).ok_or(WindowdError::ArithmeticOverflow)?;
        if end_x > width || end_y > height {
            return Err(WindowdError::InvalidDamage);
        }
    }
    Ok(())
}

pub(crate) fn checked_stride(width: u32) -> Result<u32> {
    let bytes = width.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
    let aligned = bytes.checked_add(63).ok_or(WindowdError::ArithmeticOverflow)? / 64 * 64;
    Ok(aligned)
}

pub(crate) fn checked_len(stride: u32, height: u32) -> Result<usize> {
    let len =
        (stride as usize).checked_mul(height as usize).ok_or(WindowdError::ArithmeticOverflow)?;
    if len > MAX_TOTAL_BYTES {
        return Err(WindowdError::SurfaceTooLarge);
    }
    Ok(len)
}
