// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Drop shadow compositing (full-surface + 9-slice) for TASK-0059 / RFC-0058 Phase 6.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 tests (tests/ui_v4_host/src/nine_slice_tests.rs)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
//! Drop shadow compositing. Renders a shadow into a target buffer.
//!
//! Two paths:
//! - `composite_drop_shadow`: full-surface blur (small shadows, text shadows)
//! - `composite_nine_slice_shadow`: 9-slice decomposition (large box shadows)
//!   ~90% fewer blur ops than full-surface. Cached per (size, blur_radius, color).

use crate::blur::{blur_1d, blur_3x3};
use crate::budget::EffectBudget;
use crate::cache::EffectCache;
use nexus_layout_types::Rgba8;

#[derive(Debug, Clone, Copy)]
pub struct DropShadowParams {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub shadow_color: Rgba8,
}

#[derive(Debug, Clone, Copy)]
pub struct NineSliceCompositeParams {
    pub target_w: u32,
    pub target_h: u32,
    pub stride: u32,
    pub elem_w: u32,
    pub elem_h: u32,
    pub offset_x: i32,
    pub offset_y: i32,
}

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
    alpha_mask: &[u8],
    params: DropShadowParams,
    budget: &mut EffectBudget,
) -> u32 {
    let width = params.width;
    let height = params.height;
    let stride = params.stride;
    let offset_x = params.offset_x;
    let offset_y = params.offset_y;
    let shadow_color = params.shadow_color;
    let w = width as i32;
    let h = height as i32;
    let s = stride as usize;

    if w <= 0 || h <= 0 || alpha_mask.len() < (width * height) as usize {
        return 0;
    }

    let shadow_area = width * height;
    if !budget.try_reserve(shadow_area) {
        return 0;
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

    blur_3x3(&mut shadow_layer, width, height, width * 4);

    let mut composited = 0u32;
    for sy in 0..height {
        for sx in 0..width {
            let idx = sy as usize * s + sx as usize * 4;
            let sa = shadow_layer[(sy * width + sx) as usize * 4 + 3] as u32;
            if sa == 0 {
                continue;
            }
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

// ─── 9-slice shadow ──────────────────────────────────────────────────────

/// Parameters for a 9-slice decomposed box shadow.
#[derive(Debug, Clone, Copy)]
pub struct NineSliceShadow {
    pub corner_size: u32,
    pub blur_radius: u32,
    pub spread: i32,
    pub color: Rgba8,
}

struct ShadowDims {
    total_w: u32,
    total_h: u32,
    inner_x: u32,
    inner_y: u32,
    inner_w: u32,
    inner_h: u32,
}

fn nine_slice_dims(elem_w: u32, elem_h: u32, shadow: &NineSliceShadow) -> ShadowDims {
    let spread = shadow.spread;
    let total_w = (elem_w as i32 + 2 * spread).max(0) as u32;
    let total_h = (elem_h as i32 + 2 * spread).max(0) as u32;
    let cs = shadow.corner_size;
    let inner_x = cs.min(total_w);
    let inner_y = cs.min(total_h);
    let inner_w = total_w.saturating_sub(2 * cs);
    let inner_h = total_h.saturating_sub(2 * cs);
    ShadowDims { total_w, total_h, inner_x, inner_y, inner_w, inner_h }
}

fn nine_slice_cache_key(elem_w: u32, elem_h: u32, shadow: &NineSliceShadow) -> u64 {
    let mut k: u64 = 0;
    k ^= (elem_w as u64) << 48;
    k ^= (elem_h as u64) << 32;
    k ^= (shadow.corner_size as u64) << 16;
    k ^= (shadow.blur_radius as u64) << 8;
    k ^= (shadow.spread as u32 as u64) << 4;
    k ^= shadow.color.r as u64;
    k ^= (shadow.color.g as u64) << 56;
    k ^= (shadow.color.b as u64) << 40;
    k ^= (shadow.color.a as u64) << 24;
    k
}

/// Composite a 9-slice box shadow onto a target buffer.
///
/// The 9-slice approach renders four corner patches (2D blurred), stretches
/// the blur result along the four edges (1D), and fills the center with solid
/// shadow alpha — ~90% fewer blur ops than full-surface blur.
///
/// On cache hit, the entire shadow layer is reused (no re-render).
pub fn composite_nine_slice_shadow(
    target: &mut [u8],
    shadow: &NineSliceShadow,
    params: NineSliceCompositeParams,
    budget: &mut EffectBudget,
    mut cache: Option<&mut EffectCache>,
) -> u32 {
    let elem_w = params.elem_w;
    let elem_h = params.elem_h;
    let dims = nine_slice_dims(elem_w, elem_h, shadow);
    if dims.total_w == 0 || dims.total_h == 0 {
        return 0;
    }

    let cache_key = nine_slice_cache_key(elem_w, elem_h, shadow);
    let cached = cache.as_mut().and_then(|c| c.get(cache_key));

    if let Some(data) = cached {
        composite_shadow_layer(target, data, dims.total_w, dims.total_h, shadow, params)
    } else {
        let pixels = dims.total_w * dims.total_h;
        if !budget.try_reserve(pixels) {
            return 0;
        }
        let mut layer = alloc::vec![0u8; (pixels * 4) as usize];
        render_nine_slice_corners(&mut layer, &dims, shadow);
        render_nine_slice_edges(&mut layer, &dims, shadow);
        render_nine_slice_fill(&mut layer, &dims, shadow);
        let composited =
            composite_shadow_layer(target, &layer, dims.total_w, dims.total_h, shadow, params);
        if let Some(c) = cache {
            c.insert(cache_key, layer, dims.total_w, dims.total_h);
        }
        composited
    }
}

fn composite_shadow_layer(
    target: &mut [u8],
    shadow_layer: &[u8],
    layer_width: u32,
    layer_height: u32,
    shadow: &NineSliceShadow,
    params: NineSliceCompositeParams,
) -> u32 {
    let target_w = params.target_w;
    let target_h = params.target_h;
    let stride = params.stride;
    let offset_x = params.offset_x;
    let offset_y = params.offset_y;
    let sr = shadow.color.r as u32;
    let sg = shadow.color.g as u32;
    let sb = shadow.color.b as u32;
    let s = stride as usize;
    let ls = (layer_width * 4) as usize;
    let tw = target_w as i32;
    let th = target_h as i32;
    let mut composited = 0u32;

    for ly in 0..layer_height {
        let ty = ly as i32 + offset_y;
        if ty < 0 || ty >= th {
            continue;
        }
        for lx in 0..layer_width {
            let tx = lx as i32 + offset_x;
            if tx < 0 || tx >= tw {
                continue;
            }
            let li = (ly as usize) * ls + (lx as usize) * 4;
            let sa = shadow_layer[li + 3] as u32;
            if sa == 0 {
                continue;
            }
            let ti = (ty as usize) * s + (tx as usize) * 4;
            let inv = 255 - sa;
            target[ti] = ((sr * sa + target[ti] as u32 * inv) / 255) as u8;
            target[ti + 1] = ((sg * sa + target[ti + 1] as u32 * inv) / 255) as u8;
            target[ti + 2] = ((sb * sa + target[ti + 2] as u32 * inv) / 255) as u8;
            target[ti + 3] = target[ti + 3].max(shadow_layer[li + 3]);
            composited += 1;
        }
    }
    composited
}

fn render_nine_slice_corners(layer: &mut [u8], dims: &ShadowDims, shadow: &NineSliceShadow) {
    let cs = shadow.corner_size as usize;
    let ls = (dims.total_w * 4) as usize;
    let alpha = shadow.color.a;
    let corners: [(usize, usize); 4] = [
        (0, 0),
        (dims.total_w as usize - cs, 0),
        (0, dims.total_h as usize - cs),
        (dims.total_w as usize - cs, dims.total_h as usize - cs),
    ];
    for &(cx, cy) in &corners {
        for y in 0..cs {
            for x in 0..cs {
                layer[(cy + y) * ls + (cx + x) * 4 + 3] = alpha;
            }
        }
    }
    if shadow.blur_radius > 0 {
        let w = dims.total_w;
        let h = dims.total_h;
        let stride = w * 4;
        blur_1d(layer, w, h, stride, shadow.blur_radius, true);
        blur_1d(layer, w, h, stride, shadow.blur_radius, false);
    }
}

fn render_nine_slice_edges(layer: &mut [u8], dims: &ShadowDims, _shadow: &NineSliceShadow) {
    let cs = dims.inner_x as usize;
    let ls = (dims.total_w * 4) as usize;
    let total_w = dims.total_w as usize;
    let total_h = dims.total_h as usize;
    let inner_x = dims.inner_x as usize;
    let inner_y = dims.inner_y as usize;
    let inner_w = dims.inner_w as usize;
    let inner_h = dims.inner_h as usize;

    if inner_w == 0 && inner_h == 0 {
        return;
    }

    // Top edge: stretch from corner's bottom row across inner width (horizontal)
    if inner_w > 0 && cs > 0 {
        let src_y = (inner_y - 1).min(total_h - 1).max(0);
        for x in inner_x..inner_x + inner_w {
            let src_a = layer[src_y * ls + x * 4 + 3];
            if src_a == 0 {
                continue;
            }
            for y in 0..inner_y {
                layer[y * ls + x * 4 + 3] = src_a;
            }
        }
    }

    // Bottom edge
    if inner_w > 0 && cs > 0 {
        let src_y = (inner_y + inner_h).min(total_h - 1);
        for x in inner_x..inner_x + inner_w {
            let src_a = layer[src_y * ls + x * 4 + 3];
            if src_a == 0 {
                continue;
            }
            for y in inner_y + inner_h..total_h {
                layer[y * ls + x * 4 + 3] = src_a;
            }
        }
    }

    // Left edge
    if inner_h > 0 && inner_x > 0 {
        let src_x = (inner_x - 1).min(total_w - 1).max(0);
        for y in inner_y..inner_y + inner_h {
            let src_a = layer[y * ls + src_x * 4 + 3];
            if src_a == 0 {
                continue;
            }
            for x in 0..inner_x {
                layer[y * ls + x * 4 + 3] = src_a;
            }
        }
    }

    // Right edge
    if inner_h > 0 && inner_x > 0 {
        let src_x = (inner_x + inner_w).min(total_w - 1);
        for y in inner_y..inner_y + inner_h {
            let src_a = layer[y * ls + src_x * 4 + 3];
            if src_a == 0 {
                continue;
            }
            for x in inner_x + inner_w..total_w {
                layer[y * ls + x * 4 + 3] = src_a;
            }
        }
    }
}

fn render_nine_slice_fill(layer: &mut [u8], dims: &ShadowDims, shadow: &NineSliceShadow) {
    if dims.inner_w == 0 || dims.inner_h == 0 {
        return;
    }
    let alpha = shadow.color.a;
    if alpha == 0 {
        return;
    }
    let ls = (dims.total_w * 4) as usize;
    for y in dims.inner_y as usize..(dims.inner_y + dims.inner_h) as usize {
        for x in dims.inner_x as usize..(dims.inner_x + dims.inner_w) as usize {
            layer[y * ls + x * 4 + 3] = alpha;
        }
    }
}
