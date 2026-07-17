// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Filled-shape primitives: solid rectangles, anti-aliased rounded rectangles,
//! vertical-gradient rounded rectangles, and soft drop shadows.
//!
//! Coverage comes from the one SDF math SSOT (`nexus_sdf::fixed`, fixed-point AA)
//! so a rounded edge here matches the live compositor and the GPU shaders
//! (RFC-0067 P5). Colours are straight-alpha BGRA `[u8; 4]`.

#![forbid(unsafe_code)]

use super::blend;
use super::surface::Surface;

/// Opaque solid-colour rectangle fill (overwrites, no blend).
pub fn fill_rect_solid(s: &mut Surface, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    let stride = s.stride();
    let buf = s.buf_mut();
    for py in y..end_y {
        let row = py as usize * stride;
        for px in x..end_x {
            let idx = row + px as usize * 4;
            if idx + 4 <= buf.len() {
                buf[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Hard-edged rounded-rectangle fill (binary inside/outside corner test, no
/// anti-aliasing), blended over the destination. The historical CPU/VMO path;
/// prefer [`fill_rounded_aa`] for new work.
pub fn fill_rounded_solid(
    s: &mut Surface,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    color: [u8; 4],
) {
    if color[3] == 0 {
        return;
    }
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    let r = radius.min(w / 2).min(h / 2) as i32;
    let cx = x as i32 + r;
    let cy = y as i32 + r;
    let cx2 = x as i32 + w as i32 - r - 1;
    let cy2 = y as i32 + h as i32 - r - 1;
    let stride = s.stride();
    let buf = s.buf_mut();
    for py in y..end_y {
        let row = py as usize * stride;
        for px in x..end_x {
            let idx = row + px as usize * 4;
            if idx + 4 > buf.len() {
                continue;
            }
            let inside = if r <= 0 {
                true
            } else {
                let px_i = px as i32;
                let py_i = py as i32;
                let d = if px_i <= cx && py_i <= cy {
                    corner_dist(px_i, py_i, cx, cy, r)
                } else if px_i >= cx2 && py_i <= cy {
                    corner_dist(px_i, py_i, cx2, cy, r)
                } else if px_i <= cx && py_i >= cy2 {
                    corner_dist(px_i, py_i, cx, cy2, r)
                } else if px_i >= cx2 && py_i >= cy2 {
                    corner_dist(px_i, py_i, cx2, cy2, r)
                } else {
                    0
                };
                d <= 0
            };
            if inside {
                blend::blend_over(buf, idx, &color);
            }
        }
    }
}

/// Squared distance from `(px, py)` to corner centre `(cx, cy)` minus `r²`
/// (≤ 0 inside the corner circle).
#[inline]
fn corner_dist(px: i32, py: i32, cx: i32, cy: i32, r: i32) -> i32 {
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy - r * r
}

/// Anti-aliased rounded-rectangle fill, blended over the destination.
pub fn fill_rounded_aa(
    s: &mut Surface,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    color: [u8; 4],
) {
    if color[3] == 0 || w == 0 || h == 0 {
        return;
    }
    use nexus_sdf::fixed;
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    let r = radius.min(w / 2).min(h / 2);
    let min_x = fixed::px_u32(x);
    let min_y = fixed::px_u32(y);
    let max_x = fixed::px_u32(x.saturating_add(w));
    let max_y = fixed::px_u32(y.saturating_add(h));
    let rad = fixed::px_u32(r);
    let stride = s.stride();
    let buf = s.buf_mut();
    for py in y..end_y {
        let pcy = fixed::pixel_center(py);
        let row = py as usize * stride;
        for px in x..end_x {
            let idx = row + px as usize * 4;
            if idx + 4 > buf.len() {
                continue;
            }
            let sd = fixed::rounded_rect_sd(
                fixed::pixel_center(px),
                pcy,
                min_x,
                min_y,
                max_x,
                max_y,
                rad,
            );
            let cov = fixed::fill_alpha(sd); // 0..255 anti-aliased coverage
            if cov == 0 {
                continue;
            }
            let a = (color[3] as u32 * cov / 255) as u8;
            if a == 0 {
                continue;
            }
            blend::blend_over(buf, idx, &[color[0], color[1], color[2], a]);
        }
    }
}

/// Anti-aliased rounded rectangle filled with a vertical (top→bottom) linear
/// gradient. Per-row colour lerp, the same AA coverage as [`fill_rounded_aa`].
#[allow(clippy::too_many_arguments)]
pub fn fill_gradient_aa(
    s: &mut Surface,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    top: [u8; 4],
    bottom: [u8; 4],
) {
    if w == 0 || h == 0 {
        return;
    }
    use nexus_sdf::fixed;
    let width = s.width();
    let height = s.height();
    let end_x = x.saturating_add(w).min(width);
    let end_y = y.saturating_add(h).min(height);
    let min_x = fixed::px_u32(x);
    let min_y = fixed::px_u32(y);
    let max_x = fixed::px_u32(x.saturating_add(w));
    let max_y = fixed::px_u32(y.saturating_add(h));
    let rad = fixed::px_u32(radius.min(w / 2).min(h / 2));
    let stride = s.stride();
    let buf = s.buf_mut();
    for py in y..end_y {
        // Per-row gradient interpolation (fixed point, / (h-1)).
        let t_num = (py - y) as u32;
        let denom = (h - 1).max(1);
        let mut rgba = [0u8; 4];
        for c in 0..4 {
            let tv = top[c] as u32;
            let bv = bottom[c] as u32;
            rgba[c] = ((tv * (denom - t_num) + bv * t_num) / denom) as u8;
        }
        if rgba[3] == 0 {
            continue;
        }
        let pcy = fixed::pixel_center(py);
        let row = py as usize * stride;
        for px in x..end_x {
            let idx = row + px as usize * 4;
            if idx + 4 > buf.len() {
                continue;
            }
            let sd = fixed::rounded_rect_sd(
                fixed::pixel_center(px),
                pcy,
                min_x,
                min_y,
                max_x,
                max_y,
                rad,
            );
            let cov = fixed::fill_alpha(sd); // 0..255 anti-aliased coverage
            if cov == 0 {
                continue;
            }
            let a = (rgba[3] as u32 * cov / 255) as u8;
            if a == 0 {
                continue;
            }
            blend::blend_over(buf, idx, &[rgba[0], rgba[1], rgba[2], a]);
        }
    }
}

/// Soft drop shadow for a rounded rect: alpha falls off quadratically over
/// `blur` pixels around the shape rect (shifted by `offset`). The painted band
/// is clamped vertically to `[clip_y0, clip_y1)` — callers pass the plane they
/// are allowed to write into (e.g. the display rows, or the whole surface).
///
/// Distance uses the octagonal-norm approximation of `length(qx, qy)`
/// (`max + ½·min`, error < 12 %, invisible in a penumbra), avoiding per-pixel
/// `sqrt`.
#[allow(clippy::too_many_arguments)]
pub fn drop_shadow(
    s: &mut Surface,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    blur: u32,
    offset_x: i32,
    offset_y: i32,
    color: [u8; 4],
    clip_y0: u32,
    clip_y1: u32,
) {
    if w == 0 || h == 0 || blur == 0 || color[3] == 0 {
        return;
    }
    let width = s.width() as i32;
    let blur_i = blur as i32;
    let rx0 = x as i32 + offset_x;
    let ry0 = y as i32 + offset_y;
    let rx1 = rx0 + w as i32;
    let ry1 = ry0 + h as i32;
    let r = radius.min(w / 2).min(h / 2) as i32;
    let py0 = (ry0 - blur_i).max(clip_y0 as i32);
    let py1 = (ry1 + blur_i).min(clip_y1 as i32);
    let px0 = (rx0 - blur_i).max(0);
    let px1 = (rx1 + blur_i).min(width);
    let stride = s.stride();
    let buf = s.buf_mut();
    for py in py0..py1 {
        let row = py as usize * stride;
        for px in px0..px1 {
            let qx = (rx0 + r - px).max(px - (rx1 - 1 - r)).max(0);
            let qy = (ry0 + r - py).max(py - (ry1 - 1 - r)).max(0);
            let dist = qx.max(qy) + qx.min(qy) / 2 - r;
            let fall = blur_i - dist.max(0);
            if fall <= 0 {
                continue;
            }
            let a = (color[3] as i32 * fall * fall) / (blur_i * blur_i);
            if a <= 0 {
                continue;
            }
            let idx = row + px as usize * 4;
            if idx + 4 > buf.len() {
                continue;
            }
            blend::blend_over(buf, idx, &[color[0], color[1], color[2], a.min(255) as u8]);
        }
    }
}
