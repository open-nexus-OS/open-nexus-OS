// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Layer-cache row recording for retained GPU layers. The CPU
//! proof-surface/layout-box rasterizer (`draw_proof_surface_row` + helpers) was
//! deleted in RFC-0067 C1 — the proof/target-test panel is gone and all content
//! is now GPU-composited layers. Only the layer-cache key + row recorder remain.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use super::cache::{Layer, LayerCache};
use super::types::ProofBoxRect;
use super::{LAYER_CACHE_MAX_BYTES, LAYER_CACHE_MAX_LAYER_BYTES};
use crate::error::WindowdError;
use crate::live_runtime::DamageRect;

pub(crate) fn layer_cache_key(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub(crate) fn record_layer_cache_row(
    layer_cache: &mut LayerCache,
    id: u64,
    rect: ProofBoxRect,
    y: u32,
    row: &[u8],
    opacity: u8,
    backdrop_blur: Option<u32>,
) -> Result<(), WindowdError> {
    if !rect.contains_y(y) {
        return Ok(());
    }
    let bounds = DamageRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    let needs_insert = layer_cache
        .get(id)
        .map(|layer| {
            layer.bounds != bounds
                || layer.pixels.len() != rect.width as usize * rect.height as usize * 4
        })
        .unwrap_or(true);
    if needs_insert {
        let pixel_count = rect.width as usize * rect.height as usize * 4;
        if pixel_count > LAYER_CACHE_MAX_LAYER_BYTES
            || layer_cache.used_bytes().saturating_add(pixel_count) > LAYER_CACHE_MAX_BYTES
        {
            return Ok(());
        }
        layer_cache.insert(Layer::new(id, bounds, opacity, backdrop_blur));
    }
    let row_pixels = (row.len() / 4) as u32;
    let start_x = bounds.x.min(row_pixels);
    let end_x = bounds.end_x().min(row_pixels);
    if start_x >= end_x {
        return Ok(());
    }
    let Some(layer) = layer_cache.get_mut(id) else {
        return Ok(());
    };
    layer.opacity = opacity;
    layer.backdrop_blur = backdrop_blur;
    let local_y = y.saturating_sub(bounds.y);
    if local_y >= bounds.height {
        return Ok(());
    }
    let local_start_x = start_x.saturating_sub(bounds.x);
    let local_end_x = end_x.saturating_sub(bounds.x).min(bounds.width);
    let dst_start =
        (local_y as usize * bounds.width as usize + local_start_x as usize).saturating_mul(4);
    let dst_end =
        (local_y as usize * bounds.width as usize + local_end_x as usize).saturating_mul(4);
    let src_start = start_x as usize * 4;
    let src_end = end_x as usize * 4;
    if dst_end <= layer.pixels.len() && src_end <= row.len() {
        layer.pixels[dst_start..dst_end].copy_from_slice(&row[src_start..src_end]);
        layer.rows_filled = layer.rows_filled.saturating_add(1).min(bounds.height);
        if layer.rows_filled >= bounds.height {
            layer.dirty = false;
        }
    }
    Ok(())
}
