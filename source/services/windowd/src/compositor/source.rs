// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Source-frame scaling for windowd compositor: LUT-based BGRA row copy.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (scale_lut)

use super::types::{RenderClip, SourceFrame};
use crate::error::WindowdError;
use crate::smoke::VisibleBootstrapMode;
use alloc::vec::Vec;
use systemui;

/// The widest supported source row (px) for the stack-side RLE decode.
pub(crate) const MAX_SOURCE_W: usize = 1280;

/// Decode ONE source row into `out` (RLE) or borrow it (raw). Returns the
/// row slice of `width * 4` bytes. Bounded, heap-free.
pub(crate) fn source_row<'a>(
    frame: &'a SourceFrame,
    sy: usize,
    out: &'a mut [u8],
) -> Result<&'a [u8], WindowdError> {
    let row_bytes = frame.width as usize * 4;
    match frame.rows {
        None => {
            let start = sy
                .checked_mul(frame.stride as usize)
                .ok_or(WindowdError::ArithmeticOverflow)?;
            frame
                .pixels
                .get(start..start + row_bytes)
                .ok_or(WindowdError::BufferLengthMismatch)
        }
        Some(rows) => {
            // Per-row QOI (systemui = the codec SSOT; encoder in its build).
            systemui::decode_qoi_row(frame.pixels, rows, frame.width as usize, sy, out)
                .ok_or(WindowdError::BufferLengthMismatch)?;
            Ok(&out[..row_bytes])
        }
    }
}

pub(crate) fn copy_scaled_systemui_row_clipped(
    frame: &SourceFrame,
    sx_lut: &[u32],
    sy_lut: &[u32],
    mode: VisibleBootstrapMode,
    y: u32,
    row: &mut [u8],
    rc: RenderClip,
) -> Result<(), WindowdError> {
    let rl = mode.stride as usize;
    if row.len() < rl || frame.width == 0 || frame.height == 0 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let sy = *sy_lut.get(y as usize).ok_or(WindowdError::BufferLengthMismatch)? as usize;
    let mut decode_buf = [0u8; MAX_SOURCE_W * 4];
    let src_row = source_row(frame, sy, &mut decode_buf)?;
    let mut x = rc.start_x.min(mode.width) as usize;
    let ex = rc.end_x.min(mode.width) as usize;
    while x < ex {
        let sx = *sx_lut.get(x).ok_or(WindowdError::BufferLengthMismatch)? as usize;
        let mut run = 1usize;
        while x + run < ex {
            let n = *sx_lut.get(x + run).ok_or(WindowdError::BufferLengthMismatch)? as usize;
            if n != sx.saturating_add(run) {
                break;
            }
            run += 1;
        }
        let src = sx.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        let dst = x.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
        if run >= 4 {
            let bl = run.checked_mul(4).ok_or(WindowdError::ArithmeticOverflow)?;
            row[dst..dst + bl].copy_from_slice(
                src_row.get(src..src + bl).ok_or(WindowdError::BufferLengthMismatch)?,
            );
            x += run;
            continue;
        }
        row[dst..dst + 4].copy_from_slice(
            src_row.get(src..src + 4).ok_or(WindowdError::BufferLengthMismatch)?,
        );
        x += 1;
    }
    Ok(())
}

pub(crate) fn build_scale_lut(dl: u32, sl: u32) -> Result<Vec<u32>, WindowdError> {
    if dl == 0 || sl == 0 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let mut lut = Vec::with_capacity(dl as usize);
    for d in 0..dl {
        lut.push(((u64::from(d) * u64::from(sl)) / u64::from(dl)).min(sl.saturating_sub(1) as u64)
            as u32);
    }
    Ok(lut)
}
