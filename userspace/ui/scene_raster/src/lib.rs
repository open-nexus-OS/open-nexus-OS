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
    /// Horizontal paint shift (scroll): a pixel painted at model-x lands at
    /// `x - shift_x` on the surface. 0 = identity (the common case).
    pub shift_x: i32,
    /// Horizontal scissor on the SURFACE (x0, x1 exclusive) — the scroll
    /// viewport's columns. `None` = full row.
    pub clip_x: Option<(i32, i32)>,
}

impl RowCanvas<'_> {
    /// A plain unscrolled row canvas.
    #[must_use]
    pub fn new(buf: &mut [u8], y: i32, width: i32) -> RowCanvas<'_> {
        RowCanvas { buf, y, width, shift_x: 0, clip_x: None }
    }

    /// Src-over blend one pixel of this row (model-x; scroll shift + viewport
    /// scissor applied here so every shape/text painter inherits them).
    #[inline]
    pub fn blend(&mut self, x: i32, c: Rgba8) {
        let x = x - self.shift_x;
        if let Some((x0, x1)) = self.clip_x {
            if x < x0 || x >= x1 {
                return;
            }
        }
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

    /// True when `(xx, yy)` lies inside the rounded rect (corner-circle test).
    fn inside_round_rect(xx: i32, yy: i32, x: i32, y: i32, w: i32, h: i32, radius: i32) -> bool {
        if xx < x || yy < y || xx >= x + w || yy >= y + h {
            return false;
        }
        let r = radius.max(0).min(w / 2).min(h / 2);
        if r == 0 {
            return true;
        }
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
        dx * dx + dy * dy <= (r as i64) * (r as i64)
    }

    /// This row's slice of a rounded BORDER ring: pixels inside the outer
    /// rounded rect but outside the (border-width-inset) inner one.
    fn stroke_round_rect_row(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        width: i32,
        c: Rgba8,
    ) {
        if w <= 0 || h <= 0 || self.y < y || self.y >= y + h {
            return;
        }
        let yy = self.y;
        let inner_r = (radius - width).max(0);
        for xx in x..x + w {
            if Self::inside_round_rect(xx, yy, x, y, w, h, radius)
                && !Self::inside_round_rect(
                    xx,
                    yy,
                    x + width,
                    y + width,
                    w - 2 * width,
                    h - 2 * width,
                    inner_r,
                )
            {
                self.blend(xx, c);
            }
        }
    }

    /// This row's slice of an even-odd polygon fill. Points are produced by
    /// `pt(i)` for `i in 0..n` — the painter NEVER materializes a point list:
    /// this runs per box per row on services with a non-freeing bump heap
    /// (app-host hover repaints page-faulted at the heap end when every icon
    /// contour allocated a `Vec` per row). Crossings land in a fixed array;
    /// a 2D scanline crossing more than `MAX_ROW_CROSSINGS` edges of one
    /// contour does not occur for the flattened icon/shape contours this
    /// paints (quads, 32-gons, sampled curves).
    fn fill_polygon_row(&mut self, n: usize, pt: impl Fn(usize) -> (f32, f32), c: Rgba8) {
        const MAX_ROW_CROSSINGS: usize = 64;
        if n < 3 {
            return;
        }
        let cy = self.y as f32 + 0.5;
        let mut xs = [0f32; MAX_ROW_CROSSINGS];
        let mut m = 0usize;
        for i in 0..n {
            let (ax, ay) = pt(i);
            let (bx, by) = pt((i + 1) % n);
            if ((ay <= cy && by > cy) || (by <= cy && ay > cy)) && m < MAX_ROW_CROSSINGS {
                xs[m] = ax + (cy - ay) / (by - ay) * (bx - ax);
                m += 1;
            }
        }
        let xs = &mut xs[..m];
        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
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

    /// One normalized `0..1000` contour mapped into a box, filled for this row
    /// (the `ShapeKind::{Path,Vector}` fill) — point mapping is inline, no
    /// intermediate list.
    fn fill_contour_row(&mut self, ps: &PathShape, xf: f32, yf: f32, wf: f32, hf: f32, c: Rgba8) {
        let pts = &ps.points;
        self.fill_polygon_row(
            pts.len(),
            |i| {
                let p = &pts[i];
                (xf + p.x_milli as f32 / 1000.0 * wf, yf + p.y_milli as f32 / 1000.0 * hf)
            },
            c,
        );
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

/// Paint one box's contribution to this row (fill + borders).
pub fn paint_box_row(canvas: &mut RowCanvas<'_>, b: &LayoutBox) {
    let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    if w <= 0 || h <= 0 || canvas.y < y || canvas.y >= y + h {
        return;
    }
    if let Some(bg) = b.visual.background {
        let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
        match &b.visual.shape {
            ShapeKind::Rect => {
                let radius = b.visual.corner_radius.top_left.0.max(0);
                canvas.fill_round_rect_row(x, y, w, h, radius, bg);
            }
            ShapeKind::TriangleUp => {
                let pts = [(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)];
                canvas.fill_polygon_row(3, |i| pts[i], bg);
            }
            ShapeKind::TriangleDown => {
                let pts = [(xf, yf), (xf + wf, yf), (xf + wf / 2.0, yf + hf)];
                canvas.fill_polygon_row(3, |i| pts[i], bg);
            }
            ShapeKind::Circle => {
                let (cx, cy, rx, ry) = (xf + wf / 2.0, yf + hf / 2.0, wf / 2.0, hf / 2.0);
                canvas.fill_polygon_row(
                    CIRCLE_32.len(),
                    |i| {
                        let (c, s) = CIRCLE_32[i];
                        (cx + rx * c, cy + ry * s)
                    },
                    bg,
                );
            }
            ShapeKind::Path(ps) => canvas.fill_contour_row(ps, xf, yf, wf, hf, bg),
            ShapeKind::Vector(contours) => {
                for ps in contours {
                    canvas.fill_contour_row(ps, xf, yf, wf, hf, bg);
                }
            }
        }
    }
    paint_borders_row(canvas, x, y, w, h, b.visual.corner_radius.top_left.0.max(0), &b.visual.border);
}

/// Paint every box's contribution to this row, in box (z) order.
pub fn paint_row(canvas: &mut RowCanvas<'_>, boxes: &[LayoutBox]) {
    paint_row_hover(canvas, boxes, None);
}

/// A paint-time hover wash: blended over the box whose `node_id` matches,
/// following its corner radius. `color.a` carries the wash alpha (the
/// `nexus_style::InteractionState` convention). Presentation-only — layout
/// and the box list stay untouched (pretext: hover costs one repaint, never
/// a re-layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoverWash {
    pub node_id: usize,
    pub color: Rgba8,
}

/// Paint-time scroll transform for the page's scroll viewport (pretext:
/// scrolling is a REPAINT with an offset over the RETAINED boxes — never a
/// re-layout, never a per-event allocation). Boxes carrying a `clip_rect`
/// (the engine stamps it on every descendant of an `Overflow::Hidden`
/// container) render shifted by `(dx, dy)` and scissored to the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollView {
    /// Viewport rect on the surface: x0, y0, x1, y1 (exclusive).
    pub clip: (i32, i32, i32, i32),
    /// Content shift right→left (horizontal scroll offset).
    pub dx: i32,
    /// Content shift down→up (vertical scroll offset).
    pub dy: i32,
}

/// [`paint_row`] plus an optional hover wash over the hovered box. The wash
/// paints directly after its box (before later siblings/children), so nested
/// content still reads on top of the wash like a real material highlight.
pub fn paint_row_hover(canvas: &mut RowCanvas<'_>, boxes: &[LayoutBox], hover: Option<HoverWash>) {
    paint_row_scrolled(canvas, boxes, hover, None);
}

/// [`paint_row_hover`] with the scroll transform. `canvas.y` is the SURFACE
/// row; clipped boxes are sampled at model row `canvas.y + dy` and shifted
/// left by `dx`, unclipped boxes paint identity — one pass, alloc-free.
pub fn paint_row_scrolled(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
) {
    let surface_y = canvas.y;
    for b in boxes {
        paint_one_scrolled(canvas, b, hover, scroll, surface_y);
    }
}

/// [`paint_row_scrolled`] over a PRE-FILTERED index list (`pick` = indices
/// into `boxes` that intersect the repaint span). The caller computes the
/// visibility set ONCE per repaint — the per-row cost is then proportional
/// to what is on screen, not to the page's total box count (the 1000-message
/// transcript contract). Alloc-free.
pub fn paint_row_picked(
    canvas: &mut RowCanvas<'_>,
    boxes: &[LayoutBox],
    pick: &[u32],
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
) {
    let surface_y = canvas.y;
    for &i in pick {
        let Some(b) = boxes.get(i as usize) else { continue };
        paint_one_scrolled(canvas, b, hover, scroll, surface_y);
    }
}

#[inline]
fn paint_one_scrolled(
    canvas: &mut RowCanvas<'_>,
    b: &LayoutBox,
    hover: Option<HoverWash>,
    scroll: Option<ScrollView>,
    surface_y: i32,
) {
    {
        let scrolled = match (scroll, b.clip_rect) {
            (Some(sv), Some(_)) => {
                // Inside the viewport: visible only on the viewport's rows.
                if surface_y < sv.clip.1 || surface_y >= sv.clip.3 {
                    return;
                }
                canvas.y = surface_y + sv.dy;
                canvas.shift_x = sv.dx;
                canvas.clip_x = Some((sv.clip.0, sv.clip.2));
                true
            }
            _ => false,
        };
        paint_box_row(canvas, b);
        if let Some(hw) = hover {
            if b.node_id == hw.node_id && hw.color.a > 0 {
                let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                if w > 0 && h > 0 && canvas.y >= y && canvas.y < y + h {
                    let radius = b.visual.corner_radius.top_left.0.max(0);
                    canvas.fill_round_rect_row(x, y, w, h, radius, hw.color);
                }
            }
        }
        if scrolled {
            canvas.y = surface_y;
            canvas.shift_x = 0;
            canvas.clip_x = None;
        }
    }
}


fn paint_borders_row(
    canvas: &mut RowCanvas<'_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    border: &nexus_layout_types::EdgeBorder,
) {
    // Uniform border (the kit's `Style::border` sets all four edges the same):
    // stroke a ring that FOLLOWS the corner radius — four straight strips on a
    // rounded fill read as a square frame around a round element.
    if let (Some(t), Some(bo), Some(l), Some(r)) =
        (border.top, border.bottom, border.left, border.right)
    {
        let uniform = t.width == bo.width
            && t.width == l.width
            && t.width == r.width
            && t.color == bo.color
            && t.color == l.color
            && t.color == r.color;
        if uniform {
            canvas.stroke_round_rect_row(x, y, w, h, radius, t.width.0.max(1), t.color);
            return;
        }
    }
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
    fn hover_wash_tints_only_the_hovered_box() {
        // Two side-by-side boxes; the wash lands on node 2 only, inside its
        // rounded outline (the corner pixel stays untouched).
        let grey = Rgba8 { r: 100, g: 100, b: 100, a: 255 };
        let a = boxed(0, 0, 10, 10, 0, grey);
        let mut b = boxed(10, 0, 10, 10, 4, grey);
        b.node_id = 2;
        let wash = HoverWash { node_id: 2, color: Rgba8 { r: 255, g: 255, b: 255, a: 64 } };
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 0, 20);
        paint_row_hover(&mut canvas, &[a, b], Some(wash));
        assert_eq!(row[5 * 4 + 2], 100, "unhovered box keeps its base color");
        assert!(row[15 * 4 + 2] > 100, "hovered box is washed brighter");
        assert_eq!(row[10 * 4 + 2], 0, "wash follows the hovered box's corner radius");
    }

    #[test]
    fn rounded_corners_clip_the_corner_pixels() {
        let b = boxed(0, 0, 20, 20, 8, Rgba8 { r: 255, g: 0, b: 0, a: 255 });
        let mut row = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut row, 0, 20);
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
            let mut canvas = RowCanvas::new(&mut row, y, 32);
            paint_box_row(&mut canvas, &b);
            row.chunks_exact(4).filter(|px| px[1] != 0).count()
        };
        assert!(painted(2) < painted(16), "pole rows narrower than the equator");
    }

    #[test]
    fn uniform_border_ring_follows_the_corner_radius() {
        use nexus_layout_types::{EdgeBorder, FxPx};
        let mut b = boxed(0, 0, 24, 24, 8, Rgba8 { r: 10, g: 10, b: 10, a: 255 });
        b.visual.border = EdgeBorder::all(FxPx::new(2), Rgba8 { r: 0, g: 0, b: 255, a: 255 });
        // Top row (y=0): the ring must NOT paint the extreme corner pixel
        // (a square frame would) but MUST paint near the rounded arc.
        let mut row = [0u8; 24 * 4];
        let mut canvas = RowCanvas::new(&mut row, 0, 24);
        paint_box_row(&mut canvas, &b);
        assert_eq!(row[0], 0, "corner pixel outside the rounded ring stays empty");
        assert_eq!(row[12 * 4], 255, "top-centre pixel is border blue");
        // Mid row: ring = left/right edges only; the centre is fill, not border.
        let mut mid = [0u8; 24 * 4];
        let mut canvas = RowCanvas::new(&mut mid, 12, 24);
        paint_box_row(&mut canvas, &b);
        assert_eq!(mid[0], 255, "left edge is border blue");
        assert_ne!(mid[12 * 4], 255, "centre is the fill, not the border");
    }

    #[test]
    fn src_over_blends_translucent_glass() {
        let b = boxed(0, 0, 4, 4, 0, Rgba8 { r: 255, g: 255, b: 255, a: 128 });
        let mut row = [0x40u8; 4 * 4];
        for px in row.chunks_exact_mut(4) {
            px[3] = 0xff;
        }
        let mut canvas = RowCanvas::new(&mut row, 1, 4);
        paint_box_row(&mut canvas, &b);
        assert!(row[0] > 0x40 && row[0] < 0xff, "50% white over grey = mid blend");
    }
}
