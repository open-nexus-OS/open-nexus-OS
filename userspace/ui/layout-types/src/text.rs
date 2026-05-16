// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::color::Rgba8;
use crate::types::FxPx;

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextAlign { Left, Center, Right }

/// Line height: relative (multiplier on font size) or absolute pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineHeight {
    /// Multiplier stored with implicit /100 scaling (1.5 = FxPx(150)).
    Relative(FxPx),
    /// Fixed pixel value.
    Absolute(FxPx),
}

impl LineHeight {
    pub fn effective(&self, font_size: FxPx) -> FxPx {
        match *self {
            LineHeight::Absolute(px) => px,
            LineHeight::Relative(mult) => {
                FxPx::new((font_size.0 as i64 * mult.0 as i64 / 100) as i32)
            }
        }
    }
}

/// Font weight. Matches Inter's available weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Regular = 400,
    Medium = 500,
    Semibold = 600,
    Bold = 700,
}

/// White-space handling mode for text wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WhiteSpace {
    /// Normal: collapse whitespace, wrap at width.
    Normal,
    /// Pre: preserve whitespace, wrap at width.
    Pre,
    /// NoWrap: collapse whitespace, never wrap.
    NoWrap,
}

/// Text style: font metrics, alignment, and color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStyle {
    pub font_size: FxPx,
    pub font_weight: FontWeight,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub color: Rgba8,
    pub white_space: WhiteSpace,
}
