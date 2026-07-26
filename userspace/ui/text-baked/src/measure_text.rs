// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pixel-real `MeasureText` over the baked atlases — PROMOTED
//! from windowd's demo-mount `BakedTextMeasure` so every DSL layout host
//! (windowd mount, app-host runtime, future shells) measures identically.
//! Packs (line height, width) into the opaque prepared handle.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: parity-covered via windowd/app-host scene renders

use crate::{FontSize, Weight};
use nexus_layout_types::{
    FontWeight, FxPx, LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextContent,
    TextStyle,
};

/// Pixel-accurate measurement over the baked glyph tables.
pub struct BakedTextMeasure;

impl BakedTextMeasure {
    /// The authoring weight collapsed onto the three instances the ladder
    /// actually bakes (RFC-0082): Medium reads as Regular, Bold as SemiBold.
    #[must_use]
    pub fn weight(weight: FontWeight) -> Weight {
        match weight {
            FontWeight::Light => Weight::Light,
            FontWeight::Regular | FontWeight::Medium => Weight::Regular,
            FontWeight::Semibold | FontWeight::Bold => Weight::SemiBold,
        }
    }

    /// Style→face mapping. THE single place a `TextStyle` becomes a baked
    /// face — measurement and paint must agree or text clips mid-run, so
    /// every painter routes through here (RFC-0082).
    #[must_use]
    pub fn font(style: &TextStyle) -> FontSize {
        FontSize::nearest(style.font_size.0, Self::weight(style.font_weight))
    }

    /// The line box for a run.
    ///
    /// `LineHeight::Relative` is an EXPLICIT authoring choice (the DSL's
    /// `.leading(...)`, the design system's `--leading-*`) and wins.
    /// `LineHeight::Absolute` is `TextStyle`'s struct default that nobody
    /// sets deliberately, so it defers to the baked face's own line height —
    /// which is the only value that actually matches the glyphs and scales
    /// with the ladder (RFC-0082).
    fn line_height(style: &TextStyle, font: FontSize) -> usize {
        match style.line_height {
            nexus_layout_types::LineHeight::Relative(pct) => {
                ((style.font_size.0 as i64 * pct.0 as i64) / 100).max(1) as usize
            }
            nexus_layout_types::LineHeight::Absolute(_) => crate::line_height(font) as usize,
        }
    }
}

impl MeasureText for BakedTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let font = Self::font(style);
        let width = crate::measure(content.as_str().chars(), font) as usize;
        PreparedTextHandle((Self::line_height(style, font) << 20) | (width & 0xF_FFFF))
    }

    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
        FxPx::new((handle.0 & 0xF_FFFF) as i32)
    }

    fn layout_lines(
        &self,
        handle: &PreparedTextHandle,
        width: FxPx,
        max_lines: Option<u32>,
    ) -> LineLayout {
        let natural_width = self.measure_width(handle);
        let line_height = FxPx::new((handle.0 >> 20) as i32);
        let line = LineMetrics {
            text_range: 0..1,
            width: natural_width.min(width.max(FxPx::ONE)),
            baseline: line_height,
            height: line_height,
        };
        let lines = if matches!(max_lines, Some(0)) { alloc::vec![] } else { alloc::vec![line] };
        LineLayout { lines, natural_width }
    }
}
