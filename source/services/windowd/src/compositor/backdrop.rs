// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Backdrop blur and glass-layer rendering for the windowd compositor.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use alloc::vec::Vec;
use crate::assets;
use crate::error::WindowdError;
use crate::fixed_sdf;
use crate::live_runtime::{DamageRect, GlassQuality};
use crate::smoke::VisibleBootstrapMode;
use input_live_protocol::VisibleState;
use nexus_effects::blur_separable_zero_alloc;
use nexus_layout::LayoutResult;
use super::cache::{BackdropCacheEntry, GlassLayerCache, LayerCache, PathCacheEntry};
use super::primitives::rgba_to_bgra;
use super::sdf::stroke_dark_glass_border_row;
use super::types::{ProofBoxRect, RenderClip, SourceFrame};
use super::{BACKDROP_CACHE_ENTRIES, BACKDROP_CACHE_MAX_WIDTH, COMBINED_PANEL_WIDTH, COMBINED_PANEL_HEIGHT, DARK_GLASS_BLUR_RADIUS, DARK_GLASS_BORDER, DARK_GLASS_RADIUS, DARK_GLASS_SATURATION_PERCENT, DARK_GLASS_TINT, GLASS_LAYER_MAX_BYTES, GLASS_LAYER_MAX_HEIGHT, GLASS_LAYER_MAX_WIDTH, GLASS_LAYER_SCALE, PROOF_PANEL_X, PROOF_PANEL_Y};

pub(crate) fn backdrop_cache_slot(
    y: u32,
    start_x: u32,
    width: u32,
    quality: GlassQuality,
    cache_len: usize,
) -> usize {
    if cache_len == 0 {
        return 0;
    }
    let quality_key = match quality {
        GlassQuality::High => 0usize,
        GlassQuality::Low => 1,
        GlassQuality::Opaque => 2,
    };
    (y as usize)
        .wrapping_mul(131)
        .wrapping_add(start_x as usize * 17)
        .wrapping_add(width as usize * 3)
        .wrapping_add(quality_key)
        % cache_len
}


pub(crate) fn blur_backdrop_segment(
    dst: &mut [u8],
    start_x: u32,
    end_x: u32,
    radius: u32,
    scratch: &mut [u8],
) -> Result<(), WindowdError> {
    if end_x <= start_x || radius == 0 {
        return Ok(());
    }
    let r = radius as usize;
    let start = start_x as usize * 4;
    let end = end_x as usize * 4;
    let segment_len = end.saturating_sub(start);
    if end > dst.len() || segment_len > scratch.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }
    scratch[..segment_len].copy_from_slice(&dst[start..end]);
    let pixels = segment_len / 4;
    if pixels == 0 {
        return Ok(());
    }
    let mut sums = [0u32; 4];
    let mut left = 0usize;
    let mut right = r.min(pixels - 1);
    for j in left..=right {
        let bi = j * 4;
        sums[0] += scratch[bi] as u32;
        sums[1] += scratch[bi + 1] as u32;
        sums[2] += scratch[bi + 2] as u32;
        sums[3] += scratch[bi + 3] as u32;
    }
    for i in 0..pixels {
        let count = (right - left + 1) as u32;
        let di = start + i * 4;
        for c in 0..4 {
            dst[di + c] = (sums[c] / count).min(255) as u8;
        }

        if i + 1 < pixels {
            let next_left = (i + 1).saturating_sub(r);
            if next_left > left {
                let bi = left * 4;
                sums[0] = sums[0].saturating_sub(scratch[bi] as u32);
                sums[1] = sums[1].saturating_sub(scratch[bi + 1] as u32);
                sums[2] = sums[2].saturating_sub(scratch[bi + 2] as u32);
                sums[3] = sums[3].saturating_sub(scratch[bi + 3] as u32);
                left = next_left;
            }
            let next_right = (i + 1 + r).min(pixels - 1);
            if next_right > right {
                right = next_right;
                let bi = right * 4;
                sums[0] += scratch[bi] as u32;
                sums[1] += scratch[bi + 1] as u32;
                sums[2] += scratch[bi + 2] as u32;
                sums[3] += scratch[bi + 3] as u32;
            }
        }
    }
    Ok(())
}

