// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host golden + a11y harness for the design-system primitives (TASK-0073 DoD).
//!
//! Renders a component's `LayoutNode` through the real `LayoutEngine` and a small
//! `LayoutResult` painter into a `ui_renderer::Frame`, then compares the BGRA
//! bytes against a committed hex golden (via `ui_host_snap`). Regenerate goldens
//! with `UPDATE_GOLDENS=1`. Also provides WCAG contrast + touch-target lints.
//!
//! The painter is structural: solid fills + rounded corners + square borders
//! (backdrop blur and text are not part of the fill golden — they are validated
//! separately). This locks each component's geometry and resolved colors.

use std::path::{Path, PathBuf};

use nexus_layout::{LayoutEngine, LayoutResult};
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, LineLayout, MeasureText,
    Overflow, PreparedTextHandle, Rgba8, Stack, TextContent, TextStyle, VisualStyle,
};
use ui_host_snap::{compare_hex_golden, hex_bytes, make_damage, GoldenMode, SnapResult};
use ui_renderer::{Damage, Frame, PixelBgra, Rect};

/// Fixture canvas size (px). Large enough for every core primitive at its
/// natural size; the neutral fill makes component pixels stand out in artifacts.
pub const CANVAS_W: u32 = 160;
pub const CANVAS_H: u32 = 96;
const CANVAS_FILL: PixelBgra = PixelBgra::new(0x30, 0x30, 0x30, 0xff);

/// A no-op text measurer — the primitive fixtures carry no `Text` nodes, so the
/// measure hooks are never exercised; this only satisfies the engine's type.
struct NoText;

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

fn to_px(c: Rgba8) -> PixelBgra {
    PixelBgra::from_rgba(c.r, c.g, c.b, c.a)
}

/// Paint a flattened `LayoutResult` into the frame: each box's rounded fill, then
/// its square borders. Boxes are already in paint order.
fn paint(result: &LayoutResult, frame: &mut Frame, damage: &mut Damage) -> SnapResult<()> {
    for b in &result.boxes {
        let (x, y) = (b.rect.x.0, b.rect.y.0);
        let (w, h) = (b.rect.width.0, b.rect.height.0);
        if w <= 0 || h <= 0 {
            continue;
        }
        let (w, h) = (w as u32, h as u32);
        if let Some(bg) = b.visual.background {
            let radius = b.visual.corner_radius.top_left.0.max(0) as u32;
            let rect = Rect::new(x, y, w, h).map_err(|_| ui_host_snap::SnapshotError::Codec)?;
            frame
                .draw_rounded_rect(rect, radius, to_px(bg), damage)
                .map_err(|_| ui_host_snap::SnapshotError::Codec)?;
        }
        paint_borders(frame, x, y, w, h, &b.visual.border, damage)?;
    }
    Ok(())
}

fn paint_borders(
    frame: &mut Frame,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    border: &nexus_layout_types::EdgeBorder,
    damage: &mut Damage,
) -> SnapResult<()> {
    let mut edge = |rx: i32, ry: i32, rw: u32, rh: u32, c: Rgba8| -> SnapResult<()> {
        if rw == 0 || rh == 0 {
            return Ok(());
        }
        let rect = Rect::new(rx, ry, rw, rh).map_err(|_| ui_host_snap::SnapshotError::Codec)?;
        frame.draw_rect(rect, to_px(c), damage).map_err(|_| ui_host_snap::SnapshotError::Codec)
    };
    if let Some(t) = border.top {
        edge(x, y, w, t.width.0.max(0) as u32, t.color)?;
    }
    if let Some(b) = border.bottom {
        let bw = b.width.0.max(0) as u32;
        edge(x, y + h as i32 - bw as i32, w, bw, b.color)?;
    }
    if let Some(l) = border.left {
        edge(x, y, l.width.0.max(0) as u32, h, l.color)?;
    }
    if let Some(r) = border.right {
        let rw = r.width.0.max(0) as u32;
        edge(x + w as i32 - rw as i32, y, rw, h, r.color)?;
    }
    Ok(())
}

/// Lay out a component on the fixture canvas and return the logical BGRA bytes.
pub fn render_to_bgra(node: &LayoutNode) -> SnapResult<Vec<u8>> {
    let mut frame = Frame::new_checked(CANVAS_W, CANVAS_H).map_err(|_| ui_host_snap::SnapshotError::Codec)?;
    // 64 = the renderer's max damage-rect capacity; ample for one small fixture.
    let mut damage = make_damage(&frame, 64).map_err(|_| ui_host_snap::SnapshotError::Codec)?;
    frame.clear(CANVAS_FILL, &mut damage).map_err(|_| ui_host_snap::SnapshotError::Codec)?;

    let engine = LayoutEngine::new();
    let result = engine
        .layout(node, FxPx::new(CANVAS_W as i32), &NoText)
        .map_err(|_| ui_host_snap::SnapshotError::Codec)?;
    paint(&result, &mut frame, &mut damage)?;

    frame.logical_bgra_bytes().map_err(|_| ui_host_snap::SnapshotError::Codec)
}

/// A fixed `px`×`px` opaque content box, so container primitives lay out to a
/// real (width *and* height) size in fixtures and touch-target lints.
pub fn swatch(px: i32) -> LayoutNode {
    let d = Some(FxPx::new(px));
    let visual = VisualStyle { background: Some(Rgba8::new(200, 200, 205, 255)), ..VisualStyle::default() };
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

/// This crate's golden directory (not `ui_host_snap`'s).
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
/// Enhanced recommends 44 for touch — components should meet this where they are
/// the tap target.
pub const MIN_TOUCH: i32 = 24;

/// WCAG 1.4.11 non-text/UI-component contrast threshold.
pub const CONTRAST_UI: f64 = 3.0;
/// WCAG 1.4.3 normal body-text contrast threshold.
pub const CONTRAST_TEXT: f64 = 4.5;

/// The root box size of a laid-out component (its outer tap target).
pub fn root_size(node: &LayoutNode) -> (i32, i32) {
    let engine = LayoutEngine::new();
    let result = engine.layout(node, FxPx::new(CANVAS_W as i32), &NoText).expect("layout");
    result
        .boxes
        .first()
        .map(|b| (b.rect.width.0, b.rect.height.0))
        .unwrap_or((0, 0))
}
