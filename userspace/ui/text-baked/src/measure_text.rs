// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pixel-real `MeasureText` over the baked atlases — PROMOTED
//! from windowd's demo-mount `BakedTextMeasure` so every DSL layout host
//! (windowd mount, app-host runtime, future shells) measures identically.
//! A `TextStyle` becomes a face, a tracking and a line box HERE and nowhere
//! else (RFC-0116 text styles). Packs (line height, width) into the opaque
//! prepared handle.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 unit tests below (line box, weights, tracking) + parity via scene renders

use crate::{FontSize, Tracking, Weight};
use nexus_layout_types::{
    FontWeight, FxPx, LineHeight, LineLayout, LineMetrics, MeasureText, PreparedTextHandle,
    TextContent, TextStyle,
};

/// Pixel-accurate measurement over the baked glyph tables.
pub struct BakedTextMeasure;

impl BakedTextMeasure {
    /// The authoring weight as the ladder's weight — one to one. Whether that
    /// weight is baked at the run's size is [`Self::font`]'s question.
    #[must_use]
    pub fn weight(weight: FontWeight) -> Weight {
        match weight {
            FontWeight::Light => Weight::Light,
            FontWeight::Normal => Weight::Regular,
            FontWeight::Medium => Weight::Medium,
            FontWeight::Semibold => Weight::SemiBold,
            FontWeight::Bold => Weight::Bold,
        }
    }

    /// Style→face mapping. THE single place a `TextStyle` becomes a baked
    /// face — measurement and paint must agree or text clips mid-run, so
    /// every painter routes through here. A text style's size and weight
    /// always has its own face; anything else lands on the largest face not
    /// above it ([`FontSize::nearest`]: the size, then the nearest weight baked
    /// there).
    #[must_use]
    pub fn font(style: &TextStyle) -> FontSize {
        FontSize::nearest(style.font_size.0, Self::weight(style.font_weight))
    }

    /// The run's letter-spacing. Like [`Self::font`], the one place it is read
    /// from a style, so the layout and every painter add the same amount.
    #[must_use]
    pub fn tracking(style: &TextStyle) -> Tracking {
        Tracking { milli_em: style.tracking_milli_em, tabular: style.tabular }
    }

    /// The line box, and it is authoritative: an absolute line height is the
    /// text style's line (`body` is 14/20 — its box is 20 px tall whatever the
    /// face's own ascent + descent), a relative one a multiple of the font
    /// size (× 100). Never below one pixel. A painter places the face's own band
    /// (`crate::line_height(font)`) in it with half-leading.
    #[must_use]
    pub fn line_box(style: &TextStyle) -> usize {
        let px = match style.line_height {
            LineHeight::Absolute(px) => i64::from(px.0),
            LineHeight::Relative(pct) => i64::from(style.font_size.0) * i64::from(pct.0) / 100,
        };
        px.clamp(1, 0xFFF) as usize
    }
}

impl MeasureText for BakedTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let (font, tracking) = (Self::font(style), Self::tracking(style));
        let width = crate::measure(content.as_str().chars(), font, tracking) as usize;
        PreparedTextHandle((Self::line_box(style) << 20) | (width & 0xF_FFFF))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn style(px: i32, weight: FontWeight, line: LineHeight, tracking: i16) -> TextStyle {
        TextStyle {
            font_size: FxPx::new(px),
            font_weight: weight,
            line_height: line,
            tracking_milli_em: tracking,
            ..TextStyle::default()
        }
    }

    /// The laid-out line of `text` in `style`: (width, height).
    fn line(text: &str, style: &TextStyle) -> (i32, i32) {
        let m = BakedTextMeasure;
        let handle = m.prepare(&TextContent::new(text), style);
        let laid = m.layout_lines(&handle, FxPx::new(4000), None);
        (laid.lines[0].width.0, laid.lines[0].height.0)
    }

    #[test]
    fn an_absolute_line_height_is_authoritative() {
        // `body` 14/20: the box is the style's 20 px, not the face's own line.
        let body = style(14, FontWeight::Normal, LineHeight::Absolute(FxPx::new(20)), 0);
        assert_ne!(crate::line_height(BakedTextMeasure::font(&body)), 20);
        assert_eq!(line("Handgloves", &body).1, 20);
        // `display` 64/64 is SHORTER than its face's line — still the style's.
        let display = style(64, FontWeight::Light, LineHeight::Absolute(FxPx::new(64)), -20);
        assert!(crate::line_height(BakedTextMeasure::font(&display)) > 64);
        assert_eq!(line("12:45", &display).1, 64);
        // `caption2` 10/13 and a relative line (150 % of 14 = 21).
        let caption2 = style(10, FontWeight::Medium, LineHeight::Absolute(FxPx::new(13)), 20);
        assert_eq!(line("Ansicht", &caption2).1, 13);
        let relative = style(14, FontWeight::Normal, LineHeight::Relative(FxPx::new(150)), 0);
        assert_eq!(line("Ansicht", &relative).1, 21);
        // A degenerate line never collapses the box.
        let zero = style(14, FontWeight::Normal, LineHeight::Absolute(FxPx::ZERO), 0);
        assert_eq!(line("Ansicht", &zero).1, 1);
    }

    #[test]
    fn weights_map_one_to_one_and_resolve_at_the_size() {
        let at = |px, weight| {
            BakedTextMeasure::font(&style(px, weight, LineHeight::Absolute(FxPx::new(20)), 0))
        };
        // No blanket collapse: Medium is Medium, Bold is Bold where baked.
        assert_eq!(at(11, FontWeight::Medium), FontSize::S11W500, "caption");
        assert_eq!(at(14, FontWeight::Medium), FontSize::S14W500, "body + medium");
        assert_eq!(at(10, FontWeight::Bold), FontSize::S10W700, "caption2 + bold");
        assert_eq!(at(30, FontWeight::Bold), FontSize::S30W700, "largeTitle");
        assert_eq!(at(64, FontWeight::Light), FontSize::S64W300, "display");
        assert_eq!(at(128, FontWeight::Light), FontSize::S128W300, "clock");
        // …and the nearest weight baked at that size where it is not.
        assert_eq!(at(11, FontWeight::Normal), FontSize::S11W500);
        assert_eq!(at(14, FontWeight::Bold), FontSize::S14W600);
        assert_eq!(at(24, FontWeight::Normal), FontSize::S24W700);
    }

    #[test]
    fn tracking_flows_from_the_style_into_the_measure() {
        // `overline` 11/14/600 at +0.06 em: the laid-out width is the tracked
        // run, exactly what the painter's `measure` with the same tracking says.
        let overline = style(11, FontWeight::Semibold, LineHeight::Absolute(FxPx::new(14)), 60);
        let font = BakedTextMeasure::font(&overline);
        let tracking = BakedTextMeasure::tracking(&overline);
        assert_eq!(tracking, Tracking::em(60));
        let text = "NETWORK";
        let tracked = crate::measure(text.chars(), font, tracking) as i32;
        assert_eq!(line(text, &overline).0, tracked);
        assert!(tracked > crate::measure(text.chars(), font, Tracking::NONE) as i32);
    }
}
