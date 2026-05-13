// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![allow(unsafe_code)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::error::{ShapeError, ShapeResult};
use crate::types::{FontId, GlyphIndex, GlyphRun, PixelSize, ShapedGlyph};

/// Font data owned and shared for shaping.
struct FontEntry {
    id: FontId,
    data: Arc<Vec<u8>>,
    // The leaked slice outlives ShapeContext; collected on drop.
    _leaked: &'static [u8],
    face: rustybuzz::Face<'static>,
}

impl std::fmt::Debug for FontEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontEntry")
            .field("id", &self.id)
            .field("data_len", &self.data.len())
            .finish()
    }
}

impl Drop for FontEntry {
    fn drop(&mut self) {
        // Convert the leaked reference back to an owned box and let it drop.
        let ptr = self._leaked.as_ptr();
        let len = self._leaked.len();
        // Safety: the pointer came from Box::leak of a Vec<u8>.
        unsafe {
            let _ = Vec::from_raw_parts(ptr as *mut u8, len, len);
        }
    }
}

/// Text shaping context with font fallback chain.
///
/// Loads fonts from a directory, builds a fallback chain, and shapes
/// text using rustybuzz (HarfBuzz-compatible shaping).
#[derive(Debug)]
pub struct ShapeContext {
    fonts: Vec<FontEntry>,
}

impl ShapeContext {
    /// Create a new shaping context by loading all fonts from the given directory.
    pub fn new(font_dir: &Path) -> ShapeResult<Self> {
        let entries = fs::read_dir(font_dir).map_err(|e| ShapeError::InvalidFont {
            path: font_dir.display().to_string(),
            reason: e.to_string(),
        })?;

        let mut font_paths: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .map(|ext| ext == "ttf" || ext == "otf" || ext == "woff2")
                    .unwrap_or(false)
            })
            .collect();
        font_paths.sort();

        if font_paths.is_empty() {
            return Err(ShapeError::NoFonts);
        }

        let mut fonts = Vec::with_capacity(font_paths.len());
        for (i, path) in font_paths.iter().enumerate() {
            let data = fs::read(path).map_err(|e| ShapeError::InvalidFont {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;
            let arc_data: Arc<Vec<u8>> = Arc::new(data);
            // Leak a 'static slice for rustybuzz Face (reclaimed on drop).
            let leaked: &'static [u8] = Box::leak(arc_data.as_ref().clone().into_boxed_slice());
            let face =
                rustybuzz::Face::from_slice(leaked, 0).ok_or_else(|| ShapeError::InvalidFont {
                    path: path.display().to_string(),
                    reason: "failed to parse font".to_string(),
                })?;
            fonts.push(FontEntry { id: FontId(i as u32), data: arc_data, _leaked: leaked, face });
        }

        Ok(ShapeContext { fonts })
    }

    /// Shape text with the given pixel size and direction.
    #[must_use = "GlyphRun must be consumed"]
    pub fn shape(
        &self,
        text: &str,
        pixel_size: PixelSize,
        direction: rustybuzz::Direction,
    ) -> ShapeResult<GlyphRun> {
        if self.fonts.is_empty() {
            return Err(ShapeError::NoFonts);
        }

        let primary = &self.fonts[0];
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.set_direction(direction);
        buffer.push_str(text);
        let glyph_buffer = rustybuzz::shape(&primary.face, &[], buffer);

        let positions = glyph_buffer.glyph_positions();
        let infos = glyph_buffer.glyph_infos();
        let mut glyphs = Vec::with_capacity(infos.len());
        let mut cluster_map = Vec::with_capacity(infos.len());

        for (i, info) in infos.iter().enumerate() {
            let pos = &positions[i];
            glyphs.push(ShapedGlyph {
                glyph_index: GlyphIndex(info.glyph_id),
                x: pos.x_offset,
                y: pos.y_offset,
                advance: pos.x_advance,
                font_id: primary.id,
            });
            cluster_map.push(info.cluster);
        }

        let total_advance: i32 = positions.iter().map(|p| p.x_advance).sum();
        Ok(GlyphRun {
            glyphs,
            cluster_map,
            width: total_advance.max(0) as u32,
            height: pixel_size.0 as u32,
        })
    }

    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }
}
