// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

/// Result type alias for shape operations.
pub type ShapeResult<T> = Result<T, ShapeError>;

/// Errors that can occur during text shaping and glyph rasterization.
#[derive(Debug)]
pub enum ShapeError {
    /// No fonts loaded.
    NoFonts,

    /// Font data is invalid or corrupt.
    InvalidFont { path: String, reason: String },

    /// A specific glyph is missing from all fonts in the fallback chain.
    MissingGlyph { glyph_index: u32, font_id: u32 },

    /// Shaping request failed (rustybuzz error).
    ShapeFailed { reason: String },

    /// Glyph cache is full and eviction failed (should not happen with LRU).
    CacheFull,
}

impl fmt::Display for ShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeError::NoFonts => write!(f, "no fonts loaded"),
            ShapeError::InvalidFont { path, reason } => {
                write!(f, "invalid font {path}: {reason}")
            }
            ShapeError::MissingGlyph { glyph_index, font_id } => {
                write!(f, "missing glyph {glyph_index} in font {font_id}")
            }
            ShapeError::ShapeFailed { reason } => {
                write!(f, "shape failed: {reason}")
            }
            ShapeError::CacheFull => write!(f, "glyph cache full"),
        }
    }
}

impl std::error::Error for ShapeError {}
