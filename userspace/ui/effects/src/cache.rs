// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Specialized LRU render caches (ShadowCache, TextCache, RenderCache) for TASK-0059 / RFC-0058 Phase 6f.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 15 tests (tests/ui_v4_host/src/cache_tests.rs)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
//! Specialized LRU render caches for shadow layers and text glyphs.
//!
//! - `ShadowCache`: 256-entry LRU for pre-computed shadow layers.
//!   Keyed by `(node_id_hash, blur_radius, spread, color_hash)`.
//! - `TextCache`: 512-entry LRU for pre-rendered glyph bitmaps.
//!   Keyed by `(glyph_id, scale_bucket)`.
//! - `EffectCache`: kept for 9-slice shadow backward compatibility.
//! - `RenderCache`: aggregator with `invalidate()`, `clear()`, damage awareness.

use alloc::vec::Vec;

// ─── ShadowCache ─────────────────────────────────────────────────────────

const SHADOW_CACHE_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct CachedShadow {
    pub key: u64,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub age: u64,
}

#[derive(Debug, Clone)]
pub struct ShadowCache {
    entries: Vec<CachedShadow>,
    capacity: usize,
    generation: u64,
}

impl ShadowCache {
    pub fn new() -> Self {
        Self::with_capacity(SHADOW_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { entries: Vec::with_capacity(capacity), capacity, generation: 0 }
    }

    pub fn get(&mut self, key: u64) -> Option<&[u8]> {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.age = self.generation;
                return Some(&entry.data);
            }
        }
        None
    }

    pub fn insert(&mut self, key: u64, data: Vec<u8>, width: u32, height: u32) {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.data = data;
                entry.width = width;
                entry.height = height;
                entry.age = self.generation;
                return;
            }
        }
        if self.entries.len() >= self.capacity {
            let lru = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.age)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.entries.swap_remove(lru);
        }
        self.entries.push(CachedShadow { key, data, width, height, age: self.generation });
    }

    pub fn invalidate_node(&mut self, node_id_hash: u32) {
        let _prefix = (node_id_hash as u64) << 32;
        self.entries.retain(|e| (e.key >> 32) as u32 != node_id_hash);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for ShadowCache {
    fn default() -> Self {
        Self::new()
    }
}

// ─── TextCache ───────────────────────────────────────────────────────────

const TEXT_CACHE_CAPACITY: usize = 512;

#[derive(Debug, Clone)]
pub struct CachedGlyph {
    pub key: u64,
    pub bitmap: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub age: u64,
}

#[derive(Debug, Clone)]
pub struct TextCache {
    entries: Vec<CachedGlyph>,
    capacity: usize,
    generation: u64,
}

impl TextCache {
    pub fn new() -> Self {
        Self::with_capacity(TEXT_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { entries: Vec::with_capacity(capacity), capacity, generation: 0 }
    }

    pub fn get(&mut self, key: u64) -> Option<&[u8]> {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.age = self.generation;
                return Some(&entry.bitmap);
            }
        }
        None
    }

    pub fn insert(&mut self, key: u64, bitmap: Vec<u8>, width: u32, height: u32) {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.bitmap = bitmap;
                entry.width = width;
                entry.height = height;
                entry.age = self.generation;
                return;
            }
        }
        if self.entries.len() >= self.capacity {
            let lru = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.age)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.entries.swap_remove(lru);
        }
        self.entries.push(CachedGlyph { key, bitmap, width, height, age: self.generation });
    }

    pub fn invalidate_scale(&mut self, scale_bucket: u16) {
        let _prefix = (scale_bucket as u64) << 48;
        self.entries.retain(|e| (e.key >> 48) as u16 != scale_bucket);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for TextCache {
    fn default() -> Self {
        Self::new()
    }
}

// ─── EffectCache ─────────────────────────────────────────────────────────

const DEFAULT_EFFECT_CACHE_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub struct CachedEffect {
    pub key: u64,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub age: u64,
}

#[derive(Debug, Clone)]
pub struct EffectCache {
    entries: Vec<CachedEffect>,
    capacity: usize,
    generation: u64,
}

impl EffectCache {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_EFFECT_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { entries: Vec::with_capacity(capacity), capacity, generation: 0 }
    }

    pub fn get(&mut self, key: u64) -> Option<&[u8]> {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.age = self.generation;
                return Some(&entry.data);
            }
        }
        None
    }

    pub fn insert(&mut self, key: u64, data: Vec<u8>, width: u32, height: u32) {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.data = data;
                entry.width = width;
                entry.height = height;
                entry.age = self.generation;
                return;
            }
        }
        if self.entries.len() >= self.capacity {
            let lru = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.age)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.entries.swap_remove(lru);
        }
        self.entries.push(CachedEffect { key, data, width, height, age: self.generation });
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for EffectCache {
    fn default() -> Self {
        Self::new()
    }
}

