// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `nexus-scene-raster` — the ONE CPU scene painter for laid-out design-system
//! scenes: `LayoutBox` lists → BGRA pixels, row by row.
//!
//! PROMOTED from the goldens-harness painter (`tests/ui_v10_goldens`) per the
//! promote-best rule: the harness proved this exact fill model against the
//! committed goldens (rounded-rect corner test, even-odd polygon fill for
//! `ShapeKind::{Circle,Triangle*,Path,Vector}`, per-edge borders, src-over
//! blending so translucent glass reads over the base). The harness now calls
//! THIS crate, and the `app-host` DSL renderer streams its surface rows
//! through it — on-device pixels match the goldens by construction.
//!
//! ROW MODEL: callers stream one scanline at a time (`paint_row`) — the
//! app-host renders banded into its surface VMO without a full-frame buffer.
//! Backdrop blur and drop shadows are NOT this painter's job (compositor/GPU
//! features — `nexus-gfx` `LayerBackdrop`/shadow on the live path); text
//! glyphs blend in the separate baked-text pass.

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout::LayoutBox;
use nexus_layout_types::{PathShape, Rgba8, ShapeKind};

/// Unit circle as a 32-gon (precomputed — `no_std` core has no trig).
const CIRCLE_32: [(f32, f32); 32] = [
    (1.0, 0.0),
    (0.980785, 0.19509),
    (0.92388, 0.382683),
    (0.83147, 0.55557),
    (0.707107, 0.707107),
    (0.55557, 0.83147),
    (0.382683, 0.92388),
    (0.19509, 0.980785),
    (0.0, 1.0),
    (-0.19509, 0.980785),
    (-0.382683, 0.92388),
    (-0.55557, 0.83147),
    (-0.707107, 0.707107),
    (-0.83147, 0.55557),
    (-0.92388, 0.382683),
    (-0.980785, 0.19509),
    (-1.0, 0.0),
    (-0.980785, -0.19509),
    (-0.92388, -0.382683),
    (-0.83147, -0.55557),
    (-0.707107, -0.707107),
    (-0.55557, -0.83147),
    (-0.382683, -0.92388),
    (-0.19509, -0.980785),
    (0.0, -1.0),
    (0.19509, -0.980785),
    (0.382683, -0.92388),
    (0.55557, -0.83147),
    (0.707107, -0.707107),
    (0.83147, -0.55557),
    (0.92388, -0.382683),
    (0.980785, -0.19509),
];

/// One BGRA scanline of the target surface.
pub struct RowCanvas<'a> {
    /// The row's pixel bytes (BGRA, tight — `width * 4`).
    pub buf: &'a mut [u8],
    /// The row's y in surface coordinates.
    pub y: i32,
    /// Surface width in px (clip bound).
    pub width: i32,
}

impl RowCanvas<'_> {
    /// Src-over blend one pixel of this row.
    #[inline]
    pub fn blend(&mut self, x: i32, c: Rgba8) {
        if x < 0 || x >= self.width || c.a == 0 {
            return;
        }
        let i = (x * 4) as usize;
        if i + 4 > self.buf.len() {
            return;
        }
        let (a, inv) = (c.a as u32, 255 - c.a as u32);
        let mix = |dst: u8, src: u8| ((dst as u32 * inv + src as u32 * a) / 255) as u8;
        self.buf[i] = mix(self.buf[i], c.b);
        self.buf[i + 1] = mix(self.buf[i + 1], c.g);
        self.buf[i + 2] = mix(self.buf[i + 2], c.r);
        self.buf[i + 3] = (a + self.buf[i + 3] as u32 * inv / 255) as u8;
    }

    /// This row's slice of an (optionally rounded) rect fill.
    fn fill_round_rect_row(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, c: Rgba8) {
        if w <= 0 || h <= 0 || self.y < y || self.y >= y + h {
            return;
        }
        let r = radius.max(0).min(w / 2).min(h / 2);
        let yy = self.y;
        for xx in x..x + w {
            if r > 0 {
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
            self.blend(xx, c);
        }
    }

    /// This row's slice of an even-odd polygon fill.
    fn fill_polygon_row(&mut self, pts: &[(f32, f32)], c: Rgba8) {
        if pts.len() < 3 {
            return;
        }
        let cy = self.y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();
        for i in 0..pts.len() {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % pts.len()];
            if (ay <= cy && by > cy) || (by <= cy && ay > cy) {
                xs.push(ax + (cy - ay) / (by - ay) * (bx - ax));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let mut k = 0;
        while k + 1 < xs.len() {
            let x0 = ceil_i32(xs[k]);
            let x1 = floor_i32(xs[k + 1]);
            for xx in x0..=x1 {
                self.blend(xx, c);
            }
            k += 2;
        }
    }
}

/// `no_std` floor/ceil (core `f32` has neither; trunc-and-adjust is exact for
/// the pixel-coordinate range this painter works in).
#[inline]
fn floor_i32(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) > v { t - 1 } else { t }
}

#[inline]
fn ceil_i32(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) < v { t + 1 } else { t }
}

