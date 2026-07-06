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

use crate::FontSize;
use nexus_layout_types::{
    FxPx, LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextContent, TextStyle,
};

/// Pixel-accurate measurement over the baked glyph tables.
pub struct BakedTextMeasure;

impl BakedTextMeasure {
    /// Style→face mapping (the shell convention: ≥15px = body face).
    #[must_use]
    pub fn font(style: &TextStyle) -> FontSize {
        if style.font_size.0 >= 15 {
            FontSize::Body
        } else {
            FontSize::Small
        }
    }
}

impl MeasureText for BakedTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let font = Self::font(style);
        let width = crate::measure(content.as_str().chars(), font) as usize;
        let line_height = crate::line_height(font) as usize;
        PreparedTextHandle((line_height << 20) | (width & 0xF_FFFF))
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
        let lines =
            if matches!(max_lines, Some(0)) { alloc::vec![] } else { alloc::vec![line] };
        LineLayout { lines, natural_width }
    }
}
