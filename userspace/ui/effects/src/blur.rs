// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! 3×3 box blur kernel. Integer-only (no floating point), deterministic output.
//! Budget-capped via `EffectBudget` — skipped when budget exhausted.

use alloc::vec::Vec;

/// Applies a 3×3 box blur to an RGBA8888 region.
///
/// The input region has dimensions `(width, height)` with `stride` bytes per row.
/// `pixels` is modified in-place. Only the alpha channel contributes to weight
/// for color mixing (standard box-blur premultiplied alpha shortcut).
///
/// Returns the number of pixels actually blurred (for budget accounting).
pub fn blur_3x3(pixels: &mut [u8], width: u32, height: u32, stride: u32) -> u32 {
    if width < 3 || height < 3 {
        return 0;
    }

    let h = height as usize;
    let w = width as usize;
    let s = stride as usize;
    let mut blurred = Vec::with_capacity(w * h * 4);

    // Copy to temporary buffer, then blur back
    for y in 0..h {
        let row_start = y * s;
        blurred.extend_from_slice(&pixels[row_start..row_start + w * 4]);
    }

    let mut count = 0u32;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut a_sum = 0u32;

            for ky in 0..3u32 {
                for kx in 0..3u32 {
                    let idx = ((y - 1 + ky as usize) * w + (x - 1 + kx as usize)) * 4;
                    let a = blurred[idx + 3] as u32;
                    r_sum += blurred[idx] as u32 * a;
                    g_sum += blurred[idx + 1] as u32 * a;
                    b_sum += blurred[idx + 2] as u32 * a;
                    a_sum += a;
                }
            }

            let dst = y * s + x * 4;
            if a_sum > 0 {
                pixels[dst] = ((r_sum / a_sum).min(255)) as u8;
                pixels[dst + 1] = ((g_sum / a_sum).min(255)) as u8;
                pixels[dst + 2] = ((b_sum / a_sum).min(255)) as u8;
            }
            pixels[dst + 3] = ((a_sum / 9).min(255)) as u8;
            count += 1;
        }
    }

    count
}

/// Horizontal 1×3 blur (faster, for shadow passes).
/// Returns blurred pixel count.
pub fn blur_1x3_horizontal(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
) -> u32 {
    if width < 3 || height == 0 {
        return 0;
    }

    let h = height as usize;
    let w = width as usize;
    let s = stride as usize;
    let mut count = 0u32;

    // Temporary row buffer
    let mut row = Vec::with_capacity(w * 4);
    for y in 0..h {
        let row_start = y * s;
        row.clear();
        row.extend_from_slice(&pixels[row_start..row_start + w * 4]);

        for x in 1..w - 1 {
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for kx in 0..3 {
                let idx = (x - 1 + kx) * 4;
                let alpha = row[idx + 3] as u32;
                r += row[idx] as u32 * alpha;
                g += row[idx + 1] as u32 * alpha;
                b += row[idx + 2] as u32 * alpha;
                a += alpha;
            }
            let dst = row_start + x * 4;
            if a > 0 {
                pixels[dst] = ((r / a).min(255)) as u8;
                pixels[dst + 1] = ((g / a).min(255)) as u8;
                pixels[dst + 2] = ((b / a).min(255)) as u8;
            }
            pixels[dst + 3] = ((a / 3).min(255)) as u8;
            count += 1;
        }
    }

    count
}
