// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CPU/VMO software rasterizer primitives.
//!
//! These operate directly on the framebuffer VMO backing memory via raw
//! pointers. They are the OS equivalent of `CpuMockBackend`'s rendering
//! methods, with the same deterministic semantics: solid/rounded fills, box
//! and separable-gaussian backdrop blur, opaque and alpha-blended blits, and
//! the fixed-point pixel blend helpers shared across the compositing path.

#![cfg(all(feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::command::buffer::RgbaColor;

pub(crate) fn fill_rect_solid_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    for py in y..end_y {
        let row_base = py as usize * fb_w;
        for px in x..end_x {
            let idx = (row_base + px as usize) * 4;
            if idx + 4 <= fb_len {
                unsafe {
                    core::ptr::write_volatile(fb.add(idx), color[0]);
                    core::ptr::write_volatile(fb.add(idx + 1), color[1]);
                    core::ptr::write_volatile(fb.add(idx + 2), color[2]);
                    core::ptr::write_volatile(fb.add(idx + 3), color[3]);
                }
            }
        }
    }
}

pub(crate) fn fill_sdf_rounded_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    color: RgbaColor,
) {
    let rgba = color.as_array();
    if rgba[3] == 0 {
        return;
    }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius.min(w / 2).min(h / 2) as i32;
    let cx = x as i32 + r;
    let cy = y as i32 + r;
    let cx2 = x as i32 + w as i32 - r - 1;
    let cy2 = y as i32 + h as i32 - r - 1;
    for py in y..end_y {
        let row_base = py as usize * fb_w;
        for px in x..end_x {
            let idx = (row_base + px as usize) * 4;
            if idx + 4 > fb_len {
                continue;
            }
            let inside = if r <= 0 {
                true
            } else {
                let px_i = px as i32;
                let py_i = py as i32;
                let d = if px_i <= cx && py_i <= cy {
                    corner_dist_i32(px_i, py_i, cx, cy, r)
                } else if px_i >= cx2 && py_i <= cy {
                    corner_dist_i32(px_i, py_i, cx2, cy, r)
                } else if px_i <= cx && py_i >= cy2 {
                    corner_dist_i32(px_i, py_i, cx, cy2, r)
                } else if px_i >= cx2 && py_i >= cy2 {
                    corner_dist_i32(px_i, py_i, cx2, cy2, r)
                } else {
                    0
                };
                d <= 0
            };
            if inside {
                blend_pixel_vmo(fb, idx, &rgba);
            }
        }
    }
}

