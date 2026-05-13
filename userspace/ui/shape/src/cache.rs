// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use crate::error::ShapeResult;
use crate::types::{FontId, GlyphBitmap, GlyphIndex, PixelSize};

/// Cache key: (FontId, GlyphIndex, PixelSize).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    font_id: FontId,
    glyph_index: GlyphIndex,
    pixel_size: PixelSize,
}

/// Bounded LRU glyph cache.
///
/// Rasterizes glyphs via fontdue and caches the resulting grayscale
/// bitmaps. When the cache reaches capacity, the least-recently-used
/// entry is evicted.
///
/// # Determinism
///
/// Rasterization is deterministic for a given (font, glyph, size) tuple.
/// The cache does not affect output correctness, only performance.
#[derive(Debug)]
pub struct GlyphCache {
    /// Maximum number of cached glyphs.
    capacity: usize,
    /// Cached bitmaps, keyed by (font, glyph, size).
    entries: HashMap<CacheKey, GlyphBitmap>,
    /// LRU order: most-recently-used at the end.
    lru: Vec<CacheKey>,
}

impl GlyphCache {
    /// Create a new glyph cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        GlyphCache {
            capacity,
            entries: HashMap::with_capacity(capacity),
            lru: Vec::with_capacity(capacity),
        }
    }

    /// Get a rasterized glyph bitmap, either from cache or by rasterizing.
    ///
    /// Returns a reference to the cached bitmap. The reference is valid
    /// until the next mutating operation on the cache.
    pub fn get_or_rasterize(
        &mut self,
        font: &fontdue::Font,
        font_id: FontId,
        glyph_index: GlyphIndex,
        pixel_size: PixelSize,
    ) -> ShapeResult<&GlyphBitmap> {
        let key = CacheKey {
            font_id,
            glyph_index,
            pixel_size,
        };

        // Cache hit — move to end of LRU
        if let Some(pos) = self.lru.iter().position(|k| *k == key) {
            self.lru.remove(pos);
            self.lru.push(key);
            return Ok(&self.entries[&key]);
        }

        // Cache miss — rasterize
        let (metrics, bitmap_data) =
            font.rasterize(glyph_index.0 as u16, pixel_size.0 as f32);

        let bitmap = GlyphBitmap::new(
            metrics.width as u32,
            metrics.height as u32,
            bitmap_data,
        );

        // Evict if at capacity
        if self.lru.len() >= self.capacity {
            if let Some(evicted_key) = self.lru.first().copied() {
                self.entries.remove(&evicted_key);
                self.lru.remove(0);
            }
        }

        self.entries.insert(key, bitmap);
        self.lru.push(key);
        Ok(&self.entries[&key])
    }

    /// Number of cached glyphs.
    pub fn len(&self) -> usize {
        self.lru.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.lru.is_empty()
    }

    /// Clear all cached glyphs.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Load the Inter-Regular font from resources.
    fn load_test_font() -> fontdue::Font {
        // Use a minimal embedded font for testing
        // Created from Inter-Regular which is in resources/fonts/inter/
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("resources")
            .join("fonts")
            .join("inter");
        // For host tests, we'd need an actual font file.
        // Use fontdue's built-in test if no file is available.
        match std::fs::read_dir(&path) {
            Ok(mut entries) => {
                if let Some(Ok(entry)) = entries.next() {
                    let p = entry.path();
                    if p.extension().map(|e| e == "ttf").unwrap_or(false) {
                        let data = std::fs::read(&p).unwrap();
                        return fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
                            .unwrap();
                    }
                }
                panic!("no ttf font found in resources/fonts/inter/")
            }
            Err(_) => {
                // Fallback for tests without font files
                // Use fontdue's embedded default
                fontdue::Font::from_bytes(
                    include_bytes!("../../../../../resources/fonts/inter/Inter-Regular.ttf")
                        .to_vec(),
                    fontdue::FontSettings::default(),
                )
                .unwrap()
            }
        }
    }
}