pub(crate) fn apply_backdrop_cache_row(
    row: &mut [u8],
    y: u32,
    start_x: u32,
    end_x: u32,
    quality: GlassQuality,
    cache_entries: &mut [BackdropCacheEntry],
    scratch: &mut [u8],
) -> Result<(), WindowdError> {
    if end_x <= start_x {
        return Ok(());
    }
    let width = end_x.saturating_sub(start_x);
    let segment_len = width as usize * 4;
    if segment_len > BACKDROP_CACHE_MAX_WIDTH * 4 {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let row_start = start_x as usize * 4;
    let row_end = end_x as usize * 4;
    if row_end > row.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }
    if let Some(entry) = cache_entries.iter().find(|entry| {
        entry.valid
            && entry.y == y
            && entry.start_x == start_x
            && entry.width == width
            && entry.quality == quality
    }) {
        row[row_start..row_end].copy_from_slice(&entry.pixels[..segment_len]);
        return Ok(());
    }
    let slot = backdrop_cache_slot(y, start_x, width, quality, cache_entries.len());
    let entry = &mut cache_entries[slot];
    entry.pixels[..segment_len].copy_from_slice(&row[row_start..row_end]);
    blur_backdrop_segment(
        &mut entry.pixels[..segment_len],
        0,
        width,
        quality.blur_radius(),
        scratch,
    )?;
    saturate_bgra_segment(
        &mut entry.pixels[..segment_len],
        0,
        width,
        DARK_GLASS_SATURATION_PERCENT,
    );
    entry.y = y;
    entry.start_x = start_x;
    entry.width = width;
    entry.quality = quality;
    entry.valid = true;
    row[row_start..row_end].copy_from_slice(&entry.pixels[..segment_len]);
    Ok(())
}

fn glass_layer_key(rect: ProofBoxRect, quality: GlassQuality) -> u64 {
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    key ^= rect.x as u64;
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^= (rect.y as u64).rotate_left(7);
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^= (rect.width as u64).rotate_left(17);
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^= (rect.height as u64).rotate_left(29);
    key = key.wrapping_mul(0x0000_0100_0000_01b3);
    key ^ ((quality.blur_radius() as u64).rotate_left(41))
}

fn sample_wallpaper_pixel(
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    x: u32,
    y: u32,
) -> Result<[u8; 4], WindowdError> {
    let x = x.min(mode.width.saturating_sub(1));
    let y = y.min(mode.height.saturating_sub(1));
    let src_x = *source_x_lut
        .get(x as usize)
        .ok_or(WindowdError::BufferLengthMismatch)? as usize;
    let src_y = *source_y_lut
        .get(y as usize)
        .ok_or(WindowdError::BufferLengthMismatch)? as usize;
    let src = src_y
        .checked_mul(source_frame.stride as usize)
        .and_then(|base| base.checked_add(src_x.checked_mul(4)?))
        .ok_or(WindowdError::ArithmeticOverflow)?;
    let px = source_frame
        .pixels
        .get(src..src + 4)
        .ok_or(WindowdError::BufferLengthMismatch)?;
    Ok([px[0], px[1], px[2], px[3]])
}

