// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Box blur kernels (3×3, separable, dual-kawase) for TASK-0059 / RFC-0058 Phase 6.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 21 tests (tests/ui_v4_host/src/lib.rs — blur_separable, blur_1d)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
//! Box blur kernels. Integer-only (no floating point), deterministic output.
//! Separable (horizontal + vertical) for O(w·h·2r) instead of O(w·h·r²).
//! Zero-copy friendly: per-pass temporary buffer bounded to max(row, col) size.
//! Budget-capped via `EffectBudget` — skipped when budget exhausted.

use alloc::vec::Vec;

type PyramidLevel = (Vec<u8>, u32, u32, u32);

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
pub fn blur_1x3_horizontal(pixels: &mut [u8], width: u32, height: u32, stride: u32) -> u32 {
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
        let count = blur_1d(
            &mut col_buf,
            h as u32,
            w as u32,
            (h * 4) as u32,
            radius,
            true,
        );
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

/// Zero-allocation variant of `blur_1d`.
///
/// Identical algorithm but takes pre-allocated scratch buffers instead of
/// allocating internally. Safe for OS bump-allocator (no heap allocations
/// in the hot path).
///
/// - `row_scratch`: at least `max(width, height) * 4` bytes (reused per row/column)
/// - `col_scratch`: at least `width * height * 4` bytes (only used for vertical pass)
///
/// Returns the number of pixels blurred.
pub fn blur_1d_zero_alloc(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    radius: u32,
    horizontal: bool,
    row_scratch: &mut [u8],
    col_scratch: &mut [u8],
) -> u32 {
    if width == 0 || height == 0 || radius == 0 {
        return 0;
    }

    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;
    let row_bytes = w.saturating_mul(4);
    let col_bytes = w.saturating_mul(h).saturating_mul(4);
    if pixels.len()
        < h.saturating_sub(1)
            .saturating_mul(s)
            .saturating_add(row_bytes)
        || row_scratch.len() < w.max(h).saturating_mul(4)
        || (!horizontal && col_scratch.len() < col_bytes)
    {
        return 0;
    }

    blur_1d_zero_alloc_checked(
        pixels,
        width,
        height,
        stride,
        radius,
        horizontal,
        row_scratch,
        col_scratch,
    )
}

fn blur_1d_zero_alloc_checked(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    radius: u32,
    horizontal: bool,
    row_scratch: &mut [u8],
    col_scratch: &mut [u8],
) -> u32 {
    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;
    let r = radius as usize;
    let window = 2 * r + 1;

    if horizontal {
        blur_1d_horizontal_zero_alloc(pixels, w, h, s, window, r, row_scratch)
    } else {
        // Vertical pass: transpose → blur → transpose back.
        for y in 0..h {
            for x in 0..w {
                let src = y * s + x * 4;
                let dst = (x * h + y) * 4;
                col_scratch[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
            }
        }
        let transposed_stride = h * 4;
        let count = blur_1d_horizontal_zero_alloc(
            col_scratch,
            h,
            w,
            transposed_stride,
            window,
            r,
            row_scratch,
        );
        for y in 0..h {
            for x in 0..w {
                let src = (x * h + y) * 4;
                let dst = y * s + x * 4;
                pixels[dst..dst + 4].copy_from_slice(&col_scratch[src..src + 4]);
            }
        }
        count
    }
}

fn blur_1d_horizontal_zero_alloc(
    pixels: &mut [u8],
    w: usize,
    h: usize,
    stride: usize,
    window: usize,
    radius: usize,
    row_scratch: &mut [u8],
) -> u32 {
    let mut count = 0u32;
    for y in 0..h {
        let row_start = y * stride;
        row_scratch[..w * 4].copy_from_slice(&pixels[row_start..row_start + w * 4]);

        let (mut r_sum, mut g_sum, mut b_sum, mut a_sum) = (0u64, 0u64, 0u64, 0u64);
        for i in 0..window.min(w) {
            let idx = i * 4;
            let a = row_scratch[idx + 3] as u64;
            r_sum += row_scratch[idx] as u64 * a;
            g_sum += row_scratch[idx + 1] as u64 * a;
            b_sum += row_scratch[idx + 2] as u64 * a;
            a_sum += a;
        }

        for x in 0..w {
            let dst = row_start + x * 4;
            let n = a_sum;
            if n > 0 {
                pixels[dst] = ((r_sum / n).min(255)) as u8;
                pixels[dst + 1] = ((g_sum / n).min(255)) as u8;
                pixels[dst + 2] = ((b_sum / n).min(255)) as u8;
            }
            pixels[dst + 3] = ((a_sum / window as u64).min(255)) as u8;
            count += 1;

            let left = x.saturating_sub(radius);
            if let Some(lidx) = left.checked_mul(4) {
                let la = row_scratch[lidx + 3] as u64;
                r_sum = r_sum.saturating_sub(row_scratch[lidx] as u64 * la);
                g_sum = g_sum.saturating_sub(row_scratch[lidx + 1] as u64 * la);
                b_sum = b_sum.saturating_sub(row_scratch[lidx + 2] as u64 * la);
                a_sum = a_sum.saturating_sub(la);
            }
            let right = x + radius + 1;
            if right < w {
                let ridx = right * 4;
                let ra = row_scratch[ridx + 3] as u64;
                r_sum += row_scratch[ridx] as u64 * ra;
                g_sum += row_scratch[ridx + 1] as u64 * ra;
                b_sum += row_scratch[ridx + 2] as u64 * ra;
                a_sum += ra;
            }
        }
    }
    count
}

/// Compute a 2D separable box blur in two passes (horizontal + vertical).
///
/// This is the recommended entry point for shadow blurs: it calls `blur_1d` twice
/// (once horizontal, once vertical), producing a 2D box blur in O(w·h·4) total
/// operations — roughly 4 reads + 4 writes per pixel regardless of radius.
///
/// Returns the total number of pixels blurred in both passes.
pub fn blur_separable(pixels: &mut [u8], width: u32, height: u32, stride: u32, radius: u32) -> u32 {
    let c1 = blur_1d(pixels, width, height, stride, radius, true);
    let c2 = blur_1d(pixels, width, height, stride, radius, false);
    c1 + c2
}

/// Zero-allocation variant of `blur_separable`.
///
/// Identical algorithm but takes pre-allocated scratch buffers.
/// Safe for OS bump-allocator.
///
/// - `row_scratch`: at least `max(width, height) * 4` bytes
/// - `col_scratch`: at least `width * height * 4` bytes
///
/// Returns the total number of pixels blurred in both passes.
pub fn blur_separable_zero_alloc(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    radius: u32,
    row_scratch: &mut [u8],
    col_scratch: &mut [u8],
) -> u32 {
    let c1 = blur_1d_zero_alloc(
        pixels,
        width,
        height,
        stride,
        radius,
        true,
        row_scratch,
        col_scratch,
    );
    let c2 = blur_1d_zero_alloc(
        pixels,
        width,
        height,
        stride,
        radius,
        false,
        row_scratch,
        col_scratch,
    );
    c1 + c2
}

/// Dual-Kawase blur: downscale → iterative blur → upscale.
///
/// An approximation of Gaussian blur that scales with O(log radius) instead of
/// O(radius²). Downscales by 2× in each axis, applies `iterations` passes of
/// 3×3 blur with increasing kernel stride (1, 2, 4, …), then upscales back to
/// the original resolution.
///
/// At 3 iterations, this produces a blur comparable to a ~16px radius box blur
/// with ~27 samples/pixel instead of ~225.
///
/// `pixels` is modified in-place. Temporary buffers are allocated for the
/// downscaled pyramid levels.
///
/// Returns the total number of pixels processed.
pub fn dual_kawase_blur(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    radius: u32,
    iterations: u32,
) -> u32 {
    if width == 0 || height == 0 || radius == 0 || iterations == 0 {
        return 0;
    }

    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;

    let _max_levels = iterations as usize + 1;
    let mut pyramid: [Option<PyramidLevel>; 6] = [None, None, None, None, None, None];

    let mut level0 = alloc::vec![0u8; h * s];
    level0[..h * s].copy_from_slice(&pixels[..h * s]);
    pyramid[0] = Some((level0, width, height, stride));

    let mut total = 0u32;

    // Downscale
    for level in 1..=iterations as usize {
        let (ref prev, pw, ph, _ps) = pyramid[level - 1].as_ref().unwrap();
        let pw = *pw as usize;
        let ph = *ph as usize;
        let nw = pw / 2;
        let nh = ph / 2;
        if nw < 4 || nh < 4 {
            break;
        }
        let nstride = (nw * 4) as u32;
        let mut down = alloc::vec![0u8; nh * nw * 4];
        downsample_2x(prev, pw as u32, ph as u32, &mut down, nw as u32, nh as u32);
        blur_3x3(&mut down, nw as u32, nh as u32, nstride);
        pyramid[level] = Some((down, nw as u32, nh as u32, nstride));
        total += (nw * nh) as u32;
    }

    // Increasing-stride blurs from smallest to largest
    for level in (1..=iterations as usize).rev() {
        if pyramid[level].is_none() {
            continue;
        }
        let (ref mut data, lw, lh, ls) = pyramid[level].as_mut().unwrap();
        let lw = *lw as usize;
        let lh = *lh as usize;
        stride_blur_3x3(data, lw as u32, lh as u32, *ls, 1 << (level - 1));
        total += (lw * lh) as u32;
    }

    // Upscale back to original
    for level in (0..iterations as usize).rev() {
        let (src_data, sw, sh) = {
            let (data, w, h, _) = match pyramid[level + 1].as_ref() {
                Some(v) => (v.0.as_slice(), v.1, v.2, v.3),
                None => continue,
            };
            (alloc::vec::Vec::from(data), w, h)
        };
        let (dst_data, dw, dh, ds) = match pyramid[level].as_mut() {
            Some(v) => (&mut v.0, v.1, v.2, v.3),
            None => continue,
        };
        upscale_2x(&src_data, sw, sh, dst_data, dw, dh, ds);
        total += dw * dh;
    }

    if let Some((ref result, _, _, _)) = pyramid[0] {
        for y in 0..h {
            let src_off = y * w * 4;
            let dst_off = y * s;
            let len = (w * 4).min(s);
            pixels[dst_off..dst_off + len].copy_from_slice(&result[src_off..src_off + len]);
        }
    }

    total
}

/// Box-filter 2× downscale: average every 2×2 block into 1 pixel.
fn downsample_2x(src: &[u8], sw: u32, _sh: u32, dst: &mut [u8], dw: u32, dh: u32) {
    for dy in 0..dh as usize {
        for dx in 0..dw as usize {
            let sx = dx * 2;
            let sy = dy * 2;
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for oy in 0..2usize {
                for ox in 0..2usize {
                    let idx = ((sy + oy) * sw as usize + (sx + ox)) * 4;
                    r += src[idx] as u32;
                    g += src[idx + 1] as u32;
                    b += src[idx + 2] as u32;
                    a += src[idx + 3] as u32;
                }
            }
            let di = (dy * dw as usize + dx) * 4;
            dst[di] = (r / 4) as u8;
            dst[di + 1] = (g / 4) as u8;
            dst[di + 2] = (b / 4) as u8;
            dst[di + 3] = (a / 4) as u8;
        }
    }
}

/// Bilinear 2× upscale (nearest-neighbor simple version).
fn upscale_2x(src: &[u8], sw: u32, sh: u32, dst: &mut [u8], dw: u32, dh: u32, stride: u32) {
    for dy in 0..dh as usize {
        let sy = (dy * sh as usize / dh as usize).min(sh as usize - 1);
        for dx in 0..dw as usize {
            let sx = (dx * sw as usize / dw as usize).min(sw as usize - 1);
            let si = (sy * sw as usize + sx) * 4;
            let di = dy * stride as usize + dx * 4;
            dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
}

/// 3×3 blur with configurable sampling stride.
fn stride_blur_3x3(pixels: &mut [u8], width: u32, height: u32, stride: u32, step: usize) {
    if width < 3 || height < 3 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;
    let mut copy = alloc::vec![0u8; h * s];
    copy[..h * s].copy_from_slice(&pixels[..h * s]);
    let step = step.max(1);
    for y in step..h - step {
        for x in step..w - step {
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut a = 0u32;
            for ky in 0..3u32 {
                for kx in 0..3u32 {
                    let sx = (x as isize + (kx as isize - 1) * step as isize)
                        .max(0)
                        .min(w as isize - 1) as usize;
                    let sy = (y as isize + (ky as isize - 1) * step as isize)
                        .max(0)
                        .min(h as isize - 1) as usize;
                    let idx = sy * s + sx * 4;
                    let ca = copy[idx + 3] as u32;
                    r += copy[idx] as u32 * ca;
                    g += copy[idx + 1] as u32 * ca;
                    b += copy[idx + 2] as u32 * ca;
                    a += ca;
                }
            }
            let di = y * s + x * 4;
            if a > 0 {
                pixels[di] = (r / a).min(255) as u8;
                pixels[di + 1] = (g / a).min(255) as u8;
                pixels[di + 2] = (b / a).min(255) as u8;
            }
            pixels[di + 3] = (a / 9).min(255) as u8;
        }
    }
}
