// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic SystemUI first-frame composition for TASK-0055C.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::vec;
use alloc::vec::Vec;

use crate::profile::{Result, SystemUiError};
use crate::shell::{resolve_desktop_shell, ResolvedShell};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
}

pub fn compose_first_frame() -> Result<FirstFrame> {
    compose_for_shell(&resolve_desktop_shell()?)
}

pub fn compose_for_shell(resolved: &ResolvedShell) -> Result<FirstFrame> {
    let width = resolved.shell.first_frame.width;
    let height = resolved.shell.first_frame.height;
    let stride = checked_stride(width)?;
    let len = checked_len(stride, height)?;
    let mut frame = FirstFrame { width, height, stride, pixels: vec![0u8; len] };

    fill_rect(&mut frame, 0, 0, width, height, [0x24, 0x28, 0x34, 0xff])?;
    fill_rect(&mut frame, 0, 0, width, 16, [0x80, 0x50, 0x20, 0xff])?;
    fill_rect(&mut frame, 0, 16, 10, height - 16, [0x40, 0x28, 0x18, 0xff])?;
    fill_rect(&mut frame, 18, 28, 92, 54, [0x48, 0x80, 0x38, 0xff])?;
    fill_rect(&mut frame, 118, 28, 24, 8, [0xb0, 0xa0, 0x40, 0xff])?;
    Ok(frame)
}

pub fn frame_checksum(frame: &FirstFrame) -> u32 {
    frame.pixels.chunks_exact(4).fold(0_u32, |acc, pixel| {
        acc.wrapping_add(u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
    })
}

fn fill_rect(
    frame: &mut FirstFrame,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    bgra: [u8; 4],
) -> Result<()> {
    let end_x = x.checked_add(width).ok_or(SystemUiError::ArithmeticOverflow)?;
    let end_y = y.checked_add(height).ok_or(SystemUiError::ArithmeticOverflow)?;
    if width == 0 || height == 0 || end_x > frame.width || end_y > frame.height {
        return Err(SystemUiError::InvalidFrameDimensions);
    }
    for py in y..end_y {
        for px in x..end_x {
            let idx = (py as usize)
                .checked_mul(frame.stride as usize)
                .and_then(|row| row.checked_add((px as usize).checked_mul(4)?))
                .ok_or(SystemUiError::ArithmeticOverflow)?;
            frame.pixels[idx..idx + 4].copy_from_slice(&bgra);
        }
    }
    Ok(())
}

fn checked_stride(width: u32) -> Result<u32> {
    let bytes = width.checked_mul(4).ok_or(SystemUiError::ArithmeticOverflow)?;
    bytes.checked_add(63).ok_or(SystemUiError::ArithmeticOverflow).map(|v| v / 64 * 64)
}

fn checked_len(stride: u32, height: u32) -> Result<usize> {
    (stride as usize).checked_mul(height as usize).ok_or(SystemUiError::ArithmeticOverflow)
}