// ─── RenderCache ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RenderCache {
    pub shadows: ShadowCache,
    pub text: TextCache,
    pub effects: EffectCache,
    pub generation: u64,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            shadows: ShadowCache::new(),
            text: TextCache::new(),
            effects: EffectCache::new(),
            generation: 0,
        }
    }

    pub fn begin_frame(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Conservative invalidation: any damage clears shadow cache.
    /// Text cache survives damage (glyph shapes don't change on repaint).
    pub fn invalidate_dirty(&mut self, _dirty_rows: Option<(u32, u32)>) {
        if _dirty_rows.is_some() {
            self.shadows.clear();
        }
    }

    pub fn note_scroll(&mut self) {
        // No-op: scroll repositions, doesn't change rendered content
    }

    pub fn clear(&mut self) {
        self.shadows.clear();
        self.text.clear();
        self.effects.clear();
    }
}

// ─── ShadowArena: Zero-Alloc Pre-Allocated Buffer Pool ─────────────────

/// Maximum total bytes in the shadow arena.
/// Sized for one full-box shadow at 4× downscale (worst case: glass panel ~900×320).
pub const SHADOW_ARENA_SIZE: usize = 128 * 1024; // 128 KiB

/// A caller-owned, fixed-size buffer for offscreen shadow rendering.
///
/// Bump-allocates slices within the buffer; resets each frame.
/// The arena never owns a `Vec`; OS callers provide storage from their fixed
/// render budget and host tests can use stack/static arrays.
#[derive(Debug)]
pub struct ShadowArena<'a> {
    buf: &'a mut [u8],
    used: usize,
}

impl<'a> ShadowArena<'a> {
    /// Create an arena over caller-owned storage.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, used: 0 }
    }

    /// Alias that makes call sites explicit when the backing storage is reused.
    pub fn from_buffer(buf: &'a mut [u8]) -> Self {
        Self::new(buf)
    }

    /// Create an arena over caller-owned storage while preserving a previous bump offset.
    pub fn from_buffer_with_used(buf: &'a mut [u8], used: usize) -> Self {
        let capacity = buf.len();
        Self { buf, used: used.min(capacity) }
    }

    /// Reserve `len` bytes in the arena. Returns `(offset, &mut [u8])` on success,
    /// or `None` if the arena is full.
    ///
    /// Zero heap allocations. The returned slice lives until the next `reset()`.
    pub fn alloc(&mut self, len: usize) -> Option<(usize, &mut [u8])> {
        let start = self.used;
        let end = start.checked_add(len)?;
        if end > self.buf.len() {
            return None;
        }
        self.used = end;
        Some((start, &mut self.buf[start..end]))
    }

    /// Return a reference to previously allocated data.
    pub fn get(&self, offset: usize, len: usize) -> Option<&[u8]> {
        let end = offset.checked_add(len)?;
        if end > self.used {
            return None;
        }
        Some(&self.buf[offset..end])
    }

    /// Reset the arena for the next frame. All previous allocations are invalidated.
    pub fn reset(&mut self) {
        self.used = 0;
    }

    /// Total capacity of the arena.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Bytes currently used.
    pub fn used_bytes(&self) -> usize {
        self.used
    }
}
