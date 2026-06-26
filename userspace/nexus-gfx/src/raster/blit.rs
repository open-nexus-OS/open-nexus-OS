// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Rectangle copy primitives.
//!
//! - [`blit_within`] / [`blit_within_blend`]: copy a rect from one region of a
//!   surface to another region of the *same* surface (the retained-plane →
//!   display-plane composite, and same-plane scrolls). Source rows are staged in
//!   a caller-supplied scratch row first, so an overlapping src/dst is safe and
//!   no heap is touched.
//! - [`blit_from`]: opaque copy from a *separate* source buffer (a wallpaper or
//!   icon atlas) into the surface.

#![forbid(unsafe_code)]

use super::blend;
use super::surface::Surface;
use super::RasterError;

/// Opaque copy of `(w, h)` from `(src_x, src_y)` to `(dst_x, dst_y)` within the
/// same surface. `scratch_row` needs `≥ w·4` bytes.
#[allow(clippy::too_many_arguments)]
pub fn blit_within(
    s: &mut Surface,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
    scratch_row: &mut [u8],
) -> Result<(), RasterError> {
    let width = s.width();
    let height = s.height();
    let copy_w = w.min(width.saturating_sub(dst_x)).min(width.saturating_sub(src_x));
    let copy_h = h.min(height.saturating_sub(dst_y)).min(height.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }
    let row_bytes = copy_w as usize * 4;
    if scratch_row.len() < row_bytes {
        return Err(RasterError::ScratchTooSmall);
    }
    let stride = s.stride();
    let buf = s.buf_mut();
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = sy as usize * stride + src_x as usize * 4;
        let dst_off = dy as usize * stride + dst_x as usize * 4;
        if src_off + row_bytes > buf.len() || dst_off + row_bytes > buf.len() {
            continue;
        }
        scratch_row[..row_bytes].copy_from_slice(&buf[src_off..src_off + row_bytes]);
        buf[dst_off..dst_off + row_bytes].copy_from_slice(&scratch_row[..row_bytes]);
    }
    Ok(())
}

/// Like [`blit_within`], but alpha-blends the source rows over the destination
/// (the translucent-glass composite over a blurred backdrop).
#[allow(clippy::too_many_arguments)]
pub fn blit_within_blend(
    s: &mut Surface,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
    scratch_row: &mut [u8],
) -> Result<(), RasterError> {
    let width = s.width();
    let height = s.height();
    let copy_w = w.min(width.saturating_sub(dst_x)).min(width.saturating_sub(src_x));
    let copy_h = h.min(height.saturating_sub(dst_y)).min(height.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }
    let row_bytes = copy_w as usize * 4;
    if scratch_row.len() < row_bytes {
        return Err(RasterError::ScratchTooSmall);
    }
    let stride = s.stride();
    let buf = s.buf_mut();
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = sy as usize * stride + src_x as usize * 4;
        let dst_off = dy as usize * stride + dst_x as usize * 4;
        if src_off + row_bytes > buf.len() || dst_off + row_bytes > buf.len() {
            continue;
        }
        scratch_row[..row_bytes].copy_from_slice(&buf[src_off..src_off + row_bytes]);
        for col in 0..copy_w as usize {
            let s4 = [
                scratch_row[col * 4],
                scratch_row[col * 4 + 1],
                scratch_row[col * 4 + 2],
                scratch_row[col * 4 + 3],
            ];
            blend::blend_over(buf, dst_off + col * 4, &s4);
        }
    }
    Ok(())
}

/// Opaque copy of `(w, h)` from a separate `src` buffer (`src_w × src_h`,
/// BGRA8888) into the surface at `(dst_x, dst_y)`.
#[allow(clippy::too_many_arguments)]
pub fn blit_from(
    s: &mut Surface,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) {
    if src.is_empty() || src_w == 0 {
        return;
    }
    let width = s.width();
    let height = s.height();
    let src_stride = src_w as usize * 4;
    let dst_stride = s.stride();
    let buf = s.buf_mut();
    for row in 0..h.min(height.saturating_sub(dst_y)) {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        if sy >= src_h || dy >= height {
            break;
        }
        let src_off = sy as usize * src_stride + src_x as usize * 4;
        let dst_off = dy as usize * dst_stride + dst_x as usize * 4;
        let copy_len = (w as usize * 4).min(buf.len().saturating_sub(dst_off));
        let src_end = src_off.saturating_add(copy_len);
        if src_end <= src.len() && dst_off + copy_len <= buf.len() {
            buf[dst_off..dst_off + copy_len].copy_from_slice(&src[src_off..src_end]);
        }
    }
    let _ = width;
}