pub(crate) fn blur_backdrop_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    _saturation_pct: u32,
) -> Result<(), GfxError> {
    if radius == 0 {
        return Ok(());
    }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius as usize;
    let pixels = (end_x - x) as usize;
    if pixels == 0 {
        return Ok(());
    }
    // Horizontal pass: box-blur each row in-place with a scratch buffer.
    // Allocate on stack — worst case 1280*4 = 5120 bytes for a full-width row.
    let mut scratch: [u8; 5120] = [0u8; 5120];
    let row_bytes = pixels * 4;
    if row_bytes > scratch.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for py in y..end_y {
        let row_start = (py as usize * fb_w + x as usize) * 4;
        if row_start + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(row_start), scratch.as_mut_ptr(), row_bytes);
        }
        let mut sums: [u64; 4] = [0; 4];
        let mut left: usize = 0;
        let mut right = r.min(pixels.saturating_sub(1));
        for j in left..=right {
            let bi = j * 4;
            for c in 0..4 {
                sums[c] += scratch[bi + c] as u64;
            }
        }
        for i in 0..pixels {
            let count = (right - left + 1) as u64;
            let di = row_start + i * 4;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(
                        fb.add(di + c),
                        (sums[c] / count.max(1)).min(255) as u8,
                    );
                }
            }
            if i + 1 < pixels {
                let next_left = (i + 1).saturating_sub(r);
                if next_left > left {
                    let bi = left * 4;
                    for c in 0..4 {
                        sums[c] = sums[c].saturating_sub(scratch[bi + c] as u64);
                    }
                    left = next_left;
                }
                let next_right = (i + 1 + r).min(pixels.saturating_sub(1));
                if next_right > right {
                    right = next_right;
                    let bi = right * 4;
                    for c in 0..4 {
                        sums[c] += scratch[bi + c] as u64;
                    }
                }
            }
        }
    }
    // Vertical pass
    let col_h = (end_y - y) as usize;
    let mut col_buf: [u8; 3200] = [0u8; 3200]; // 800 rows * 4 bytes
    if col_h * 4 > col_buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..col_h {
            let src = (y as usize + row_i) * fb_w + col_off;
            if src + 4 <= fb_len {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        fb.add(src),
                        col_buf.as_mut_ptr().add(row_i * 4),
                        4,
                    );
                }
            }
        }
        let mut sums: [u64; 4] = [0; 4];
        let mut top: usize = 0;
        let mut bot = r.min(col_h.saturating_sub(1));
        for j in top..=bot {
            let bi = j * 4;
            for c in 0..4 {
                sums[c] += col_buf[bi + c] as u64;
            }
        }
        for i in 0..col_h {
            let count = (bot - top + 1) as u64;
            let dst = (y as usize + i) * fb_w + col_off;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(
                        fb.add(dst + c),
                        (sums[c] / count.max(1)).min(255) as u8,
                    );
                }
            }
            if i + 1 < col_h {
                let ntop = (i + 1).saturating_sub(r);
                if ntop > top {
                    let bi = top * 4;
                    for c in 0..4 {
                        sums[c] = sums[c].saturating_sub(col_buf[bi + c] as u64);
                    }
                    top = ntop;
                }
                let nbot = (i + 1 + r).min(col_h.saturating_sub(1));
                if nbot > bot {
                    bot = nbot;
                    let bi = bot * 4;
                    for c in 0..4 {
                        sums[c] += col_buf[bi + c] as u64;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Separable gaussian blur — the virgl GPU path target.
///
/// Uses a precomputed gaussian kernel for higher-quality blur than the box-blur
/// fallback. The two-pass separable convolution (horizontal + vertical) is the
/// same algorithm a GPU compute shader would execute; this CPU implementation
/// serves as both the reference and the fallback when virgl is unavailable.
pub(crate) fn blur_backdrop_separable_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    _saturation_pct: u32,
) -> Result<(), GfxError> {
    if radius == 0 {
        return Ok(());
    }
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    let r = radius as usize;
    let pixels = (end_x - x) as usize;
    let rows = (end_y - y) as usize;
    if pixels == 0 || rows == 0 {
        return Ok(());
    }

    // Precompute gaussian kernel weights for the given radius.
    // σ = radius / 2 gives a natural falloff.
    let sigma = (r as f32) / 2.0_f32.max(0.5);
    let kernel_size = r * 2 + 1;
    let kernel: [f32; 41] = {
        let mut k = [0.0_f32; 41];
        let mut sum = 0.0_f32;
        for i in 0..kernel_size.min(41) {
            let dx = (i as i32 - r as i32) as f32;
            let w = libm::expf(-dx * dx / (2.0 * sigma * sigma));
            k[i] = w;
            sum += w;
        }
        // Normalize
        if sum > 0.0 {
            for v in k.iter_mut().take(kernel_size.min(41)) {
                *v /= sum;
            }
        }
        k
    };
    let k_len = kernel_size.min(41);

    // Horizontal pass: convolve each row with the gaussian kernel.
    // Stack-allocated scratch: worst case 1280*4 = 5120 bytes.
    let row_bytes = pixels * 4;
    let mut scratch: [u8; 5120] = [0u8; 5120];
    if row_bytes > scratch.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for py in y..end_y {
        let row_start = (py as usize * fb_w + x as usize) * 4;
        if row_start + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(row_start), scratch.as_mut_ptr(), row_bytes);
        }
        for i in 0..pixels {
            let mut acc: [f32; 4] = [0.0; 4];
            for ki in 0..k_len {
                let src_i = (i as i32 + ki as i32 - r as i32).clamp(0, pixels as i32 - 1) as usize;
                let si = src_i * 4;
                let w = kernel[ki];
                for c in 0..4 {
                    acc[c] += scratch[si + c] as f32 * w;
                }
            }
            let di = row_start + i * 4;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(
                        fb.add(di + c),
                        libm::roundf(acc[c]).clamp(0.0, 255.0) as u8,
                    );
                }
            }
        }
    }

    // Vertical pass: convolve each column with the gaussian kernel.
    let col_bytes = rows * 4;
    let mut col_buf: [u8; 3200] = [0u8; 3200]; // 800 rows * 4 bytes
    if col_bytes > col_buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for px in x..end_x {
        let col_off = px as usize * 4;
        for row_i in 0..rows {
            let src = (y as usize + row_i) * fb_w + col_off;
            if src + 4 <= fb_len {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        fb.add(src),
                        col_buf.as_mut_ptr().add(row_i * 4),
                        4,
                    );
                }
            }
        }
        for i in 0..rows {
            let mut acc: [f32; 4] = [0.0; 4];
            for ki in 0..k_len {
                let src_i = (i as i32 + ki as i32 - r as i32).clamp(0, rows as i32 - 1) as usize;
                let si = src_i * 4;
                let w = kernel[ki];
                for c in 0..4 {
                    acc[c] += col_buf[si + c] as f32 * w;
                }
            }
            let di = (y as usize + i) * fb_w + col_off;
            for c in 0..4 {
                unsafe {
                    core::ptr::write_volatile(
                        fb.add(di + c),
                        libm::roundf(acc[c]).clamp(0.0, 255.0) as u8,
                    );
                }
            }
        }
    }

    Ok(())
}