fn ensure_glass_layer(
    layer: &mut GlassLayerCache,
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    rect: ProofBoxRect,
    quality: GlassQuality,
    row_scratch: &mut [u8],
    glass_scratch: &mut [u8],
) -> Result<(), WindowdError> {
    let key = glass_layer_key(rect, quality);
    let bounds = DamageRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    if layer.valid && layer.key == key && layer.rect == bounds {
        return Ok(());
    }

    let cache_w = rect.width.div_ceil(GLASS_LAYER_SCALE).max(1);
    let cache_h = rect.height.div_ceil(GLASS_LAYER_SCALE).max(1);
    let layer_len = cache_w as usize * cache_h as usize * 4;
    if layer_len > layer.pixels.len() || layer_len > glass_scratch.len() {
        return Err(WindowdError::BufferLengthMismatch);
    }

    for py in 0..cache_h {
        for px in 0..cache_w {
            let sample_x = rect
                .x
                .saturating_add(px.saturating_mul(GLASS_LAYER_SCALE))
                .saturating_add(GLASS_LAYER_SCALE / 2)
                .min(rect.x.saturating_add(rect.width.saturating_sub(1)));
            let sample_y = rect
                .y
                .saturating_add(py.saturating_mul(GLASS_LAYER_SCALE))
                .saturating_add(GLASS_LAYER_SCALE / 2)
                .min(rect.y.saturating_add(rect.height.saturating_sub(1)));
            let src = sample_wallpaper_pixel(
                source_frame,
                source_x_lut,
                source_y_lut,
                mode,
                sample_x,
                sample_y,
            )?;
            let idx = (py as usize * cache_w as usize + px as usize) * 4;
            layer.pixels[idx..idx + 4].copy_from_slice(&src);
        }
    }

    if quality != GlassQuality::Opaque {
        let blur_radius = DARK_GLASS_BLUR_RADIUS
            .min(quality.blur_radius())
            .div_ceil(GLASS_LAYER_SCALE)
            .max(1);
        blur_separable_zero_alloc(
            &mut layer.pixels[..layer_len],
            cache_w,
            cache_h,
            cache_w * 4,
            blur_radius,
            row_scratch,
            glass_scratch,
        );
        saturate_bgra_segment(
            &mut layer.pixels[..layer_len],
            0,
            cache_w,
            DARK_GLASS_SATURATION_PERCENT,
        );
    }

    layer.key = key;
    layer.rect = bounds;
    layer.width = cache_w;
    layer.height = cache_h;
    layer.valid = true;
    Ok(())
}

fn sample_glass_layer(layer: &GlassLayerCache, x: u32, y: u32) -> [u8; 4] {
    let local_x = x.saturating_sub(layer.rect.x);
    let local_y = y.saturating_sub(layer.rect.y);
    let sx = local_x / GLASS_LAYER_SCALE;
    let sy = local_y / GLASS_LAYER_SCALE;
    let x0 = sx.min(layer.width.saturating_sub(1));
    let y0 = sy.min(layer.height.saturating_sub(1));
    let x1 = x0.saturating_add(1).min(layer.width.saturating_sub(1));
    let y1 = y0.saturating_add(1).min(layer.height.saturating_sub(1));
    let fx = local_x % GLASS_LAYER_SCALE;
    let fy = local_y % GLASS_LAYER_SCALE;
    let wx1 = fx;
    let wx0 = GLASS_LAYER_SCALE.saturating_sub(fx);
    let wy1 = fy;
    let wy0 = GLASS_LAYER_SCALE.saturating_sub(fy);
    let sample = |px: u32, py: u32, c: usize| -> u32 {
        let idx = (py as usize * layer.width as usize + px as usize) * 4 + c;
        layer.pixels.get(idx).copied().unwrap_or(0) as u32
    };
    let denom = GLASS_LAYER_SCALE * GLASS_LAYER_SCALE;
    let mut out = [0u8; 4];
    for (c, dst) in out.iter_mut().enumerate() {
        let v = sample(x0, y0, c) * wx0 * wy0
            + sample(x1, y0, c) * wx1 * wy0
            + sample(x0, y1, c) * wx0 * wy1
            + sample(x1, y1, c) * wx1 * wy1;
        *dst = (v / denom).min(255) as u8;
    }
    out
}

