// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![allow(unsafe_code)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::error::{ShapeError, ShapeResult};
use crate::types::{FontId, GlyphIndex, GlyphRun, PixelSize, ShapedGlyph, VariationSettings};

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
/// Loads fonts from a directory recursively, builds a fallback chain, and shapes
/// text using rustybuzz (HarfBuzz-compatible shaping).
///
/// Supports variable fonts (OpenType Font Variations). When variation settings
/// are provided, they are applied to all loaded fonts that support the requested
/// axes. Fonts without matching axes silently ignore the settings.
#[derive(Debug)]
pub struct ShapeContext {
    fonts: Vec<FontEntry>,
}

/// Recursively collect font paths from a directory tree.
fn collect_font_paths(dir: &Path, paths: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_font_paths(&path, paths)?;
        } else if path.extension().map(|ext| ext == "ttf" || ext == "otf").unwrap_or(false) {
            paths.push(path);
        }
    }
    Ok(())
}

impl ShapeContext {
    /// Create a new shaping context by loading all fonts from the given directory
    /// (recursively). Uses default font coordinates (for variable fonts, this
    /// means the default instance, typically Regular/weight 400).
    pub fn new(font_dir: &Path) -> ShapeResult<Self> {
        Self::with_variation(font_dir, None)
    }

    /// Create a new shaping context with optional variation settings.
    ///
    /// When `variation` is `Some(settings)`, each loaded font that supports the
    /// requested axes will have those coordinates applied. Variable fonts without
    /// matching axes, and static fonts, silently ignore the settings.
    pub fn with_variation(
        font_dir: &Path,
        variation: Option<&VariationSettings>,
    ) -> ShapeResult<Self> {
        let mut font_paths = Vec::new();
        collect_font_paths(font_dir, &mut font_paths).map_err(|e| ShapeError::InvalidFont {
            path: font_dir.display().to_string(),
            reason: e.to_string(),
        })?;
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
            let mut face =
                rustybuzz::Face::from_slice(leaked, 0).ok_or_else(|| ShapeError::InvalidFont {
                    path: path.display().to_string(),
                    reason: "failed to parse font".to_string(),
                })?;

            // Apply variation coordinates if requested.
            if let Some(settings) = variation {
                crate::variation::apply_to_face(&mut face, settings);
            }

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