pub(crate) fn blit_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), GfxError> {
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(dst_x)).min(fb_w_u.saturating_sub(src_x));
    let copy_h = h.min(fb_h.saturating_sub(dst_y)).min(fb_h.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }
    // Use stack scratch for row copy to handle overlapping regions safely.
    let row_bytes = copy_w as usize * 4;
    let mut buf: [u8; 5120] = [0u8; 5120];
    if row_bytes > buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = (sy as usize * fb_w + src_x as usize) * 4;
        let dst_off = (dy as usize * fb_w + dst_x as usize) * 4;
        if src_off + row_bytes > fb_len || dst_off + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(src_off), buf.as_mut_ptr(), row_bytes);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), fb.add(dst_off), row_bytes);
        }
    }
    Ok(())
}

/// Like `blit_vmo`, but ALPHA-BLENDS the source over the destination (instead
/// of an opaque copy). Used by the CPU glass path: composite a translucent
/// layer (e.g. the chat panel with a low-alpha background) over a blurred
/// backdrop so the blur shows through. Source rows are read into scratch first
/// so a src/dst overlap is safe.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blit_blend_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    w: u32,
    h: u32,
) -> Result<(), GfxError> {
    let fb_w_u = fb_w as u32;
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let copy_w = w.min(fb_w_u.saturating_sub(dst_x)).min(fb_w_u.saturating_sub(src_x));
    let copy_h = h.min(fb_h.saturating_sub(dst_y)).min(fb_h.saturating_sub(src_y));
    if copy_w == 0 || copy_h == 0 {
        return Ok(());
    }
    let row_bytes = copy_w as usize * 4;
    let mut buf: [u8; 5120] = [0u8; 5120];
    if row_bytes > buf.len() {
        return Err(GfxError::ResourceExhausted);
    }
    for row in 0..copy_h {
        let sy = src_y.saturating_add(row);
        let dy = dst_y.saturating_add(row);
        let src_off = (sy as usize * fb_w + src_x as usize) * 4;
        let dst_off = (dy as usize * fb_w + dst_x as usize) * 4;
        if src_off + row_bytes > fb_len || dst_off + row_bytes > fb_len {
            continue;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(fb.add(src_off), buf.as_mut_ptr(), row_bytes);
        }
        for col in 0..copy_w as usize {
            let s = [buf[col * 4], buf[col * 4 + 1], buf[col * 4 + 2], buf[col * 4 + 3]];
            blend_pixel_vmo(fb, dst_off + col * 4, &s);
        }
    }
    Ok(())
}