pub(crate) fn draw_combined_panel_glass_row(
    y: u32,
    row: &mut [u8],
    rect: ProofBoxRect,
    render_clip: RenderClip,
    quality: GlassQuality,
    source_frame: &SourceFrame,
    source_x_lut: &[u32],
    source_y_lut: &[u32],
    mode: VisibleBootstrapMode,
    glass_layer: &mut GlassLayerCache,
    row_scratch: &mut [u8],
    glass_scratch: &mut [u8],
) -> Result<(), WindowdError> {
    if !rect.contains_y(y) {
        return Ok(());
    }
    ensure_glass_layer(
        glass_layer,
        source_frame,
        source_x_lut,
        source_y_lut,
        mode,
        rect,
        quality,
        row_scratch,
        glass_scratch,
    )?;
    let row_pixels = (row.len() / 4) as u32;
    let start = rect.x.max(render_clip.start_x).min(row_pixels);
    let end = rect
        .x
        .saturating_add(rect.width)
        .min(render_clip.end_x)
        .min(row_pixels);
    if start >= end {
        return Ok(());
    }
    let tint_a = DARK_GLASS_TINT.a as u32;
    let inv_tint = 255u32.saturating_sub(tint_a);
    let interior_left = rect.x.saturating_add(DARK_GLASS_RADIUS);
    let interior_right = rect
        .x
        .saturating_add(rect.width.saturating_sub(DARK_GLASS_RADIUS));
    let interior_top = rect.y.saturating_add(DARK_GLASS_RADIUS);
    let interior_bottom = rect
        .y
        .saturating_add(rect.height.saturating_sub(DARK_GLASS_RADIUS));
    if start >= interior_left && end <= interior_right && y >= interior_top && y < interior_bottom {
        for px in start..end {
            let blurred = sample_glass_layer(glass_layer, px, y);
            let idx = px as usize * 4;
            row[idx] =
                ((blurred[0] as u32 * inv_tint + DARK_GLASS_TINT.b as u32 * tint_a) / 255) as u8;
            row[idx + 1] =
                ((blurred[1] as u32 * inv_tint + DARK_GLASS_TINT.g as u32 * tint_a) / 255) as u8;
            row[idx + 2] =
                ((blurred[2] as u32 * inv_tint + DARK_GLASS_TINT.r as u32 * tint_a) / 255) as u8;
        }
        return Ok(());
    }
    let min_x = fixed_sdf::px_u32(rect.x);
    let min_y = fixed_sdf::px_u32(rect.y);
    let max_x = fixed_sdf::px_u32(rect.x.saturating_add(rect.width));
    let max_y = fixed_sdf::px_u32(rect.y.saturating_add(rect.height));
    let radius = fixed_sdf::px_u32(DARK_GLASS_RADIUS);
    let point_y = fixed_sdf::pixel_center(y);
    for px in start..end {
        let sd = fixed_sdf::rounded_rect_sd(
            fixed_sdf::pixel_center(px),
            point_y,
            min_x,
            min_y,
            max_x,
            max_y,
            radius,
        );
        let mask = fixed_sdf::fill_alpha(sd);
        if mask == 0 {
            continue;
        }
        let blurred = sample_glass_layer(glass_layer, px, y);
        let final_b = (blurred[0] as u32 * inv_tint + DARK_GLASS_TINT.b as u32 * tint_a) / 255;
        let final_g = (blurred[1] as u32 * inv_tint + DARK_GLASS_TINT.g as u32 * tint_a) / 255;
        let final_r = (blurred[2] as u32 * inv_tint + DARK_GLASS_TINT.r as u32 * tint_a) / 255;
        let inv_mask = 255u32.saturating_sub(mask);
        let idx = px as usize * 4;
        row[idx] = ((final_b * mask + row[idx] as u32 * inv_mask) / 255) as u8;
        row[idx + 1] = ((final_g * mask + row[idx + 1] as u32 * inv_mask) / 255) as u8;
        row[idx + 2] = ((final_r * mask + row[idx + 2] as u32 * inv_mask) / 255) as u8;
    }
    stroke_dark_glass_border_row(
        y,
        row,
        rect,
        render_clip,
        1,
        rgba_to_bgra(DARK_GLASS_BORDER),
    )
}

pub(crate) fn saturate_bgra_segment(
    row: &mut [u8],
    start_x: u32,
    end_x: u32,
    saturation_percent: u32,
) {
    if end_x <= start_x || saturation_percent == 100 {
        return;
    }
    let start = start_x as usize * 4;
    let end = (end_x as usize * 4).min(row.len());
    let sat = saturation_percent as i32;
    let mut idx = start;
    while idx + 3 < end {
        let b = row[idx] as i32;
        let g = row[idx + 1] as i32;
        let r = row[idx + 2] as i32;
        let gray = (29 * b + 150 * g + 77 * r) >> 8;
        row[idx] = (gray + ((b - gray) * sat) / 100).clamp(0, 255) as u8;
        row[idx + 1] = (gray + ((g - gray) * sat) / 100).clamp(0, 255) as u8;
        row[idx + 2] = (gray + ((r - gray) * sat) / 100).clamp(0, 255) as u8;
        idx += 4;
    }
}

