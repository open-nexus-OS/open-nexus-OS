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
    Overflow, PreparedTextHandle, Rgba8, ShapeKind, Stack, TextContent, TextStyle, VisualStyle,
};
use ui_host_snap::{compare_hex_golden, hex_bytes, GoldenMode, SnapResult};

/// Fixture canvas size (px).
pub const CANVAS_W: i32 = 160;
pub const CANVAS_H: i32 = 96;
const CANVAS_FILL: Rgba8 = Rgba8 { r: 0x30, g: 0x30, b: 0x30, a: 0xff };

/// A no-op text measurer — the primitive fixtures carry no measured `Text`.
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

/// A BGRA canvas the painter writes into (tight `W*H*4`, no stride padding).
struct Canvas {
    buf: Vec<u8>,
}

impl Canvas {
    fn new(fill: Rgba8) -> Self {
        let mut buf = vec![0u8; (CANVAS_W * CANVAS_H * 4) as usize];
        for px in buf.chunks_exact_mut(4) {
            px[0] = fill.b;
            px[1] = fill.g;
            px[2] = fill.r;
            px[3] = fill.a;
        }
        Self { buf }
    }

    /// Src-over blend one pixel.
    fn blend(&mut self, x: i32, y: i32, c: Rgba8) {
        if x < 0 || y < 0 || x >= CANVAS_W || y >= CANVAS_H || c.a == 0 {
            return;
        }
        let i = ((y * CANVAS_W + x) * 4) as usize;
        let (a, inv) = (c.a as u32, 255 - c.a as u32);
        let mix = |dst: u8, src: u8| ((dst as u32 * inv + src as u32 * a) / 255) as u8;
        self.buf[i] = mix(self.buf[i], c.b);
        self.buf[i + 1] = mix(self.buf[i + 1], c.g);
        self.buf[i + 2] = mix(self.buf[i + 2], c.r);
        self.buf[i + 3] = (a + self.buf[i + 3] as u32 * inv / 255) as u8;
    }

    /// Fill an (optionally rounded) rectangle.
    fn fill_round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, c: Rgba8) {
        if w <= 0 || h <= 0 {
            return;
        }
        let r = radius.max(0).min(w / 2).min(h / 2);
        for yy in y..y + h {
            for xx in x..x + w {
                if r > 0 {
                    // Corner test: distance from the nearest corner centre.
                    let cx = if xx < x + r {
                        x + r
                    } else if xx >= x + w - r {
                        x + w - r - 1
                    } else {
                        xx
                    };
                    let cy = if yy < y + r {
                        y + r
                    } else if yy >= y + h - r {
                        y + h - r - 1
                    } else {
                        yy
                    };
                    let (dx, dy) = ((xx - cx) as i64, (yy - cy) as i64);
                    if dx * dx + dy * dy > (r as i64) * (r as i64) {
                        continue;
                    }
                }
                self.blend(xx, yy, c);
            }
        }
    }

    /// Even-odd scanline polygon fill.
    fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Rgba8) {
        if pts.len() < 3 {
            return;
        }
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
        for &(_, py) in pts {
            min_y = min_y.min(py);
            max_y = max_y.max(py);
        }
        let y0 = (min_y.floor() as i32).max(0);
        let y1 = (max_y.ceil() as i32).min(CANVAS_H);
        for yy in y0..y1 {
            let cy = yy as f32 + 0.5;
            let mut xs: Vec<f32> = Vec::new();
            for i in 0..pts.len() {
                let (ax, ay) = pts[i];
                let (bx, by) = pts[(i + 1) % pts.len()];
                if (ay <= cy && by > cy) || (by <= cy && ay > cy) {
                    xs.push(ax + (cy - ay) / (by - ay) * (bx - ax));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut k = 0;
            while k + 1 < xs.len() {
                let x0 = xs[k].ceil() as i32;
                let x1 = xs[k + 1].floor() as i32;
                for xx in x0..=x1 {
                    self.blend(xx, yy, c);
                }
                k += 2;
            }
        }
    }
}

/// Map a shape to polygon points in a box (`None` = a plain rounded rect).
fn shape_polygon(shape: &ShapeKind, x: i32, y: i32, w: i32, h: i32) -> Option<Vec<(f32, f32)>> {
    let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
    match shape {
        ShapeKind::Rect => None,
        ShapeKind::TriangleUp => {
            Some(vec![(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)])
        }
        ShapeKind::TriangleDown => {
            Some(vec![(xf, yf), (xf + wf, yf), (xf + wf / 2.0, yf + hf)])
        }
        ShapeKind::Circle => {
            let (cx, cy, rx, ry) = (xf + wf / 2.0, yf + hf / 2.0, wf / 2.0, hf / 2.0);
            Some(
                (0..32)
                    .map(|i| {
                        let t = i as f32 / 32.0 * core::f32::consts::TAU;
                        (cx + rx * t.cos(), cy + ry * t.sin())
                    })
                    .collect(),
            )
        }
        ShapeKind::Path(ps) => Some(
            ps.points
                .iter()
                .map(|p| (xf + p.x_milli as f32 / 1000.0 * wf, yf + p.y_milli as f32 / 1000.0 * hf))
                .collect(),
        ),
    }
}

/// Paint a flattened `LayoutResult` into a fresh canvas and return its BGRA bytes.
pub fn render_to_bgra(node: &LayoutNode) -> SnapResult<Vec<u8>> {
    let engine = LayoutEngine::new();
    let result: LayoutResult = engine
        .layout(node, FxPx::new(CANVAS_W), &NoText)
        .map_err(|_| ui_host_snap::SnapshotError::Codec)?;

    let mut canvas = Canvas::new(CANVAS_FILL);
    for b in &result.boxes {
        let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
        if w <= 0 || h <= 0 {
            continue;
        }
        if let Some(bg) = b.visual.background {
            match shape_polygon(&b.visual.shape, x, y, w, h) {
                Some(poly) => canvas.fill_polygon(&poly, bg),
                None => {
                    let radius = b.visual.corner_radius.top_left.0.max(0);
                    canvas.fill_round_rect(x, y, w, h, radius, bg);
                }
            }
        }
        paint_borders(&mut canvas, x, y, w, h, &b.visual.border);
    }
    Ok(canvas.buf)
}

fn paint_borders(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, border: &nexus_layout_types::EdgeBorder) {
    if let Some(t) = border.top {
        canvas.fill_round_rect(x, y, w, t.width.0.max(0), 0, t.color);
    }
    if let Some(b) = border.bottom {
        let bw = b.width.0.max(0);
        canvas.fill_round_rect(x, y + h - bw, w, bw, 0, b.color);
    }
    if let Some(l) = border.left {
        canvas.fill_round_rect(x, y, l.width.0.max(0), h, 0, l.color);
    }
    if let Some(r) = border.right {
        let rw = r.width.0.max(0);
        canvas.fill_round_rect(x + w - rw, y, rw, h, 0, r.color);
    }
}

/// A fixed `px`×`px` opaque content box, so container primitives lay out to a
/// real size in fixtures and touch-target lints.
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