/// Composite a premultiplied-alpha BGRA pixel over the destination:
/// out_channel = src_channel + dst_channel * (255 - alpha) / 255.
pub(crate) fn blend_premultiplied_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    let inv = 255u32 - src[3] as u32;
    unsafe {
        let b = core::ptr::read_volatile(fb.add(idx)) as u32;
        let g = core::ptr::read_volatile(fb.add(idx + 1)) as u32;
        let r = core::ptr::read_volatile(fb.add(idx + 2)) as u32;
        // (x*257)>>16 ≈ x/255 with rounding (+32768), matching blend_pixel_vmo.
        let out_b = src[0] as u32 + ((inv * b * 257 + 32768) >> 16);
        let out_g = src[1] as u32 + ((inv * g * 257 + 32768) >> 16);
        let out_r = src[2] as u32 + ((inv * r * 257 + 32768) >> 16);
        core::ptr::write_volatile(fb.add(idx), out_b.min(255) as u8);
        core::ptr::write_volatile(fb.add(idx + 1), out_g.min(255) as u8);
        core::ptr::write_volatile(fb.add(idx + 2), out_r.min(255) as u8);
        core::ptr::write_volatile(fb.add(idx + 3), 255);
    }
}

pub(crate) fn blend_pixel_vmo(fb: *mut u8, idx: usize, src: &[u8; 4]) {
    let alpha = src[3] as u32;
    if alpha == 0 {
        return;
    }
    if alpha >= 255 {
        unsafe {
            core::ptr::write_volatile(fb.add(idx), src[0]);
            core::ptr::write_volatile(fb.add(idx + 1), src[1]);
            core::ptr::write_volatile(fb.add(idx + 2), src[2]);
            core::ptr::write_volatile(fb.add(idx + 3), src[3]);
        }
    } else {
        let inv = 255 - alpha;
        unsafe {
            let b = core::ptr::read_volatile(fb.add(idx)) as u32;
            let g = core::ptr::read_volatile(fb.add(idx + 1)) as u32;
            let r = core::ptr::read_volatile(fb.add(idx + 2)) as u32;
            // Phase 6e: fixed-point blend — (x*257)>>16 replaces /255.
            // 257/65536 ≈ 1/255 with <0.002% error. Multiplies by 257 with
            // rounding (+32768 before shift) for 8-bit color accuracy.
            let blend_b = ((alpha * src[0] as u32 + inv * b) * 257 + 32768) >> 16;
            let blend_g = ((alpha * src[1] as u32 + inv * g) * 257 + 32768) >> 16;
            let blend_r = ((alpha * src[2] as u32 + inv * r) * 257 + 32768) >> 16;
            core::ptr::write_volatile(fb.add(idx), blend_b as u8);
            core::ptr::write_volatile(fb.add(idx + 1), blend_g as u8);
            core::ptr::write_volatile(fb.add(idx + 2), blend_r as u8);
            let dst_alpha = core::ptr::read_volatile(fb.add(idx + 3)) as u32;
            core::ptr::write_volatile(
                fb.add(idx + 3),
                src[3].saturating_add((((inv * dst_alpha) * 257 + 32768) >> 16) as u8),
            );
        }
    }
}

pub(crate) fn corner_dist_i32(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy - r * r
}
