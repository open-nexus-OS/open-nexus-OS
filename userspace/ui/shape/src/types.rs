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
// Variable font support
// ---------------------------------------------------------------------------

/// An OpenType variation axis tag + target coordinate.
///
/// Common axes:
/// - `wght` (weight): 100–900 (400 = Regular, 700 = Bold)
/// - `wdth` (width): percentage of normal width (100 = normal)
/// - `slnt` (slant): angle in degrees (0 = upright)
/// - `ital` (italic): 0 = Roman, 1 = Italic
/// - `opsz` (optical size): in points
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VariationAxis {
    /// OpenType axis tag as 4 ASCII bytes (e.g. `b"wght"`).
    pub tag: [u8; 4],
    /// Target coordinate value.
    pub value: f32,
}

impl VariationAxis {
    /// Create a new variation axis setting.
    pub fn new(tag: [u8; 4], value: f32) -> Self {
        Self { tag, value }
    }

    /// Weight axis: `wght` (100–900).
    pub fn weight(value: f32) -> Self {
        Self::new(*b"wght", value)
    }

    /// Width axis: `wdth` (percentage, 100 = normal).
    pub fn width(value: f32) -> Self {
        Self::new(*b"wdth", value)
    }

    /// Optical size axis: `opsz` (in points).
    pub fn optical_size(value: f32) -> Self {
        Self::new(*b"opsz", value)
    }

    /// Slant axis: `slnt` (degrees).
    pub fn slant(value: f32) -> Self {
        Self::new(*b"slnt", value)
    }
}

/// A set of variation axis coordinates to apply to variable fonts.
///
/// Applied to all loaded fonts at context creation time. Fonts without
/// matching axes silently ignore the settings.
#[derive(Debug, Clone, Default)]
pub struct VariationSettings {
    pub axes: Vec<VariationAxis>,
}

impl VariationSettings {
    /// Create empty variation settings (use font defaults).
    pub fn new() -> Self {
        Self { axes: Vec::new() }
    }

    /// Add an axis coordinate.
    pub fn with_axis(mut self, axis: VariationAxis) -> Self {
        self.axes.push(axis);
        self
    }

    /// Convenience: request Regular weight (400).
    pub fn regular_weight() -> Self {
        Self::new().with_axis(VariationAxis::weight(400.0))
    }

    /// Convenience: request Bold weight (700).
    pub fn bold_weight() -> Self {
        Self::new().with_axis(VariationAxis::weight(700.0))
    }
}

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
