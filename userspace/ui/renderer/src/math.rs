// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Checked arithmetic helpers shared by renderer construction and drawing.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::{RenderError, RenderResult};
use crate::geometry::Rect;
use crate::limits::BYTES_PER_PIXEL;
use crate::units::StrideBytes;

pub(crate) fn checked_row_bytes(width: u32) -> RenderResult<u32> {
    width
        .checked_mul(BYTES_PER_PIXEL)
        .ok_or(RenderError::ArithmeticOverflow)
}

pub(crate) fn ensure_row_arithmetic(width: u32) -> RenderResult<()> {
    checked_row_bytes(width).map(|_| ())
}

pub(crate) fn align_up(value: u32, align: u32) -> RenderResult<u32> {
    if align == 0 {
        return Err(RenderError::InvalidStride);
    }
    let adjusted = value
        .checked_add(align - 1)
        .ok_or(RenderError::ArithmeticOverflow)?;
    Ok((adjusted / align) * align)
}

pub(crate) fn checked_buffer_len(stride: StrideBytes, height: u32) -> RenderResult<usize> {
    checked_raw_buffer_len(stride, height)
}

pub(crate) fn checked_raw_buffer_len(stride: StrideBytes, height: u32) -> RenderResult<usize> {
    let len = u64::from(stride.get())
        .checked_mul(u64::from(height))
        .ok_or(RenderError::ArithmeticOverflow)?;
    usize::try_from(len).map_err(|_| RenderError::ArithmeticOverflow)
}

pub(crate) fn checked_i32_extent(start: i32, size: u32) -> RenderResult<i64> {
    let end = i64::from(start)
        .checked_add(i64::from(size))
        .ok_or(RenderError::ArithmeticOverflow)?;
    if end > i64::from(i32::MAX) {
        return Err(RenderError::InvalidRect);
    }
    Ok(end)
}

pub(crate) fn rounded_rect_covers(rect: Rect, radius: u32, x: i32, y: i32) -> RenderResult<bool> {
    if radius == 0 {
        return Ok(true);
    }
    let local_x = u32::try_from(x - rect.x).map_err(|_| RenderError::InvalidRect)?;
    let local_y = u32::try_from(y - rect.y).map_err(|_| RenderError::InvalidRect)?;

    let dx = if local_x < radius {
        radius - local_x
    } else if local_x >= rect.width - radius {
        local_x - (rect.width - radius - 1)
    } else {
        0
    };
    let dy = if local_y < radius {
        radius - local_y
    } else if local_y >= rect.height - radius {
        local_y - (rect.height - radius - 1)
    } else {
        0
    };
    if dx == 0 || dy == 0 {
        return Ok(true);
    }
    let dist = u64::from(dx)
        .checked_mul(u64::from(dx))
        .and_then(|left| {
            u64::from(dy)
                .checked_mul(u64::from(dy))
                .and_then(|right| left.checked_add(right))
        })
        .ok_or(RenderError::ArithmeticOverflow)?;
    let radius_sq = u64::from(radius)
        .checked_mul(u64::from(radius))
        .ok_or(RenderError::ArithmeticOverflow)?;
    Ok(dist <= radius_sq)
}
