// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::color::Rgba8;
use crate::types::FxPx;

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

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

/// Font weight on Inter's axis — the five weights the text styles and their overrides use
/// (RFC-0116); the baked atlas carries a face for every (size, weight) a style needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// Large headlines only — never a default UI weight
    /// (`docs/dev/ui/foundations/visual/typography.md`).
    Light = 300,
    Normal = 400,
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
    /// Letter spacing in thousandths of an em, signed (RFC-0116: the text style's tracking —
    /// `display` −20, `overline` +60). Measure and draw add it after every glyph.
    pub tracking_milli_em: i16,
    /// Tabular figures (CSS `tabular-nums`): every digit advances by the face's widest digit, so
    /// a changing number never shifts (the design artifacts' counts, clocks and values).
    pub tabular: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: FxPx::new(16),
            font_weight: FontWeight::Normal,
            line_height: LineHeight::Absolute(FxPx::new(20)),
            text_align: TextAlign::Left,
            color: Rgba8::WHITE,
            white_space: WhiteSpace::Normal,
            tracking_milli_em: 0,
            tabular: false,
        }
    }
}
