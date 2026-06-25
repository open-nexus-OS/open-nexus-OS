// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CPU fallback for the GPU vector pipeline (FillSdfGradient /
//! DropShadow) — used on non-virgl backends (mmio 2D path) and when the
//! virgl submit fails. Matches the GPU shaders' geometry semantics; edge
//! quality is the hard-edged SDF of the legacy CPU path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md

#![cfg(all(feature = "os-lite", target_os = "none"))]

use nexus_gfx::command::buffer::RgbaColor;

use crate::backend::blend_pixel_vmo;

/// Fill an SDF rounded rect with a vertical linear gradient (top → bottom).
/// `y` is the absolute fb row (display offset already applied by the caller).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_sdf_gradient_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    top: RgbaColor,
    bottom: RgbaColor,
) {
    if w == 0 || h == 0 {
        return;
    }
    let tc = top.as_array();
    let bc = bottom.as_array();
    let fb_w_u = fb_w as u32;
    let end_x = x.saturating_add(w).min(fb_w_u);
    let fb_h = (fb_len / (fb_w * 4)) as u32;
    let end_y = y.saturating_add(h).min(fb_h);
    // Anti-aliased coverage via the one SDF math SSOT (`nexus-sdf` fixed-point) —
    // the SAME rounded-rect coverage the GPU shader + the compositor use
    // (RFC-0067 P5), replacing the old hard-edged binary corner test.
    use nexus_sdf::fixed;
    let min_x = fixed::px_u32(x);
    let min_y = fixed::px_u32(y);
    let max_x = fixed::px_u32(x.saturating_add(w));
    let max_y = fixed::px_u32(y.saturating_add(h));
    let rad = fixed::px_u32(radius.min(w / 2).min(h / 2));
    for py in y..end_y {
        // Per-row gradient interpolation (fixed point, /h).
        let t_num = (py - y) as u32;
        let denom = (h - 1).max(1);
        let mut rgba = [0u8; 4];
        for c in 0..4 {
            let tv = tc[c] as u32;
            let bv = bc[c] as u32;
            rgba[c] = ((tv * (denom - t_num) + bv * t_num) / denom) as u8;
        }
        if rgba[3] == 0 {
            continue;
        }
        let pcy = fixed::pixel_center(py);
        let row_base = py as usize * fb_w;
        for px in x..end_x {
            let idx = (row_base + px as usize) * 4;
            if idx + 4 > fb_len {
                continue;
            }
            let sd =
                fixed::rounded_rect_sd(fixed::pixel_center(px), pcy, min_x, min_y, max_x, max_y, rad);
            let cov = fixed::fill_alpha(sd); // 0..255 anti-aliased coverage
            if cov == 0 {
                continue;
            }
            let a = (rgba[3] as u32 * cov / 255) as u8;
            if a == 0 {
                continue;
            }
            blend_pixel_vmo(fb, idx, &[rgba[0], rgba[1], rgba[2], a]);
        }
    }
}

/// Soft drop shadow for a rounded rect: alpha falls off quadratically with
/// the (integer-approximated) SDF distance over `blur` pixels. `(x, y, w, h)`
/// is the casting shape with `y` absolute; the painted region is the shape
/// shifted by the offset and padded by `blur`, clamped to the display plane.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drop_shadow_vmo(
    fb: *mut u8,
    fb_len: usize,
    fb_w: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    blur: u32,
    offset_x: i32,
    offset_y: i32,
    color: RgbaColor,
    display_row: u32,
    display_h: u32,
) {
    if w == 0 || h == 0 || blur == 0 {
        return;
    }
    let rgba = color.as_array();
    if rgba[3] == 0 {
        return;
    }
    let fb_w_i = fb_w as i32;
    let blur_i = blur as i32;
    // Shape rect, shifted by the shadow offset (absolute rows).
    let rx0 = x as i32 + offset_x;
    let ry0 = y as i32 + offset_y;
    let rx1 = rx0 + w as i32;
    let ry1 = ry0 + h as i32;
    let r = radius.min(w / 2).min(h / 2) as i32;
    // Painted region, clamped to the display plane.
    let py0 = (ry0 - blur_i).max(display_row as i32);
    let py1 = (ry1 + blur_i).min((display_row + display_h) as i32);
    let px0 = (rx0 - blur_i).max(0);
    let px1 = (rx1 + blur_i).min(fb_w_i);
    for py in py0..py1 {
        let row_base = py as usize * fb_w;
        for px in px0..px1 {
            // Distance from the rounded rect (0 inside, grows outside).
            let qx = (rx0 + r - px).max(px - (rx1 - 1 - r)).max(0);
            let qy = (ry0 + r - py).max(py - (ry1 - 1 - r)).max(0);
            // Octagonal norm approximation of length(qx,qy): max + ½min —
            // avoids per-pixel sqrt, error <12% (invisible in a penumbra).
            let dist = qx.max(qy) + qx.min(qy) / 2 - r;
            let fall = blur_i - dist.max(0);
            if fall <= 0 {
                continue;
            }
            // Quadratic falloff matching the GPU shader.
            let a = (rgba[3] as i32 * fall * fall) / (blur_i * blur_i);
            if a <= 0 {
                continue;
            }
            let idx = (row_base + px as usize) * 4;
            if idx + 4 > fb_len {
                continue;
            }
            let src = [rgba[0], rgba[1], rgba[2], a.min(255) as u8];
            blend_pixel_vmo(fb, idx, &src);
        }
    }
}
