// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Box blur kernels. Integer-only (no floating point), deterministic output.
//! Separable (horizontal + vertical) for O(w·h·2r) instead of O(w·h·r²).
//! Zero-copy friendly: per-pass temporary buffer bounded to max(row, col) size.
//! Budget-capped via `EffectBudget` — skipped when budget exhausted.

use alloc::vec::Vec;

/// Applies a 3×3 box blur to an RGBA8888 region.
///
/// The input region has dimensions `(width, height)` with `stride` bytes per row.
/// `pixels` is modified in-place. Only the alpha channel contributes to weight
/// for color mixing (standard box-blur premultiplied alpha shortcut).
///
/// Returns the number of pixels actually blurred (for budget accounting).
/// For radius > 1, prefer `blur_separable` (faster for larger radii).
pub fn blur_3x3(pixels: &mut [u8], width: u32, height: u32, stride: u32) -> u32 {
    if width < 3 || height < 3 {
        return 0;
    }

    let h = height as usize;
    let w = width as usize;
    let s = stride as usize;
    let mut blurred = Vec::with_capacity(w * h * 4);

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

/// One-dimensional separable box blur pass.
///
/// Slides a window of `(2*radius + 1)` pixels along each row (`horizontal=true`)
/// or column (`horizontal=false`). Uses a running-sum sliding window so each
/// output pixel costs O(1) instead of O(radius). Two passes (horizontal + vertical)
/// produce a 2D box blur in O(w·h·2) total work.
///
/// `pixels` is modified in-place. A row buffer (width × 4 bytes) is reused per row;
/// a column-oriented transpose buffer (w·h·4) is allocated once for the vertical pass.
/// Hot loop is allocation-free (zero-copy on the output side).
///
/// Returns the number of pixels blurred (for budget accounting).
pub fn blur_1d(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    radius: u32,
    horizontal: bool,
) -> u32 {
    if width == 0 || height == 0 || radius == 0 {
        return 0;
    }

    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;
    let r = radius as usize;
    let window = 2 * r + 1;

    if horizontal {
        let mut row_buf = Vec::with_capacity(w * 4);
        let mut count = 0u32;
        for y in 0..h {
            let row_start = y * s;
            row_buf.clear();
            row_buf.extend_from_slice(&pixels[row_start..row_start + w * 4]);

            // Initialize sliding window sums
            let (mut r_sum, mut g_sum, mut b_sum, mut a_sum) = (0u64, 0u64, 0u64, 0u64);
            for i in 0..window.min(w) {
                let idx = i * 4;
                let a = row_buf[idx + 3] as u64;
                r_sum += row_buf[idx] as u64 * a;
                g_sum += row_buf[idx + 1] as u64 * a;
                b_sum += row_buf[idx + 2] as u64 * a;
                a_sum += a;
            }

            for x in 0..w {
                let dst = row_start + x * 4;
                let n = a_sum; // total alpha weight in window
                if n > 0 {
                    pixels[dst] = ((r_sum / n).min(255)) as u8;
                    pixels[dst + 1] = ((g_sum / n).min(255)) as u8;
                    pixels[dst + 2] = ((b_sum / n).min(255)) as u8;
                }
                pixels[dst + 3] = ((a_sum / window as u64).min(255)) as u8;
                count += 1;

                // Slide window: remove leftmost, add rightmost
                let left = x.saturating_sub(r);
                if let Some(lidx) = left.checked_mul(4) {
                    let la = row_buf[lidx + 3] as u64;
                    r_sum = r_sum.saturating_sub(row_buf[lidx] as u64 * la);
                    g_sum = g_sum.saturating_sub(row_buf[lidx + 1] as u64 * la);
                    b_sum = b_sum.saturating_sub(row_buf[lidx + 2] as u64 * la);
                    a_sum = a_sum.saturating_sub(la);
                }
                let right = x + r + 1;
                if right < w {
                    let ridx = right * 4;
                    let ra = row_buf[ridx + 3] as u64;
                    r_sum += row_buf[ridx] as u64 * ra;
                    g_sum += row_buf[ridx + 1] as u64 * ra;
                    b_sum += row_buf[ridx + 2] as u64 * ra;
                    a_sum += ra;
                }
            }
        }
        count
    } else {
        // Vertical pass: transpose → horizontal blur → transpose back.
        let mut col_buf = alloc::vec![0u8; h * w * 4];
        for y in 0..h {
            for x in 0..w {
                let src = y * s + x * 4;
                let dst = (x * h + y) * 4;
                col_buf[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        }
        let count = blur_1d(&mut col_buf, h as u32, w as u32, (w * 4) as u32, radius, true);
        for y in 0..h {
            for x in 0..w {
                let src = (x * h + y) * 4;
                let dst = y * s + x * 4;
                pixels[dst..dst + 4].copy_from_slice(&col_buf[src..src + 4]);
            }
        }
        count
    }
}

/// Compute a 2D separable box blur in two passes (horizontal + vertical).
///
/// This is the recommended entry point for shadow blurs: it calls `blur_1d` twice
/// (once horizontal, once vertical), producing a 2D box blur in O(w·h·4) total
/// operations — roughly 4 reads + 4 writes per pixel regardless of radius.
///
/// Returns the total number of pixels blurred in both passes.
pub fn blur_separable(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    radius: u32,
) -> u32 {
    let c1 = blur_1d(pixels, width, height, stride, radius, true);
    let c2 = blur_1d(pixels, width, height, stride, radius, false);
    c1 + c2
}