/// Map a shape to polygon points in a box (`None` = a plain rounded rect).
fn shape_polygon(shape: &ShapeKind, x: i32, y: i32, w: i32, h: i32) -> Option<Vec<(f32, f32)>> {
    let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
    match shape {
        ShapeKind::Rect => None,
        ShapeKind::TriangleUp => {
            Some(alloc::vec![(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)])
        }
        ShapeKind::TriangleDown => {
            Some(alloc::vec![(xf, yf), (xf + wf, yf), (xf + wf / 2.0, yf + hf)])
        }
        ShapeKind::Circle => {
            let (cx, cy, rx, ry) = (xf + wf / 2.0, yf + hf / 2.0, wf / 2.0, hf / 2.0);
            Some(CIRCLE_32.iter().map(|&(c, s)| (cx + rx * c, cy + ry * s)).collect())
        }
        ShapeKind::Path(ps) => Some(contour_points(ps, xf, yf, wf, hf)),
        // Multi-contour is filled per-contour in `paint_box_row`.
        ShapeKind::Vector(_) => None,
    }
}

/// Map a normalized `0..1000` contour into a box.
fn contour_points(ps: &PathShape, xf: f32, yf: f32, wf: f32, hf: f32) -> Vec<(f32, f32)> {
    ps.points
        .iter()
        .map(|p| (xf + p.x_milli as f32 / 1000.0 * wf, yf + p.y_milli as f32 / 1000.0 * hf))
        .collect()
}

/// Paint one box's contribution to this row (fill + borders).
pub fn paint_box_row(canvas: &mut RowCanvas<'_>, b: &LayoutBox) {
    let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    if w <= 0 || h <= 0 || canvas.y < y || canvas.y >= y + h {
        return;
    }
    if let Some(bg) = b.visual.background {
        if let ShapeKind::Vector(contours) = &b.visual.shape {
            for ps in contours {
                let poly = contour_points(ps, x as f32, y as f32, w as f32, h as f32);
                canvas.fill_polygon_row(&poly, bg);
            }
        } else {
            match shape_polygon(&b.visual.shape, x, y, w, h) {
                Some(poly) => canvas.fill_polygon_row(&poly, bg),
                None => {
                    let radius = b.visual.corner_radius.top_left.0.max(0);
                    canvas.fill_round_rect_row(x, y, w, h, radius, bg);
                }
            }
        }
    }
    paint_borders_row(canvas, x, y, w, h, &b.visual.border);
}

/// Paint every box's contribution to this row, in box (z) order.
pub fn paint_row(canvas: &mut RowCanvas<'_>, boxes: &[LayoutBox]) {
    for b in boxes {
        paint_box_row(canvas, b);
    }
}

fn paint_borders_row(
    canvas: &mut RowCanvas<'_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    border: &nexus_layout_types::EdgeBorder,
) {
    if let Some(t) = border.top {
        canvas.fill_round_rect_row(x, y, w, t.width.0.max(0), 0, t.color);
    }
    if let Some(b) = border.bottom {
        let bw = b.width.0.max(0);
        canvas.fill_round_rect_row(x, y + h - bw, w, bw, 0, b.color);
    }
    if let Some(l) = border.left {
        canvas.fill_round_rect_row(x, y, l.width.0.max(0), h, 0, l.color);
    }
    if let Some(r) = border.right {
        let rw = r.width.0.max(0);
        canvas.fill_round_rect_row(x + w - rw, y, rw, h, 0, r.color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{CornerRadius, FxPx, Overflow, Rect, VisualStyle};

    fn boxed(x: i32, y: i32, w: i32, h: i32, radius: i32, color: Rgba8) -> LayoutBox {
        LayoutBox {
            node_id: 1,
            id: None,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            z_index: 0,
            visual: VisualStyle {
                background: Some(color),
                corner_radius: CornerRadius::uniform(FxPx::new(radius)),
                ..VisualStyle::default()
            },
            clip_rect: None,
            scroll_offset: (FxPx::ZERO, FxPx::ZERO),
            overflow: Overflow::Visible,
        }
    }

    #[test]
    fn rounded_corners_clip_the_corner_pixels() {
        let b = boxed(0, 0, 20, 20, 8, Rgba8 { r: 255, g: 0, b: 0, a: 255 });
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas { buf: &mut row, y: 0, width: 20 };
        paint_box_row(&mut canvas, &b);
        assert_eq!(row[0], 0, "corner pixel stays empty");
        assert_ne!(row[10 * 4 + 2], 0, "centre pixel painted red");
    }

    #[test]
    fn circle_row_is_narrower_near_the_pole() {
        let mut b = boxed(0, 0, 32, 32, 0, Rgba8 { r: 0, g: 255, b: 0, a: 255 });
        b.visual.shape = ShapeKind::Circle;
        let painted = |y: i32| {
            let mut row = [0u8; 32 * 4];
            let mut canvas = RowCanvas { buf: &mut row, y, width: 32 };
            paint_box_row(&mut canvas, &b);
            row.chunks_exact(4).filter(|px| px[1] != 0).count()
        };
        assert!(painted(2) < painted(16), "pole rows narrower than the equator");
    }

    #[test]
    fn src_over_blends_translucent_glass() {
        let b = boxed(0, 0, 4, 4, 0, Rgba8 { r: 255, g: 255, b: 255, a: 128 });
        let mut row = [0x40u8; 4 * 4];
        for px in row.chunks_exact_mut(4) {
            px[3] = 0xff;
        }
        let mut canvas = RowCanvas { buf: &mut row, y: 1, width: 4 };
        paint_box_row(&mut canvas, &b);
        assert!(row[0] > 0x40 && row[0] < 0xff, "50% white over grey = mid blend");
    }
}
