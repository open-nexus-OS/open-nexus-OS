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
    let mut frame = FirstFrame { width, height, stride, pixels: vec![0u8; len] };

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
    (generated_wallpaper::WALLPAPER_WIDTH, generated_wallpaper::WALLPAPER_HEIGHT)
}

/// The theme-matched wallpaper as ROW-RLE (`resources/wallpapers/base/
/// default[.dark]`): `(data, row_offsets)`. Both variants bake at the same
/// decoded size, so a live theme switch only swaps the pointers. Row format:
/// runs of `[len:u16 LE][b g r a]`; row y = `data[rows[y]..rows[y+1]]`.
#[must_use]
pub const fn wallpaper_rle_for(dark: bool) -> (&'static [u8], &'static [u32]) {
    if dark {
        (generated_wallpaper::WALLPAPER_DARK_RLE, generated_wallpaper::WALLPAPER_DARK_ROWS)
    } else {
        (generated_wallpaper::WALLPAPER_RLE, generated_wallpaper::WALLPAPER_ROWS)
    }
}

/// Decode ONE per-row-QOI wallpaper row into `out` (opaque BGRA, width×4).
/// The SHARED decoder for the compositor and the first-frame composer —
/// mirrors the build-time encoder exactly (RUN/INDEX/DIFF/LUMA/RGB, state
/// reset per row). Bounded, heap-free, `no_std`.
pub fn decode_qoi_row(
    data: &'static [u8],
    rows: &'static [u32],
    width: usize,
    sy: usize,
    out: &mut [u8],
) -> Option<()> {
    let start = *rows.get(sy)? as usize;
    let end = *rows.get(sy + 1)? as usize;
    let row = data.get(start..end)?;
    let row_bytes = width * 4;
    if out.len() < row_bytes {
        return None;
    }
    let mut index = [[0u8; 3]; 64];
    let mut prev = [0u8, 0u8, 0u8]; // b, g, r
    let mut i = 0usize;
    let mut px = 0usize;
    while px < row_bytes {
        let b0 = *row.get(i)?;
        i += 1;
        match b0 {
            0b1111_1110 => {
                // RGB literal (r, g, b on the wire).
                let r = *row.get(i)?;
                let g = *row.get(i + 1)?;
                let b = *row.get(i + 2)?;
                i += 3;
                prev = [b, g, r];
            }
            _ => match b0 >> 6 {
                0b00 => {
                    prev = index[(b0 & 0x3f) as usize];
                }
                0b01 => {
                    let dr = ((b0 >> 4) & 0x03) as i16 - 2;
                    let dg = ((b0 >> 2) & 0x03) as i16 - 2;
                    let db = (b0 & 0x03) as i16 - 2;
                    prev = [
                        (prev[0] as i16 + db) as u8,
                        (prev[1] as i16 + dg) as u8,
                        (prev[2] as i16 + dr) as u8,
                    ];
                }
                0b10 => {
                    let dg = (b0 & 0x3f) as i16 - 32;
                    let b1 = *row.get(i)?;
                    i += 1;
                    let dr_dg = ((b1 >> 4) & 0x0f) as i16 - 8;
                    let db_dg = (b1 & 0x0f) as i16 - 8;
                    prev = [
                        (prev[0] as i16 + dg + db_dg) as u8,
                        (prev[1] as i16 + dg) as u8,
                        (prev[2] as i16 + dg + dr_dg) as u8,
                    ];
                }
                _ => {
                    // RUN of prev (1..=62).
                    let mut run = (b0 & 0x3f) as usize + 1;
                    while run > 0 && px < row_bytes {
                        out[px] = prev[0];
                        out[px + 1] = prev[1];
                        out[px + 2] = prev[2];
                        out[px + 3] = 255;
                        px += 4;
                        run -= 1;
                    }
                    continue;
                }
            },
        }
        let hash =
            (prev[2] as usize * 3 + prev[1] as usize * 5 + prev[0] as usize * 7 + 255 * 11) % 64;
        index[hash] = prev;
        out[px] = prev[0];
        out[px + 1] = prev[1];
        out[px + 2] = prev[2];
        out[px + 3] = 255;
        px += 4;
    }
    Some(())
}

/// Decode ONE wallpaper row (light variant) into `out` (BGRA, width*4).
fn wallpaper_row(sy: usize, out: &mut [u8]) -> Option<()> {
    let (data, rows) = wallpaper_rle_for(false);
    decode_qoi_row(data, rows, generated_wallpaper::WALLPAPER_WIDTH as usize, sy, out)
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
    // Row-RLE source: decode each needed source row once (rows repeat when
    // the frame is smaller than the wallpaper), then nearest-sample columns.
    let src_row_bytes = generated_wallpaper::WALLPAPER_WIDTH as usize * 4;
    let mut src_row = vec![0u8; src_row_bytes];
    let mut decoded_sy = usize::MAX;
    for y in 0..frame.height {
        let src_y = ((u64::from(y) * u64::from(generated_wallpaper::WALLPAPER_HEIGHT))
            / u64::from(frame.height)) as usize;
        if src_y != decoded_sy {
            wallpaper_row(src_y, &mut src_row).ok_or(SystemUiError::InvalidFrameDimensions)?;
            decoded_sy = src_y;
        }
        for x in 0..frame.width {
            let idx = (y as usize)
                .checked_mul(frame.stride as usize)
                .and_then(|row| row.checked_add((x as usize).checked_mul(4)?))
                .ok_or(SystemUiError::ArithmeticOverflow)?;
            let src_x = ((u64::from(x) * u64::from(generated_wallpaper::WALLPAPER_WIDTH))
                / u64::from(frame.width)) as usize;
            let src = src_x.checked_mul(4).ok_or(SystemUiError::ArithmeticOverflow)?;
            frame.pixels[idx..idx + 4].copy_from_slice(
                src_row.get(src..src + 4).ok_or(SystemUiError::InvalidFrameDimensions)?,
            );
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
