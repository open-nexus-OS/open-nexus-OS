// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Surface buffer and VMO-shaped validation rules for `windowd`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{Result, WindowdError};
use crate::geometry::{checked_len, checked_stride, validate_dimensions};
use crate::ids::{CallerCtx, CallerId, VmoHandleId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8888,
    Unsupported(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmoRights {
    pub read: bool,
    pub write: bool,
}

impl VmoRights {
    pub const fn read_write() -> Self {
        Self { read: true, write: true }
    }

    pub const fn read_only() -> Self {
        Self { read: true, write: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmoHandle {
    pub id: VmoHandleId,
    pub owner: CallerId,
    pub rights: VmoRights,
    pub byte_len: usize,
    pub surface_buffer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceBuffer {
    pub handle: VmoHandle,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub pixels: Vec<u8>,
}

impl SurfaceBuffer {
    pub fn solid(
        caller: CallerCtx,
        handle_id: u64,
        width: u32,
        height: u32,
        bgra: [u8; 4],
    ) -> Result<Self> {
        validate_dimensions(width, height)?;
        let stride = checked_stride(width)?;
        let len = checked_len(stride, height)?;
        let mut pixels = vec![0u8; len];
        for y in 0..height as usize {
            let row = y.checked_mul(stride as usize).ok_or(WindowdError::ArithmeticOverflow)?;
            for x in 0..width as usize {
                let idx = row
                    .checked_add(x.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?)
                    .ok_or(WindowdError::ArithmeticOverflow)?;
                pixels[idx..idx + 4].copy_from_slice(&bgra);
            }
        }
        Ok(Self {
            handle: VmoHandle {
                id: VmoHandleId::new(handle_id),
                owner: caller.caller_id(),
                rights: VmoRights::read_write(),
                byte_len: len,
                surface_buffer: true,
            },
            width,
            height,
            stride,
            format: PixelFormat::Bgra8888,
            pixels,
        })
    }
}

pub(crate) fn validate_buffer(caller: CallerCtx, buffer: &SurfaceBuffer) -> Result<()> {
    if buffer.handle.id == VmoHandleId::new(0) {
        return Err(WindowdError::MissingVmoHandle);
    }
    if buffer.handle.owner != caller.caller_id() {
        return Err(WindowdError::ForgedVmoHandle);
    }
    if !buffer.handle.rights.read || !buffer.handle.rights.write {
        return Err(WindowdError::WrongVmoRights);
    }
    if !buffer.handle.surface_buffer {
        return Err(WindowdError::NonSurfaceBuffer);
    }
    validate_dimensions(buffer.width, buffer.height)?;
    if buffer.format != PixelFormat::Bgra8888 {
        return Err(WindowdError::UnsupportedFormat);
    }
    let min_stride = buffer.width.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
    if buffer.stride < min_stride || buffer.stride.checked_rem(64) != Some(0) {
        return Err(WindowdError::InvalidStride);
    }
    let len = checked_len(buffer.stride, buffer.height)?;
    if buffer.handle.byte_len != len || buffer.pixels.len() != len {
        return Err(WindowdError::BufferLengthMismatch);
    }
    Ok(())
}
