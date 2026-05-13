// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

// ---------------------------------------------------------------------------
// Newtypes
// ---------------------------------------------------------------------------

/// Unique identifier for a loaded font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct FontId(pub u32);

/// Glyph index within a font face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct GlyphIndex(pub u32);

/// Font pixel size (ppem).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PixelSize(pub u16);

// ---------------------------------------------------------------------------
// GlyphBitmap
// ---------------------------------------------------------------------------

/// Rasterized grayscale glyph bitmap (8-bit alpha per pixel).
/// Row-major, top-to-bottom.
#[derive(Debug, Clone)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    /// 8-bit grayscale alpha values, row-major.
    pub data: Vec<u8>,
}

impl GlyphBitmap {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Self {
        GlyphBitmap { width, height, data }
    }
}

// ---------------------------------------------------------------------------
// GlyphRun
// ---------------------------------------------------------------------------

/// A shaped sequence of positioned glyphs produced by the shaping engine.
#[derive(Debug, Clone)]
#[must_use = "GlyphRun must be consumed — call rasterize or serialize"]
pub struct GlyphRun {
    /// Positioned glyphs in visual order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Cluster map: logical character index → glyph index.
    pub cluster_map: Vec<u32>,
    /// Bounding width in pixels.
    pub width: u32,
    /// Bounding height in pixels.
    pub height: u32,
}

/// A single shaped glyph with position and advance.
#[derive(Debug, Clone, Copy)]
pub struct ShapedGlyph {
    /// Glyph index in the font.
    pub glyph_index: GlyphIndex,
    /// Horizontal offset from origin.
    pub x: i32,
    /// Vertical offset from baseline.
    pub y: i32,
    /// Horizontal advance to next glyph.
    pub advance: i32,
    /// Which font provided this glyph.
    pub font_id: FontId,
}

impl fmt::Display for FontId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FontId({})", self.0)
    }
}

impl fmt::Display for GlyphIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GlyphIndex({})", self.0)
    }
}

impl fmt::Display for PixelSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}px", self.0)
    }
}
