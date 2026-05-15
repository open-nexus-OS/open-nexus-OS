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

const WALLPAPER_JPEG_BYTES: &[u8] =
    include_bytes!("../../../../resources/wallpapers/base/default.jpeg");
mod generated_wallpaper {
    include!(concat!(env!("OUT_DIR"), "/wallpaper_generated.rs"));
}

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
    let mut frame = FirstFrame {
        width,
        height,
        stride,
        pixels: vec![0u8; len],
    };

    fill_wallpaper(&mut frame)?;
    Ok(frame)
}

#[must_use]
pub fn wallpaper_source_is_jpeg() -> bool {
    WALLPAPER_JPEG_BYTES.len() >= 3
        && WALLPAPER_JPEG_BYTES[0] == 0xff
        && WALLPAPER_JPEG_BYTES[1] == 0xd8
        && WALLPAPER_JPEG_BYTES[2] == 0xff
}

#[must_use]
pub const fn wallpaper_decoded_size() -> (u32, u32) {
    (
        generated_wallpaper::WALLPAPER_WIDTH,
        generated_wallpaper::WALLPAPER_HEIGHT,
    )
}

#[must_use]
pub const fn wallpaper_bgra() -> &'static [u8] {
    generated_wallpaper::WALLPAPER_BGRA
}

pub fn frame_checksum(frame: &FirstFrame) -> u32 {
    frame.pixels.chunks_exact(4).fold(0_u32, |acc, pixel| {
        acc.wrapping_add(u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]))
    })
}

fn fill_wallpaper(frame: &mut FirstFrame) -> Result<()> {
    if !wallpaper_source_is_jpeg() {
        return Err(SystemUiError::InvalidFrameDimensions);
    }
    for y in 0..frame.height {
        for x in 0..frame.width {
            let idx = (y as usize)
                .checked_mul(frame.stride as usize)
                .and_then(|row| row.checked_add((x as usize).checked_mul(4)?))
                .ok_or(SystemUiError::ArithmeticOverflow)?;
            let src_x = ((u64::from(x) * u64::from(generated_wallpaper::WALLPAPER_WIDTH))
                / u64::from(frame.width)) as usize;
            let src_y = ((u64::from(y) * u64::from(generated_wallpaper::WALLPAPER_HEIGHT))
                / u64::from(frame.height)) as usize;
            let src = src_y
                .checked_mul(generated_wallpaper::WALLPAPER_WIDTH as usize * 4)
                .and_then(|row| row.checked_add(src_x.checked_mul(4)?))
                .ok_or(SystemUiError::ArithmeticOverflow)?;
            frame.pixels[idx..idx + 4].copy_from_slice(
                generated_wallpaper::WALLPAPER_BGRA
                    .get(src..src + 4)
                    .ok_or(SystemUiError::InvalidFrameDimensions)?,
            );
        }
    }
    Ok(())
}

fn checked_stride(width: u32) -> Result<u32> {
    let bytes = width
        .checked_mul(4)
        .ok_or(SystemUiError::ArithmeticOverflow)?;
    bytes
        .checked_add(63)
        .ok_or(SystemUiError::ArithmeticOverflow)
        .map(|v| v / 64 * 64)
}

fn checked_len(stride: u32, height: u32) -> Result<usize> {
    (stride as usize)
        .checked_mul(height as usize)
        .ok_or(SystemUiError::ArithmeticOverflow)
}
