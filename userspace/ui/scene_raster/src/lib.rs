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

mod borders;
use borders::{paint_borders_row, paint_inset_highlight_row};

pub mod anim;
pub use anim::NodeAnim;

mod scrolled;
pub use scrolled::{
    paint_row_hover, paint_row_picked, paint_row_picked_animated, paint_row_picked_indexed,
    paint_row_scrolled, ScrollView,
};

use nexus_layout::LayoutBox;
use nexus_layout_types::{PathShape, Rgba8, ShapeKind};

/// Unit circle as a 32-gon (precomputed — `no_std` core has no trig).
// reason: hand-computed lookup table; every entry is an intentional literal at
// matching precision — the 0.707107 entries must stay literal, not the constant.
#[allow(clippy::approx_constant)]
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

    /// REPLACE one pixel of this row (alpha included; same shift/scissor
    /// discipline as [`blend`](Self::blend)) — the glass-region reset write.
    #[inline]
    pub fn set(&mut self, x: i32, c: Rgba8) {
        let x = x - self.shift_x;
        if let Some((x0, x1)) = self.clip_x {
            if x < x0 || x >= x1 {
                return;
            }
        }
        if x < 0 || x >= self.width {
            return;
        }
        let i = (x * 4) as usize;
        if i + 4 > self.buf.len() {
            return;
        }
        // Premultiplied write — the exact pixel `blend` produces onto an
        // empty destination, so a glass reset is indistinguishable from
        // painting the tint on a fresh surface.
        let a = c.a as u32;
        self.buf[i] = (c.b as u32 * a / 255) as u8;
        self.buf[i + 1] = (c.g as u32 * a / 255) as u8;
        self.buf[i + 2] = (c.r as u32 * a / 255) as u8;
        self.buf[i + 3] = c.a;
    }

    /// This row's slice of an (optionally rounded) rect fill.
    pub(crate) fn fill_round_rect_row(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        c: Rgba8,
    ) {
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

    /// This row's slice of an (optionally rounded) rect fill, REPLACING the
    /// destination pixels (alpha included) instead of src-over blending.
    /// Glass-material boxes use this: whatever the surface painted BENEATH a
    /// glass region must not bake through its translucent fill — the
    /// compositor supplies the backdrop (destination-so-far blur), so the
    /// region's surface pixels start from the pure tint.
    pub(crate) fn fill_round_rect_row_replace(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        c: Rgba8,
    ) {
        if w <= 0 || h <= 0 || self.y < y || self.y >= y + h {
            return;
        }
        let yy = self.y;
        for xx in x..x + w {
            if Self::inside_round_rect(xx, yy, x, y, w, h, radius) {
                self.set(xx, c);
            }
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
    // reason: geometry primitive — the args are the box rect, radius, border
    // width and colour; grouping into a struct would not improve clarity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stroke_round_rect_row(
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
    if (t as f32) > v {
        t - 1
    } else {
        t
    }
}

#[inline]
fn ceil_i32(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) < v {
        t + 1
    } else {
        t
    }
}

/// Paint one box's contribution to this row (fill + borders).
pub fn paint_box_row(canvas: &mut RowCanvas<'_>, b: &LayoutBox) {
    let (x, y, w, h) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    paint_box_row_at(canvas, b, x, y, w, h, b.visual.background, 100);
}

/// The row's flat color of a vertical linear gradient: a row-based painter
/// renders `linear-gradient(to bottom, top, bottom)` EXACTLY as one lerped
/// color per row — no banding beyond 8-bit quantization, zero extra passes.
#[inline]
fn gradient_row_color(
    top: nexus_layout_types::Rgba8,
    bottom: nexus_layout_types::Rgba8,
    row: i32,
    y: i32,
    h: i32,
) -> nexus_layout_types::Rgba8 {
    let t_num = (row - y).clamp(0, h.max(1) - 1);
    let t_den = (h.max(1) - 1).max(1);
    // Signed-safe integer lerp per channel.
    let ch = |a: u8, b: u8| -> u8 {
        let ai = a as i32;
        let bi = b as i32;
        (ai + (bi - ai) * t_num / t_den) as u8
    };
    nexus_layout_types::Rgba8 {
        r: ch(top.r, bottom.r),
        g: ch(top.g, bottom.g),
        b: ch(top.b, bottom.b),
        a: ch(top.a, bottom.a),
    }
}

/// [`paint_box_row`] at an EXPLICIT geometry + background: the shared shape
/// dispatch used by the plain path (the box's own rect) and the per-node
/// ANIMATION path (the transformed rect + faded fill) — every `ShapeKind`
/// (rect/triangles/circle/path/vector) scales and translates, so icons and
/// round buttons animate as whole shapes, not just their bounding fill.
/// `radius_pct` scales the corner radius (100 = as authored).
// reason: shared shape-dispatch entry — args are the explicit geometry, fill
// and radius scale; a struct wrapper would not improve this hot path.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_box_row_at(
    canvas: &mut RowCanvas<'_>,
    b: &LayoutBox,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    background: Option<Rgba8>,
    radius_pct: u16,
) {
    if w <= 0 || h <= 0 || canvas.y < y || canvas.y >= y + h {
        return;
    }
    // Soft drop shadow BEHIND the box (design elevation, `.shadow(t)`):
    // painted first so the box fill covers its own footprint. Analytic
    // rounded-rect SDF with a linear falloff over the blur radius — a
    // one-shot cost at (re)render time, never per frame.
    if let Some(shadow) = b.visual.shadow {
        paint_shadow_row(canvas, &shadow, b, x, y, w, h);
    }
    // A vertical gradient wins over the flat fill: substitute this row's
    // lerped color and reuse every shape path unchanged.
    let background = match b.visual.background_gradient {
        Some((top, bottom)) => Some(gradient_row_color(top, bottom, canvas.y, y, h)),
        None => background,
    };
    if let Some(bg) = background {
        let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
        match &b.visual.shape {
            ShapeKind::Rect => {
                let radius = (b.visual.corner_radius.top_left.0.max(0) as i64
                    * radius_pct.max(1) as i64
                    / 100) as i32;
                // A glass ROOT resets its rect to the pure tint (alpha
                // included) instead of src-over: content the surface painted
                // beneath the region must not bake through — the COMPOSITOR
                // supplies the backdrop (destination-so-far blur of
                // everything already composited under the region). NESTED
                // glass (a `subtle` row on a `windowPane` pane) blends
                // src-over instead: it sits on its parent's tint, and only
                // the root's single region asks the compositor for blur.
                // Replacing unconditionally is how every glass stack showed
                // nothing but the wallpaper — each inner rect erased its
                // parent's pixels and the only backdrop left was the
                // window's.
                if matches!(b.visual.material, nexus_layout_types::SurfaceMaterial::Glass(_))
                    && !b.glass_nested
                {
                    let r = radius.max(0).min(w / 2).min(h / 2);
                    canvas.fill_round_rect_row_replace(x, y, w, h, r, bg);
                } else {
                    canvas.fill_round_rect_row(x, y, w, h, radius, bg);
                }
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
            ShapeKind::Raster { w: sw, h: sh, rgba } => {
                // Straight-alpha sprite blit, nearest-sampled onto the box
                // (sprites are baked at the tile sizes, so this is normally
                // a 1:1 row copy). `bg` is ignored — the artwork owns its
                // pixels; the box needs SOME background for this arm to run,
                // the builder sets a transparent one.
                let (sw, sh) = (*sw as i32, *sh as i32);
                if sw > 0 && sh > 0 {
                    let sy = ((canvas.y - y).clamp(0, h - 1) as i64 * sh as i64 / h as i64)
                        .clamp(0, (sh - 1) as i64) as i32;
                    let x0 = x.max(0);
                    let x1 = (x + w).min(canvas.width);
                    for px in x0..x1 {
                        let sx = ((px - x) as i64 * sw as i64 / w as i64).clamp(0, (sw - 1) as i64)
                            as i32;
                        let o = ((sy * sw + sx) * 4) as usize;
                        if o + 3 < rgba.len() && rgba[o + 3] > 0 {
                            canvas.blend(
                                px,
                                nexus_layout_types::Rgba8 {
                                    r: rgba[o],
                                    g: rgba[o + 1],
                                    b: rgba[o + 2],
                                    a: rgba[o + 3],
                                },
                            );
                        }
                    }
                }
            }
            ShapeKind::Path(ps) => canvas.fill_contour_row(ps, xf, yf, wf, hf, bg),
            ShapeKind::Vector(contours) => {
                for ps in contours {
                    canvas.fill_contour_row(ps, xf, yf, wf, hf, bg);
                }
            }
        }
    }
    let radius =
        (b.visual.corner_radius.top_left.0.max(0) as i64 * radius_pct.max(1) as i64 / 100) as i32;
    // The `inset 0 1px 0` top-shine goes UNDER the border ring: the hairline
    // is the outermost pixel, the shine the one just inside it.
    if let Some(shine) = b.visual.inset_highlight {
        paint_inset_highlight_row(canvas, (x, y, w, h), radius, &b.visual.border, shine);
    }
    paint_borders_row(canvas, x, y, w, h, radius, &b.visual.border);
}

/// One row of a soft drop shadow: signed distance to the (offset, spread-
/// adjusted) rounded shadow rect, alpha falls off linearly across the blur
/// band. Row-based like everything else here — no buffers, no passes.
fn paint_shadow_row(
    canvas: &mut RowCanvas<'_>,
    shadow: &nexus_layout_types::BoxShadow,
    b: &LayoutBox,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let blur = shadow.blur_radius.0.max(1) as f32;
    let sx = x + shadow.offset_x.0 - shadow.spread.0;
    let sy = y + shadow.offset_y.0 - shadow.spread.0;
    let sw = w + 2 * shadow.spread.0;
    let sh = h + 2 * shadow.spread.0;
    if sw <= 0 || sh <= 0 {
        return;
    }
    let reach = shadow.blur_radius.0;
    if canvas.y < sy - reach || canvas.y >= sy + sh + reach {
        return;
    }
    let radius = b.visual.corner_radius.top_left.0.max(0).min(sw / 2).min(sh / 2) as f32;
    let (cx, cy) = (sx as f32 + sw as f32 / 2.0, sy as f32 + sh as f32 / 2.0);
    let (hw, hh) = (sw as f32 / 2.0 - radius, sh as f32 / 2.0 - radius);
    let py = canvas.y as f32 + 0.5;
    let x0 = (sx - reach).max(0);
    let x1 = (sx + sw + reach).min(canvas.width);
    let dy = (py - cy).abs() - hh;
    let dyc = if dy > 0.0 { dy } else { 0.0 };
    for px in x0..x1 {
        let pxf = px as f32 + 0.5;
        let dx = (pxf - cx).abs() - hw;
        let dxc = if dx > 0.0 { dx } else { 0.0 };
        // Rounded-rect SDF (outside-only; interior clamps to the max axis).
        let outside = sqrt_f32(dxc * dxc + dyc * dyc);
        let inside = if dx.max(dy) < 0.0 { dx.max(dy) } else { 0.0 };
        let dist = outside + inside - radius;
        // Linear falloff across [-blur/2, +blur/2] around the rect edge.
        let t = 0.5 - dist / blur;
        if t <= 0.0 {
            continue;
        }
        let f = if t >= 1.0 { 1.0 } else { t };
        let a = (shadow.color.a as f32 * f) as u8;
        if a == 0 {
            continue;
        }
        canvas.blend(
            px,
            nexus_layout_types::Rgba8 {
                r: shadow.color.r,
                g: shadow.color.g,
                b: shadow.color.b,
                a,
            },
        );
    }
}

/// `no_std` sqrt via Newton iterations (the painter has no libm; three
/// rounds from a decent seed are exact to well under 8-bit alpha).
#[inline]
fn sqrt_f32(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut r = if v >= 1.0 { v } else { 1.0 };
    r = 0.5 * (r + v / r);
    r = 0.5 * (r + v / r);
    r = 0.5 * (r + v / r);
    r = 0.5 * (r + v / r);
    0.5 * (r + v / r)
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
    /// Alpha of a bright 2px outline ring around the hovered control
    /// (0 = none) — the handoff's hover ring ("Slider größer mit einem hellen
    /// Ring"). Drawn at the node's ANIMATED rect so it tracks the hover-grow.
    pub ring_alpha: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{CornerRadius, FxPx, Rect, VisualStyle};

    fn boxed(x: i32, y: i32, w: i32, h: i32, radius: i32, color: Rgba8) -> LayoutBox {
        LayoutBox {
            node_id: 1,
            rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
            visual: VisualStyle {
                background: Some(color),
                corner_radius: CornerRadius::uniform(FxPx::new(radius)),
                ..VisualStyle::default()
            },
            ..LayoutBox::default()
        }
    }

    /// The glass-stacking contract: a glass ROOT resets its rect to the pure
    /// tint, NESTED glass blends src-over onto the parent's pixels. The old
    /// unconditional replace made every inner glass rect erase its parent —
    /// a whole page of stacked panes showed nothing but the window backdrop.
    #[test]
    fn nested_glass_blends_while_root_glass_replaces() {
        use nexus_layout_types::{GlassLevel, SurfaceMaterial};
        // Root pane: opaque-ish grey tint. Nested row: half-transparent white.
        let mut pane = boxed(0, 0, 20, 1, 0, Rgba8 { r: 80, g: 80, b: 80, a: 255 });
        pane.visual.material = SurfaceMaterial::Glass(GlassLevel::WindowPane);
        let mut row = boxed(0, 0, 10, 1, 0, Rgba8 { r: 255, g: 255, b: 255, a: 128 });
        row.node_id = 2;
        row.visual.material = SurfaceMaterial::Glass(GlassLevel::Subtle);
        row.glass_nested = true;

        let mut buf = [0u8; 20 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 0, 20);
        paint_row(&mut canvas, &[pane.clone(), row.clone()]);
        // Nested: the row's white BLENDS with the pane grey — the pixel keeps
        // a parent contribution (grey shifted toward white, alpha stays up).
        let blended = buf[2];
        assert!(
            blended > 80 && buf[3] == 255,
            "nested glass must blend onto the pane: rgba={:?}",
            &buf[..4]
        );

        // Same row as a ROOT (mis-stamped) would REPLACE: alpha drops to the
        // tint's own 128 — proving the flag is what separates the two paths.
        row.glass_nested = false;
        let mut buf2 = [0u8; 20 * 4];
        let mut canvas2 = RowCanvas::new(&mut buf2, 0, 20);
        paint_row(&mut canvas2, &[pane, row]);
        assert_eq!(buf2[3], 128, "root glass replaces (alpha = tint): rgba={:?}", &buf2[..4]);
    }

    #[test]
    fn hover_wash_tints_only_the_hovered_box() {
        // Two side-by-side boxes; the wash lands on node 2 only, inside its
        // rounded outline (the corner pixel stays untouched).
        let grey = Rgba8 { r: 100, g: 100, b: 100, a: 255 };
        let a = boxed(0, 0, 10, 10, 0, grey);
        let mut b = boxed(10, 0, 10, 10, 4, grey);
        b.node_id = 2;
        let wash =
            HoverWash { node_id: 2, color: Rgba8 { r: 255, g: 255, b: 255, a: 64 }, ring_alpha: 0 };
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
