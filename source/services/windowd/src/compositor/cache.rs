// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cache structures for the windowd compositor: shadow box cache, backdrop cache,
//! glass layer cache, path cache, and retained layer cache.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

use crate::live_runtime::{DamageRect, GlassQuality};
use alloc::vec::Vec;

use super::{
    BACKDROP_CACHE_MAX_WIDTH, GLASS_LAYER_MAX_BYTES, LAYER_CACHE_MAX_BYTES,
    LAYER_CACHE_MAX_LAYER_BYTES, PATH_CACHE_MAX_PIXELS, SHADOW_BOX_CACHE_ENTRIES,
};

const LAYER_CACHE_MAX_ENTRIES: usize = 4;

/// Per-box shadow cache entry: stores arena offset for pre-rendered full-box shadow.
/// Zero heap allocation — fixed-size array, linear-probe lookup.
#[derive(Clone, Copy)]
pub(crate) struct ShadowBoxCacheEntry {
    pub(crate) key: u64,
    pub(crate) arena_offset: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) cache_width: u32,
    pub(crate) cache_height: u32,
    pub(crate) scale: u8,
    pub(crate) valid: bool,
}

impl ShadowBoxCacheEntry {
    pub(crate) const fn empty() -> Self {
        Self {
            key: 0,
            arena_offset: 0,
            width: 0,
            height: 0,
            cache_width: 0,
            cache_height: 0,
            scale: 1,
            valid: false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct BackdropCacheEntry {
    pub(crate) y: u32,
    pub(crate) start_x: u32,
    pub(crate) width: u32,
    pub(crate) quality: GlassQuality,
    pub(crate) valid: bool,
    pub(crate) pixels: Vec<u8>,
}

impl BackdropCacheEntry {
    pub(crate) fn new() -> Self {
        Self {
            y: 0,
            start_x: 0,
            width: 0,
            quality: GlassQuality::High,
            valid: false,
            pixels: alloc::vec![0u8; BACKDROP_CACHE_MAX_WIDTH * 4],
        }
    }
}

#[derive(Clone)]
pub(crate) struct GlassLayerCache {
    pub(crate) key: u64,
    pub(crate) rect: DamageRect,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) valid: bool,
    pub(crate) pixels: Vec<u8>,
}

impl GlassLayerCache {
    pub(crate) fn new() -> Self {
        Self {
            key: 0,
            rect: DamageRect { x: 0, y: 0, width: 0, height: 0 },
            width: 0,
            height: 0,
            valid: false,
            pixels: alloc::vec![0u8; GLASS_LAYER_MAX_BYTES],
        }
    }
}

#[derive(Clone)]
pub(crate) struct PathCacheEntry {
    pub(crate) id_hash: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color: [u8; 4],
    pub(crate) valid: bool,
    pub(crate) pixels: Vec<u8>,
}

impl PathCacheEntry {
    pub(crate) fn new() -> Self {
        Self {
            id_hash: 0,
            width: 0,
            height: 0,
            color: [0; 4],
            valid: false,
            pixels: alloc::vec![0u8; PATH_CACHE_MAX_PIXELS],
        }
    }
}

/// A retained render layer for a panel or UI element.
/// Holds pre-rendered pixel data so we can skip re-rendering when not dirty.
#[derive(Clone)]
pub(crate) struct Layer {
    pub(crate) id: u64,
    pub(crate) bounds: DamageRect,
    pub(crate) pixels: Vec<u8>,
    pub(crate) dirty: bool,
    pub(crate) rows_filled: u32,
    pub(crate) opacity: u8,
    pub(crate) backdrop_blur: Option<u32>,
}

impl Layer {
    pub(crate) fn new(
        id: u64,
        bounds: DamageRect,
        opacity: u8,
        backdrop_blur: Option<u32>,
    ) -> Self {
        let pixel_count = bounds.width as usize * bounds.height as usize * 4;
        Self {
            id,
            bounds,
            pixels: alloc::vec![0u8; pixel_count],
            dirty: true,
            rows_filled: 0,
            opacity,
            backdrop_blur,
        }
    }
}

/// Simple layer cache: retains pre-rendered pixel data per layer.
#[derive(Clone)]
pub(crate) struct LayerCache {
    pub(crate) layers: Vec<Layer>,
}

impl Default for LayerCache {
    fn default() -> Self {
        Self { layers: Vec::new() }
    }
}

impl LayerCache {
    pub(crate) fn clear(&mut self) {
        self.layers.clear();
    }
    pub(crate) fn len(&self) -> usize {
        self.layers.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub(crate) fn insert(&mut self, layer: Layer) {
        if let Some(existing) = self.layers.iter_mut().find(|l| l.id == layer.id) {
            *existing = layer;
            return;
        }
        if self.layers.len() >= LAYER_CACHE_MAX_ENTRIES {
            // Cache is best-effort; when full, skip inserting a new entry
            // instead of growing the backing Vec and risking runtime OOM.
            return;
        }
        self.layers.push(layer);
    }

    pub(crate) fn used_bytes(&self) -> usize {
        self.layers.iter().map(|layer| layer.pixels.len()).sum()
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: u64) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub(crate) fn invalidate(&mut self, id: u64) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id == id) {
            layer.dirty = true;
            layer.rows_filled = 0;
        }
    }

    pub(crate) fn mark_clean(&mut self, id: u64) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id == id) {
            layer.dirty = false;
        }
    }
}
