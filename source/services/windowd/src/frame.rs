// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

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
