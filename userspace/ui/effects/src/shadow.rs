// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Drop shadow compositing. Renders a shadow into a target buffer by
//! offsetting the source alpha and applying a blur pass.

use crate::blur::blur_3x3;
use crate::budget::EffectBudget;
use nexus_layout_types::Rgba8;

/// Composite a drop shadow from a source alpha mask onto a target RGBA buffer.
///
/// - `target`: RGBA8888 pixel buffer (`width` × `height`, `stride` bytes/row)
/// - `alpha_mask`: single-channel alpha (0-255), same dimensions as target
/// - `offset_x`, `offset_y`: shadow displacement in pixels
/// - `shadow_color`: color of the shadow (RGBA)
/// - `budget`: per-frame pixel allowance
///
/// Returns the number of shadow pixels composited, or 0 if budget exhausted.
pub fn composite_drop_shadow(
    target: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
    alpha_mask: &[u8],
    offset_x: i32,
    offset_y: i32,
    shadow_color: Rgba8,
    budget: &mut EffectBudget,
) -> u32 {
    let w = width as i32;
    let h = height as i32;
    let s = stride as usize;

    if w <= 0 || h <= 0 || alpha_mask.len() < (width * height) as usize {
        return 0;
    }

    // Build shadow layer: alpha_mask shifted by offset, tinted with shadow_color
    let shadow_area = (width * height) as u32;
    if !budget.try_reserve(shadow_area) {
        return 0; // budget exhausted — degrade gracefully
    }

    let mut shadow_layer = alloc::vec![0u8; (width * height * 4) as usize];
    let shadow_r = shadow_color.r;
    let shadow_g = shadow_color.g;
    let shadow_b = shadow_color.b;

    for sy in 0..height {
        for sx in 0..width {
            let src_x = sx as i32 - offset_x;
            let src_y = sy as i32 - offset_y;
            if src_x < 0 || src_x >= w || src_y < 0 || src_y >= h {
                continue;
            }
            let src_idx = (src_y as u32 * width + src_x as u32) as usize;
            let alpha = alpha_mask[src_idx];
            if alpha == 0 {
                continue;
            }
            let dst_idx = sy as usize * s + sx as usize * 4;
            shadow_layer[dst_idx] = shadow_r;
            shadow_layer[dst_idx + 1] = shadow_g;
            shadow_layer[dst_idx + 2] = shadow_b;
            shadow_layer[dst_idx + 3] = alpha;
        }
    }

    // Blur the shadow
    blur_3x3(&mut shadow_layer, width, height, width * 4);

    // Composite shadow onto target
    let mut composited = 0u32;
    for sy in 0..height {
        for sx in 0..width {
            let idx = sy as usize * s + sx as usize * 4;
            let sa = shadow_layer[(sy * width + sx) as usize * 4 + 3] as u32;
            if sa == 0 {
                continue;
            }
            // Over operator: dst = src + dst * (1 - src_alpha/255)
            let inv = 255 - sa;
            for c in 0..3 {
                let d = target[idx + c] as u32;
                let s = shadow_layer[idx + c] as u32;
                target[idx + c] = ((s * sa + d * inv) / 255) as u8;
            }
            target[idx + 3] = target[idx + 3].max(shadow_layer[idx + 3]);
            composited += 1;
        }
    }

    composited
}
