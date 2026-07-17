// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host golden + a11y harness for the design-system primitives (TASK-0073 DoD).
//!
//! Renders a component's `LayoutNode` through the real `LayoutEngine` and a small
//! `LayoutResult` painter into a raw BGRA buffer, then compares the bytes against
//! a committed hex golden (via `ui_host_snap`). Regenerate goldens with
//! `UPDATE_GOLDENS=1`. Also provides WCAG contrast + touch-target lints.
//!
//! The painter is structural but shape-aware: rounded-rect fills, square borders,
//! and **polygon fills** for `ShapeKind::{Circle,Triangle*,Path}` (so vector
//! symbols/chevrons render), src-over blended so translucent glass reads over the
//! canvas. Text and backdrop blur are not part of the fill golden.

use std::path::{Path, PathBuf};

use nexus_layout::{LayoutEngine, LayoutResult};
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, LineLayout, MeasureText,
    Overflow, PreparedTextHandle, Rgba8, Stack, TextContent, TextStyle, VisualStyle,
};
use ui_host_snap::{compare_hex_golden, hex_bytes, GoldenMode, SnapResult};

/// Fixture canvas size (px).
pub const CANVAS_W: i32 = 160;
pub const CANVAS_H: i32 = 96;
const CANVAS_FILL: Rgba8 = Rgba8 { r: 0x30, g: 0x30, b: 0x30, a: 0xff };

/// A no-op text measurer — the primitive fixtures carry no measured `Text`.
pub struct NoText;

impl MeasureText for NoText {
    fn prepare(&self, _content: &TextContent, _style: &TextStyle) -> PreparedTextHandle {
        PreparedTextHandle(0)
    }
    fn measure_width(&self, _handle: &PreparedTextHandle) -> FxPx {
        FxPx::ZERO
    }
    fn layout_lines(
        &self,
        _handle: &PreparedTextHandle,
        _width: FxPx,
        _max_lines: Option<u32>,
    ) -> LineLayout {
        LineLayout { lines: Vec::new(), natural_width: FxPx::ZERO }
    }
}

/// Paint a flattened `LayoutResult` into a fresh canvas and return its BGRA bytes.
pub fn render_to_bgra(node: &LayoutNode) -> SnapResult<Vec<u8>> {
    let engine = LayoutEngine::new();
    let result: LayoutResult = engine
        .layout(node, FxPx::new(CANVAS_W), &NoText)
        .map_err(|_| ui_host_snap::SnapshotError::Codec)?;

    // The painter SSOT (`nexus-scene-raster`, promoted FROM this harness):
    // the goldens verify the shared implementation byte-for-byte — the
    // app-host streams the same rows on-device.
    let mut buf = vec![0u8; (CANVAS_W * CANVAS_H * 4) as usize];
    for px in buf.chunks_exact_mut(4) {
        px[0] = CANVAS_FILL.b;
        px[1] = CANVAS_FILL.g;
        px[2] = CANVAS_FILL.r;
        px[3] = CANVAS_FILL.a;
    }
    for y in 0..CANVAS_H {
        let row = &mut buf[(y * CANVAS_W * 4) as usize..((y + 1) * CANVAS_W * 4) as usize];
        let mut canvas = nexus_scene_raster::RowCanvas::new(row, y, CANVAS_W);
        nexus_scene_raster::paint_row(&mut canvas, &result.boxes);
    }
    Ok(buf)
}

/// A fixed `px`×`px` opaque content box, so container primitives lay out to a
/// real size in fixtures and touch-target lints.
pub fn swatch(px: i32) -> LayoutNode {
    let d = Some(FxPx::new(px));
    let visual =
        VisualStyle { background: Some(Rgba8::new(200, 200, 205, 255)), ..VisualStyle::default() };
    LayoutNode::Stack(
        Stack {
            id: Some("swatch"),
            direction: Direction::Row,
            gap: FxPx::ZERO,
            padding: EdgeInsets::zero(),
            align: Align::Center,
            justify: Justify::Center,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem::default(),
        },
        visual,
        Vec::new(),
    )
}

/// This crate's golden directory.
pub fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens")
}

/// Render `node` and compare its BGRA against `goldens/<name>.bgra.hex`
/// (created/updated when `UPDATE_GOLDENS=1`).
pub fn check_golden(name: &str, node: &LayoutNode) -> SnapResult<()> {
    let bgra = render_to_bgra(node)?;
    let actual = hex_bytes(&bgra)?;
    let relative = PathBuf::from(format!("{name}.bgra.hex"));
    compare_hex_golden(&golden_root(), &relative, &actual, GoldenMode::from_env())
}

// ── A11y: WCAG contrast + touch targets ─────────────────────────────────────

/// WCAG relative luminance of an opaque sRGB color (0.0..1.0).
pub fn relative_luminance(c: Rgba8) -> f64 {
    fn lin(ch: u8) -> f64 {
        let s = ch as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colors (1.0..21.0).
pub fn contrast_ratio(a: Rgba8, b: Rgba8) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Minimum interactive touch-target edge (px). Desktop floor; WCAG 2.5.5
/// Enhanced recommends 44 for touch.
pub const MIN_TOUCH: i32 = 24;
/// WCAG 1.4.11 non-text/UI-component contrast threshold.
pub const CONTRAST_UI: f64 = 3.0;
/// WCAG 1.4.3 normal body-text contrast threshold.
pub const CONTRAST_TEXT: f64 = 4.5;

/// The root box size of a laid-out component (its outer tap target).
pub fn root_size(node: &LayoutNode) -> (i32, i32) {
    let engine = LayoutEngine::new();
    let result = engine.layout(node, FxPx::new(CANVAS_W), &NoText).expect("layout");
    result.boxes.first().map(|b| (b.rect.width.0, b.rect.height.0)).unwrap_or((0, 0))
}
