// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host-tested adapter that turns the pure desktop-shell scene
//! ([`crate::build_desktop_scene`]) into positioned, themed `LayoutBox`es via
//! `nexus_layout`, ready for the compositor to rasterize. Lives in the desktop
//! shell crate (not the compositor) so windowd consumes it as a client
//! (RFC-0067 P3).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 tests
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

use nexus_layout::{LayoutEngine, LayoutResult};
use nexus_layout_types::{
    FxPx, LineHeight, LineLayout, LineMetrics, MeasureText, PreparedTextHandle, TextContent,
    TextStyle,
};
use nexus_theme_tokens::BaseTokens;

use crate::build_desktop_scene;
use alloc::vec;
use alloc::vec::Vec;

/// The live display width windowd composites at (matches `compositor::DESKTOP_LAYOUT_WIDTH`).
/// The live caller passes the compositor's own constant into
/// [`compute_desktop_layout`]; this default backs host tests.
pub const DESKTOP_LAYOUT_WIDTH: u32 = 1280;

/// Approximate per-character advance as a fraction of the font size (in 1/100).
/// The shell's chrome text (`menu`, `Search…`, `chat`, `Chat`, `x`) is not in
/// the pre-rendered proof asset table, so widths are estimated geometrically —
/// good enough to produce sane rects for layout. Real glyph metrics land when
/// the shell text is rasterized.
const ADVANCE_PER_CHAR_PCT: i32 = 55;

/// A deterministic, font-asset-free text measurer for the shell scene. Encodes
/// the estimated `(width, line_height)` into the opaque [`PreparedTextHandle`]
/// so `measure_width`/`layout_lines` can recover them without the content.
pub struct EstimateTextMeasure;

impl EstimateTextMeasure {
    fn estimate(content: &TextContent, style: &TextStyle) -> (i32, i32) {
        let chars = content.as_str().chars().count() as i32;
        let font = style.font_size.0.max(1);
        let width = (chars * font * ADVANCE_PER_CHAR_PCT) / 100;
        let line_height = style.line_height.effective(style.font_size).0.max(font);
        (width.max(0), line_height.max(1))
    }
}

impl MeasureText for EstimateTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let (width, line_height) = Self::estimate(content, style);
        // Pack width in the low 20 bits, line height above it. Both are small
        // (display is 1280px); 20 bits (≈1M) is ample headroom.
        PreparedTextHandle(((line_height as usize) << 20) | (width as usize & 0xF_FFFF))
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
        let lines = if matches!(max_lines, Some(0)) { Vec::new() } else { vec![line] };
        LineLayout { lines, natural_width }
    }
}

/// Lay out the desktop shell scene at the given available width, returning the
/// positioned + styled `LayoutBox`es.
pub fn compute_desktop_layout(available_width: u32) -> Result<LayoutResult, &'static str> {
    let scene = build_desktop_scene(&BaseTokens);
    LayoutEngine::new()
        .layout(&scene, FxPx::new(available_width as i32), &EstimateTextMeasure)
        .map_err(|_| "desktop layout failed")
}

/// Lay out the desktop shell scene at the default live display width.
pub fn compute_desktop_layout_for_display() -> Result<LayoutResult, &'static str> {
    compute_desktop_layout(DESKTOP_LAYOUT_WIDTH)
}

/// Build the live desktop-shell layout set for the compositor's `proof_layouts`
/// slot: a single `LayoutResult` (the desktop scene has no filter variants).
/// `content_width` is the width available to the scene at its on-screen origin
/// (display width minus the scene inset on both sides). Returns `None` only if
/// layout fails.
pub fn build_live_desktop_layouts(content_width: u32) -> Option<Vec<LayoutResult>> {
    Some(vec![compute_desktop_layout(content_width).ok()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_with_id<'a>(result: &'a LayoutResult, id: &str) -> Option<&'a nexus_layout::LayoutBox> {
        result.boxes.iter().find(|b| b.id == Some(id))
    }

    #[test]
    fn desktop_layout_places_root_topbar_and_window() {
        let result = compute_desktop_layout(DESKTOP_LAYOUT_WIDTH).expect("layout ok");
        let root = box_with_id(&result, "desktop_root").expect("root present");
        // Root fills the available width and carries a themed background.
        assert!(root.rect.width > FxPx::ZERO, "root has width");
        assert!(root.visual.background.is_some(), "root themed bg");

        let topbar = box_with_id(&result, "topbar").expect("topbar present");
        let window = box_with_id(&result, "chat_window").expect("chat window present");
        // Column layout: the window sits below the top bar.
        assert!(
            window.rect.y >= topbar.rect.y + topbar.rect.height,
            "chat window stacks under the top bar (topbar.y+h={:?}, window.y={:?})",
            topbar.rect.y + topbar.rect.height,
            window.rect.y,
        );
    }

    #[test]
    fn desktop_layout_contains_interactive_regions() {
        let result = compute_desktop_layout(DESKTOP_LAYOUT_WIDTH).expect("layout ok");
        // Visible controls must have non-empty rects.
        for id in ["menu_btn", "search", "chat_btn", "chat_titlebar", "chat_close"] {
            let b = box_with_id(&result, id).unwrap_or_else(|| panic!("{id} present"));
            assert!(b.rect.width > FxPx::ZERO, "{id} has a non-empty rect");
        }
        // The chat viewport region must exist; it collapses to zero extent until
        // the VirtualList fills it at runtime, so we only assert presence.
        assert!(box_with_id(&result, "chat_viewport").is_some(), "chat_viewport region present");
    }

    #[test]
    fn boxes_stay_within_available_width() {
        let width = DESKTOP_LAYOUT_WIDTH;
        let result = compute_desktop_layout(width).expect("layout ok");
        for b in &result.boxes {
            assert!(
                b.rect.x + b.rect.width <= FxPx::new(width as i32),
                "box {:?} overflows available width",
                b.id,
            );
        }
    }

    #[test]
    fn estimate_measure_roundtrips_width_and_height() {
        let m = EstimateTextMeasure;
        let style = TextStyle {
            font_size: FxPx::new(16),
            line_height: LineHeight::Absolute(FxPx::new(20)),
            ..TextStyle::default()
        };
        let handle = m.prepare(&TextContent::new("chat"), &style);
        // 4 chars * 16px * 0.55 = 35px.
        assert_eq!(m.measure_width(&handle), FxPx::new(35));
        let lines = m.layout_lines(&handle, FxPx::new(1000), Some(1));
        assert_eq!(lines.lines.len(), 1);
        assert_eq!(lines.lines[0].height, FxPx::new(20));
    }
}
