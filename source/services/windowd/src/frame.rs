// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Frame/layer composition primitives for BGRA blit in `windowd`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Composition and hit-test behavior covered by `ui_windowd_host` and `ui_v2a_host`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::vec::Vec;

use crate::buffer::SurfaceBuffer;
use crate::error::{Result, WindowdError};
use crate::ids::SurfaceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    pub surface: SurfaceId,
    pub x: i32,
    pub y: i32,
    pub z: i16,
}

impl Layer {
    pub(crate) fn contains_point(self, surface_width: u32, surface_height: u32, x: i32, y: i32) -> bool {
        let Ok(width) = i32::try_from(surface_width) else {
            return false;
        };
        let Ok(height) = i32::try_from(surface_height) else {
            return false;
        };
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(width)
            && y < self.y.saturating_add(height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
}

pub(crate) fn blit_surface(frame: &mut Frame, layer: &Layer, buffer: &SurfaceBuffer) -> Result<()> {
    for sy in 0..buffer.height as i32 {
        let dy = sy.checked_add(layer.y).ok_or(WindowdError::ArithmeticOverflow)?;
        if dy < 0 || dy >= frame.height as i32 {
            continue;
        }
        for sx in 0..buffer.width as i32 {
            let dx = sx.checked_add(layer.x).ok_or(WindowdError::ArithmeticOverflow)?;
            if dx < 0 || dx >= frame.width as i32 {
                continue;
            }
            let src_idx = (sy as usize)
                .checked_mul(buffer.stride as usize)
                .and_then(|base| base.checked_add((sx as usize).checked_mul(4)?))
                .ok_or(WindowdError::ArithmeticOverflow)?;
            if buffer.pixels[src_idx + 3] == 0 {
                continue;
            }
            let dst_idx = (dy as usize)
                .checked_mul(frame.stride as usize)
                .and_then(|base| base.checked_add((dx as usize).checked_mul(4)?))
                .ok_or(WindowdError::ArithmeticOverflow)?;
            frame.pixels[dst_idx..dst_idx + 4]
                .copy_from_slice(&buffer.pixels[src_idx..src_idx + 4]);
        }
    }
    Ok(())
}
