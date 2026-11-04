// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Backdrop blur primitives — a fast separable box blur (the CPU executor's
//! trick: constant cost a pixel, named here and judged by the gaussian) and the
//! separable gaussian of the kernel table (`frame::kernels`, ADR-0094: the same
//! weights the shader pipelines are built with, at full resolution — the oracle
//! every executor's blur is judged by). Saturation is a separate opt-in pass
//! ([`saturate`]) the caller applies afterward, so a backend that historically
//! did not boost saturation keeps its exact output.
//!
//! Both passes are **allocation-free**: the caller supplies the row and column
//! scratch slices. The live driver passes fixed stack buffers (no per-frame heap
//! traffic — the hard rule that keeps the bump-allocated services from OOMing);
//! the host reference passes `Vec`-backed slices. A scratch slice that is too
//! small returns [`RasterError::ScratchTooSmall`] rather than truncating.

#![forbid(unsafe_code)]

use super::surface::Surface;
use super::RasterError;
use crate::frame::kernels::{Gaussian, MAX_FULL_TAPS};

/// The gaussian kernel's taps (`radius` up to the table's largest → `2·40 + 1`).
const MAX_KERNEL: usize = 2 * MAX_FULL_TAPS + 1;

/// Separable box blur of `(x, y, w, h)`. The classic fast backdrop blur;
/// `scratch_row` needs `≥ w·4` bytes and `scratch_col` `≥ h·4` bytes. Apply
/// [`saturate`] separately if a saturation boost is wanted.
#[allow(clippy::too_many_arguments)]
pub fn blur_box(
    s: &mut Surface,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    scratch_row: &mut [u8],
    scratch_col: &mut [u8],
) -> Result<(), RasterError> {
    if radius == 0 {
        return Ok(());
    }
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    let r = radius as usize;
    let pixels = (end_x - x) as usize;
    let rows = (end_y - y) as usize;
    if pixels == 0 || rows == 0 {
        return Ok(());
    }
    let stride = s.stride();
    let row_bytes = pixels * 4;
    let col_bytes = rows * 4;
    if scratch_row.len() < row_bytes || scratch_col.len() < col_bytes {
        return Err(RasterError::ScratchTooSmall);
    }
    let buf = s.buf_mut();

    // Horizontal pass: sliding-window box average per row.
    for py in y..end_y {
        let row_start = (py as usize * (width as usize) + x as usize) * 4;
        if row_start + row_bytes > buf.len() {
            continue;
        }
        scratch_row[..row_bytes].copy_from_slice(&buf[row_start..row_start + row_bytes]);
        let mut sums = [0u64; 4];
        let mut left = 0usize;
        let mut right = r.min(pixels.saturating_sub(1));
        for j in left..=right {
            let bi = j * 4;
            for c in 0..4 {
                sums[c] += scratch_row[bi + c] as u64;
            }
        }
        for i in 0..pixels {
            let count = (right - left + 1) as u64;
            let di = row_start + i * 4;
            for c in 0..4 {
                buf[di + c] = (sums[c] / count.max(1)).min(255) as u8;
            }
            if i + 1 < pixels {
                let next_left = (i + 1).saturating_sub(r);
                if next_left > left {
                    let bi = left * 4;
                    for c in 0..4 {
                        sums[c] = sums[c].saturating_sub(scratch_row[bi + c] as u64);
                    }
                    left = next_left;
                }
                let next_right = (i + 1 + r).min(pixels.saturating_sub(1));
                if next_right > right {
                    right = next_right;
                    let bi = right * 4;
                    for c in 0..4 {
                        sums[c] += scratch_row[bi + c] as u64;
                    }
                }
            }
        }
    }

    // Vertical pass.
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..rows {
            let src = (y as usize + row_i) * stride + col_off;
            if src + 4 <= buf.len() {
                scratch_col[row_i * 4..row_i * 4 + 4].copy_from_slice(&buf[src..src + 4]);
            }
        }
        let mut sums = [0u64; 4];
        let mut top = 0usize;
        let mut bot = r.min(rows.saturating_sub(1));
        for j in top..=bot {
            let bi = j * 4;
            for c in 0..4 {
                sums[c] += scratch_col[bi + c] as u64;
            }
        }
        for i in 0..rows {
            let count = (bot - top + 1) as u64;
            let dst = (y as usize + i) * stride + col_off;
            for c in 0..4 {
                if dst + c < buf.len() {
                    buf[dst + c] = (sums[c] / count.max(1)).min(255) as u8;
                }
            }
            if i + 1 < rows {
                let ntop = (i + 1).saturating_sub(r);
                if ntop > top {
                    let bi = top * 4;
                    for c in 0..4 {
                        sums[c] = sums[c].saturating_sub(scratch_col[bi + c] as u64);
                    }
                    top = ntop;
                }
                let nbot = (i + 1 + r).min(rows.saturating_sub(1));
                if nbot > bot {
                    bot = nbot;
                    let bi = bot * 4;
                    for c in 0..4 {
                        sums[c] += scratch_col[bi + c] as u64;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Separable gaussian blur of `(x, y, w, h)` with the kernel table's weights at
/// full resolution (`Gaussian::full`: σ = radius/2, taps out to ±radius; a radius
/// past the table's largest is held to it). The same convolution the shader
/// pipelines run at reduced resolution, so it is the oracle their pictures are
/// judged by (`frame::oracle`). Same scratch requirements as [`blur_box`]. Apply
/// [`saturate`] separately for a boost.
#[allow(clippy::too_many_arguments)]
pub fn blur_gaussian(
    s: &mut Surface,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    scratch_row: &mut [u8],
    scratch_col: &mut [u8],
) -> Result<(), RasterError> {
    if radius == 0 {
        return Ok(());
    }
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    let weights = Gaussian::full(radius).weights();
    let r = weights.taps();
    let pixels = (end_x - x) as usize;
    let rows = (end_y - y) as usize;
    if pixels == 0 || rows == 0 {
        return Ok(());
    }
    let stride = s.stride();
    let row_bytes = pixels * 4;
    let col_bytes = rows * 4;
    if scratch_row.len() < row_bytes || scratch_col.len() < col_bytes {
        return Err(RasterError::ScratchTooSmall);
    }

    // The table's kernel, `-r..=r`.
    let mut kernel = [0.0_f32; MAX_KERNEL];
    for (k, (_, w)) in kernel.iter_mut().zip(weights.iter()) {
        *k = w;
    }
    let k_len = 2 * r + 1;
    let buf = s.buf_mut();

    // Horizontal pass: convolve each row.
    for py in y..end_y {
        let row_start = (py as usize * (width as usize) + x as usize) * 4;
        if row_start + row_bytes > buf.len() {
            continue;
        }
        scratch_row[..row_bytes].copy_from_slice(&buf[row_start..row_start + row_bytes]);
        for i in 0..pixels {
            let mut acc = [0.0_f32; 4];
            for (ki, &weight) in kernel.iter().enumerate().take(k_len) {
                let src_i = (i as i32 + ki as i32 - r as i32).clamp(0, pixels as i32 - 1) as usize;
                let si = src_i * 4;
                for c in 0..4 {
                    acc[c] += scratch_row[si + c] as f32 * weight;
                }
            }
            let di = row_start + i * 4;
            for c in 0..4 {
                buf[di + c] = libm::roundf(acc[c]).clamp(0.0, 255.0) as u8;
            }
        }
    }

    // Vertical pass: convolve each column.
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..rows {
            let src = (y as usize + row_i) * stride + col_off;
            if src + 4 <= buf.len() {
                scratch_col[row_i * 4..row_i * 4 + 4].copy_from_slice(&buf[src..src + 4]);
            }
        }
        for i in 0..rows {
            let mut acc = [0.0_f32; 4];
            for (ki, &weight) in kernel.iter().enumerate().take(k_len) {
                let src_i = (i as i32 + ki as i32 - r as i32).clamp(0, rows as i32 - 1) as usize;
                let si = src_i * 4;
                for c in 0..4 {
                    acc[c] += scratch_col[si + c] as f32 * weight;
                }
            }
            let di = (y as usize + i) * stride + col_off;
            for c in 0..4 {
                if di + c < buf.len() {
                    buf[di + c] = libm::roundf(acc[c]).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    Ok(())
}

/// In-place saturation adjustment of `(x, y, w, h)`: each channel is lerped
/// toward its luma (`gray + (c - gray)·factor`). A no-op at 0 % / 100 %. An
/// opt-in pass — callers that want the glass saturation boost run this after a
/// blur; callers that historically did not simply skip it.
pub fn saturate(s: &mut Surface, x: u32, y: u32, w: u32, h: u32, saturation_pct: u32) {
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    if saturation_pct == 0 || saturation_pct == 100 || end_x <= x || end_y <= y {
        return;
    }
    let factor = saturation_pct as f32 / 100.0;
    let stride = s.stride();
    let buf = s.buf_mut();
    let row_len = (end_x - x) as usize * 4;
    for py in y..end_y {
        let row_off = py as usize * stride + x as usize * 4;
        for off in (row_off..row_off + row_len).step_by(4) {
            if off + 4 > buf.len() {
                continue;
            }
            let b = buf[off] as f32;
            let g = buf[off + 1] as f32;
            let r = buf[off + 2] as f32;
            let gray = 0.299 * r + 0.587 * g + 0.114 * b;
            buf[off] = (gray + (b - gray) * factor).clamp(0.0, 255.0) as u8;
            buf[off + 1] = (gray + (g - gray) * factor).clamp(0.0, 255.0) as u8;
            buf[off + 2] = (gray + (r - gray) * factor).clamp(0.0, 255.0) as u8;
        }
    }
}
