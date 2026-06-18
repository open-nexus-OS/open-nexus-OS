// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stride-check and horizontal blur utility for the windowd compositor
//! shadow/backdrop pipeline.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use crate::error::WindowdError;

pub(crate) fn checked_stride(width: u32) -> Result<u32, WindowdError> {
    let bytes = width.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
    bytes.checked_add(63).ok_or(WindowdError::ArithmeticOverflow).map(|v| v / 64 * 64)
}

/// Single-row horizontal box blur with variable radius.
/// Zero-allocation: uses `row_buf` (pre-allocated) for the temporary copy.
/// Sliding window: O(width) operations regardless of radius.
pub(crate) fn blur_row_horizontal(
    pixels: &mut [u8],
    row_bytes: usize,
    radius: u32,
    row_buf: &mut [u8],
) {
    if row_bytes == 0 || radius == 0 {
        return;
    }
    let w = row_bytes / 4;
    let r = radius as usize;
    let window = 2 * r + 1;

    row_buf[..row_bytes].copy_from_slice(&pixels[..row_bytes]);

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
        let idx = x * 4;
        if a_sum > 0 {
            pixels[idx] = ((r_sum / a_sum).min(255)) as u8;
            pixels[idx + 1] = ((g_sum / a_sum).min(255)) as u8;
            pixels[idx + 2] = ((b_sum / a_sum).min(255)) as u8;
        }
        pixels[idx + 3] = ((a_sum / window as u64).min(255)) as u8;

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
