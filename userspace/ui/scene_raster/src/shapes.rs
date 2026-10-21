// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The painter's shape fills, anti-aliased (TASK-0251 P2a step 3): every edge pixel is painted
//! with the fraction of its area the shape covers, so curves and diagonals read as the shapes
//! they are on a real panel instead of stair steps. Rounded rects, rings and ellipses take their
//! coverage from the analytic signed distance to the outline (a one-pixel ramp across the edge;
//! axis-aligned edges on whole pixels stay exactly as hard as before), polygons from four
//! sub-scanlines with the exact horizontal overlap of every span, line symbols from the distance
//! to the nearest segment of their polylines (round caps and joins, painted once). No buffers:
//! every fill works on the one row it is given, from fixed stack arrays — the painter runs on
//! services with a non-freeing heap.

use super::RowCanvas;
use nexus_layout_types::{PathPoint, PathShape, Rgba8};

/// `no_std` floor (core `f32` has none; trunc-and-adjust is exact for the pixel-coordinate range
/// this painter works in).
#[inline]
pub(crate) fn floor_i32(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) > v {
        t - 1
    } else {
        t
    }
}

#[inline]
pub(crate) fn ceil_i32(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) < v {
        t + 1
    } else {
        t
    }
}

/// `no_std` sqrt via Newton iterations (the painter has no libm). Seeded from the exponent so
/// five rounds converge across the whole range — exact to well under 8-bit alpha.
#[inline]
pub(crate) fn sqrt_f32(v: f32) -> f32 {
    if v <= 0.0 {
        return 0.0;
    }
    // Halving the exponent gives a seed within a factor of two of the root.
    let mut r = f32::from_bits((v.to_bits() >> 1) + 0x1fc0_0000);
    r = 0.5 * (r + v / r);
    r = 0.5 * (r + v / r);
    r = 0.5 * (r + v / r);
    0.5 * (r + v / r)
}

/// The fraction (0..=1) of the pixel centred at `(px, py)` the rounded rect `(x, y, w, h)` with
/// corner radius `r` covers: the analytic signed distance to its outline, ramped over one pixel.
#[inline]
pub(crate) fn round_rect_coverage(px: f32, py: f32, rect: (f32, f32, f32, f32), r: f32) -> f32 {
    let (x, y, w, h) = rect;
    let (hw, hh) = (w * 0.5, h * 0.5);
    if hw <= 0.0 || hh <= 0.0 {
        return 0.0;
    }
    let r = r.max(0.0).min(hw).min(hh);
    let qx = (px - (x + hw)).abs() - (hw - r);
    let qy = (py - (y + hh)).abs() - (hh - r);
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    let d = sqrt_f32(ox * ox + oy * oy) + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// The fraction of the pixel centred at `(px, py)` the ellipse inscribed in `(x, y, w, h)` covers
/// (the radial distance scaled by the smaller radius — exact for circles).
#[inline]
pub(crate) fn ellipse_coverage(px: f32, py: f32, rect: (f32, f32, f32, f32)) -> f32 {
    let (x, y, w, h) = rect;
    let (rx, ry) = (w * 0.5, h * 0.5);
    if rx <= 0.0 || ry <= 0.0 {
        return 0.0;
    }
    let (dx, dy) = ((px - (x + rx)) / rx, (py - (y + ry)) / ry);
    let d = (sqrt_f32(dx * dx + dy * dy) - 1.0) * rx.min(ry);
    (0.5 - d).clamp(0.0, 1.0)
}

/// Squared distance from `(px, py)` to the segment `a..b`.
#[inline]
fn segment_distance2(px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t =
        if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (ax + t * dx - px, ay + t * dy - py);
    qx * qx + qy * qy
}

/// `c` with its alpha scaled by `coverage` (0..=1).
#[inline]
fn covered(c: Rgba8, coverage: f32) -> Rgba8 {
    Rgba8 { a: (c.a as f32 * coverage.clamp(0.0, 1.0) + 0.5) as u8, ..c }
}

impl RowCanvas<'_> {
    /// The byte index of surface pixel `x` after the scroll shift and the scissor, if it lands.
    #[inline]
    fn pixel_index(&self, x: i32) -> Option<usize> {
        let x = x - self.shift_x;
        if let Some((x0, x1)) = self.clip_x {
            if x < x0 || x >= x1 {
                return None;
            }
        }
        let i = (x * 4) as usize;
        (x >= 0 && x < self.width && i + 4 <= self.buf.len()).then_some(i)
    }

    /// The bytes of model-x span `[x0, x1)` on this row after the scroll shift and the scissor
    /// (`None` when nothing of it lands) — the clip of `blend`/`set`, taken once for a span.
    #[inline]
    fn span_bytes(&self, x0: i32, x1: i32) -> Option<(usize, usize)> {
        let (mut s0, mut s1) = (x0 - self.shift_x, x1 - self.shift_x);
        if let Some((c0, c1)) = self.clip_x {
            s0 = s0.max(c0);
            s1 = s1.min(c1);
        }
        let n = (self.buf.len() / 4) as i32;
        let (s0, s1) = (s0.max(0).min(self.width).min(n), s1.min(self.width).min(n));
        (s1 > s0).then_some((s0 as usize * 4, s1 as usize * 4))
    }

    /// Src-over blend of one colour over span `[x0, x1)` — exactly `blend` on each pixel, its
    /// clip and its constants taken once (TASK-0337: a box's interior is one colour a row, and
    /// pixel-at-a-time cost the board seconds of a big window's first frame).
    pub(crate) fn blend_span(&mut self, x0: i32, x1: i32, c: Rgba8) {
        if c.a == 0 {
            return;
        }
        let Some((b0, b1)) = self.span_bytes(x0, x1) else { return };
        let span = &mut self.buf[b0..b1];
        if c.a == 255 {
            for px in span.chunks_exact_mut(4) {
                px.copy_from_slice(&[c.b, c.g, c.r, 255]);
            }
            return;
        }
        let (a, inv) = (c.a as u32, 255 - c.a as u32);
        let (sb, sg, sr) = (c.b as u32 * a, c.g as u32 * a, c.r as u32 * a);
        for px in span.chunks_exact_mut(4) {
            px[0] = ((px[0] as u32 * inv + sb) / 255) as u8;
            px[1] = ((px[1] as u32 * inv + sg) / 255) as u8;
            px[2] = ((px[2] as u32 * inv + sr) / 255) as u8;
            px[3] = (a + px[3] as u32 * inv / 255) as u8;
        }
    }

    /// `blend_covered` with one coverage over span `[x0, x1)`.
    pub(crate) fn blend_span_covered(&mut self, x0: i32, x1: i32, c: Rgba8, coverage: f32) {
        if coverage >= 1.0 {
            self.blend_span(x0, x1, c);
        } else if coverage > 0.0 {
            self.blend_span(x0, x1, covered(c, coverage));
        }
    }

    /// REPLACE span `[x0, x1)` with `c` — exactly `set` on each pixel (premultiplied), once.
    pub(crate) fn set_span(&mut self, x0: i32, x1: i32, c: Rgba8) {
        let Some((b0, b1)) = self.span_bytes(x0, x1) else { return };
        let a = c.a as u32;
        let px = [
            (c.b as u32 * a / 255) as u8,
            (c.g as u32 * a / 255) as u8,
            (c.r as u32 * a / 255) as u8,
            c.a,
        ];
        for dst in self.buf[b0..b1].chunks_exact_mut(4) {
            dst.copy_from_slice(&px);
        }
    }

    /// REPLACE the fraction `coverage` of one pixel with `c` (premultiplied, alpha included) and
    /// keep the rest of what was there — the anti-aliased edge of a glass reset.
    #[inline]
    pub(crate) fn set_covered(&mut self, x: i32, c: Rgba8, coverage: f32) {
        if coverage >= 1.0 {
            self.set(x, c);
            return;
        }
        if coverage <= 0.0 {
            return;
        }
        let Some(i) = self.pixel_index(x) else {
            return;
        };
        let a = c.a as f32;
        let tint = [c.b as f32 * a / 255.0, c.g as f32 * a / 255.0, c.r as f32 * a / 255.0, a];
        for (k, t) in tint.iter().enumerate() {
            let d = self.buf[i + k] as f32;
            self.buf[i + k] = (d + (t - d) * coverage + 0.5) as u8;
        }
    }

    /// Src-over blend of `c` over the fraction `coverage` of one pixel.
    #[inline]
    pub(crate) fn blend_covered(&mut self, x: i32, c: Rgba8, coverage: f32) {
        if coverage >= 1.0 {
            self.blend(x, c);
        } else if coverage > 0.0 {
            self.blend(x, covered(c, coverage));
        }
    }

    /// This row's slice of an (optionally rounded) rect fill; the corners anti-aliased.
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
        // Rows clear of the corner arcs, and every pixel between the arcs, are wholly covered:
        // one span each (`blend_span`, pixel-identical to `blend` per pixel).
        let in_corner_rows = self.y < y + r || self.y >= y + h - r;
        if !in_corner_rows {
            self.blend_span(x, x + w, c);
            return;
        }
        let rect = (x as f32, y as f32, w as f32, h as f32);
        let py = self.y as f32 + 0.5;
        for xx in (x..x + r).chain(x + w - r..x + w) {
            let cov = round_rect_coverage(xx as f32 + 0.5, py, rect, r as f32);
            self.blend_covered(xx, c, cov);
        }
        self.blend_span(x + r, x + w - r, c);
    }

    /// This row's slice of an (optionally rounded) rect fill, REPLACING the destination pixels
    /// (alpha included) instead of src-over blending. Glass-material boxes use this: whatever
    /// the surface painted BENEATH a glass region must not bake through its translucent fill —
    /// the compositor supplies the backdrop (destination-so-far blur), so the region's surface
    /// pixels start from the pure tint. An anti-aliased corner pixel takes the tint over the
    /// fraction the region covers and keeps the rest of what was there.
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
        let r = radius.max(0).min(w / 2).min(h / 2);
        let rect = (x as f32, y as f32, w as f32, h as f32);
        let py = self.y as f32 + 0.5;
        let in_corner_rows = self.y < y + r || self.y >= y + h - r;
        if !in_corner_rows {
            self.set_span(x, x + w, c);
            return;
        }
        for xx in (x..x + r).chain(x + w - r..x + w) {
            let cov = round_rect_coverage(xx as f32 + 0.5, py, rect, r as f32);
            self.set_covered(xx, c, cov);
        }
        self.set_span(x + r, x + w - r, c);
    }

    /// This row's slice of a rounded BORDER ring: the outer rounded rect minus the
    /// (border-width-inset) inner one, anti-aliased on both edges.
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
        let outer = (x as f32, y as f32, w as f32, h as f32);
        let inner = (
            (x + width) as f32,
            (y + width) as f32,
            (w - 2 * width) as f32,
            (h - 2 * width) as f32,
        );
        let (r_out, r_in) = (radius.max(0) as f32, (radius - width).max(0) as f32);
        let py = self.y as f32 + 0.5;
        let ring = |canvas: &mut Self, xx: i32| {
            let px = xx as f32 + 0.5;
            let cov = round_rect_coverage(px, py, outer, r_out)
                - round_rect_coverage(px, py, inner, r_in);
            canvas.blend_covered(xx, c, cov);
        };
        // The straight sides' rows (below the top edge and its arcs, above the bottom ones): the
        // ring lives in the outer `width + 2` columns of each side — between them a pixel lies
        // wholly inside both the outer and the inner shape (both coverages exactly 1), so the
        // ring there is exactly 0 and nothing is computed (a hairline cost two distance fields
        // on every pixel of every row of a window before).
        let reach = width.max(0) + radius.max(0) + 1;
        let side = width.max(0) + 2;
        if self.y >= y + reach && self.y < y + h - reach && w > 2 * side {
            for xx in (x..x + side).chain(x + w - side..x + w) {
                ring(self, xx);
            }
            return;
        }
        // The top and bottom edges' rows: between the corner zones the edge is straight, so both
        // coverages are one value a row — computed at the middle column, laid as one span.
        let margin = width.max(0) + radius.max(0) + 2;
        if w > 2 * margin {
            for xx in (x..x + margin).chain(x + w - margin..x + w) {
                ring(self, xx);
            }
            let px = (x + w / 2) as f32 + 0.5;
            let cov = round_rect_coverage(px, py, outer, r_out)
                - round_rect_coverage(px, py, inner, r_in);
            self.blend_span_covered(x + margin, x + w - margin, c, cov);
            return;
        }
        for xx in x..x + w {
            ring(self, xx);
        }
    }

    /// This row's slice of the ellipse inscribed in `rect` (`ShapeKind::Circle`), anti-aliased.
    pub(crate) fn fill_ellipse_row(&mut self, rect: (f32, f32, f32, f32), c: Rgba8) {
        let (x, y, w, h) = rect;
        let py = self.y as f32 + 0.5;
        if py < y - 1.0 || py > y + h + 1.0 {
            return;
        }
        for xx in floor_i32(x)..ceil_i32(x + w) {
            let cov = ellipse_coverage(xx as f32 + 0.5, py, rect);
            self.blend_covered(xx, c, cov);
        }
    }

    /// This row's slice of an even-odd polygon fill, anti-aliased: four sub-scanlines, each
    /// pixel covered by the exact horizontal overlap of every span. Points come from `pt(i)` for
    /// `i in 0..n` — the painter NEVER materializes a point list (a per-row `Vec` page-faulted
    /// app-host's non-freeing heap); crossings and spans land in fixed arrays, and a scanline
    /// crossing more than `MAX_CROSSINGS` edges of one contour does not occur for the shapes
    /// this paints (triangles, built-in symbols, sampled curves).
    pub(crate) fn fill_polygon_row(
        &mut self,
        n: usize,
        pt: impl Fn(usize) -> (f32, f32),
        c: Rgba8,
    ) {
        const SUB: usize = 4;
        const MAX_CROSSINGS: usize = 64;
        if n < 3 {
            return;
        }
        let mut spans = [[(0f32, 0f32); MAX_CROSSINGS / 2]; SUB];
        let mut counts = [0usize; SUB];
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        for (k, row_spans) in spans.iter_mut().enumerate() {
            let cy = self.y as f32 + (k as f32 + 0.5) / SUB as f32;
            let mut xs = [0f32; MAX_CROSSINGS];
            let mut m = 0usize;
            for i in 0..n {
                let (ax, ay) = pt(i);
                let (bx, by) = pt((i + 1) % n);
                if ((ay <= cy && by > cy) || (by <= cy && ay > cy)) && m < MAX_CROSSINGS {
                    xs[m] = ax + (cy - ay) / (by - ay) * (bx - ax);
                    m += 1;
                }
            }
            let xs = &mut xs[..m];
            xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            for pair in xs.chunks_exact(2) {
                row_spans[counts[k]] = (pair[0], pair[1]);
                counts[k] += 1;
                min_x = min_x.min(pair[0]);
                max_x = max_x.max(pair[1]);
            }
        }
        if min_x > max_x {
            return;
        }
        for xx in floor_i32(min_x)..ceil_i32(max_x) {
            let (l, r) = (xx as f32, xx as f32 + 1.0);
            let mut area = 0.0f32;
            for (row_spans, &count) in spans.iter().zip(counts.iter()) {
                for &(a, b) in &row_spans[..count] {
                    area += (b.min(r) - a.max(l)).max(0.0);
                }
            }
            self.blend_covered(xx, c, area / SUB as f32);
        }
    }

    /// One normalized `0..1000` contour mapped into a box, filled for this row (the
    /// `ShapeKind::{Path,Vector}` fill) — point mapping is inline, no intermediate list.
    pub(crate) fn fill_contour_row(
        &mut self,
        ps: &PathShape,
        xf: f32,
        yf: f32,
        wf: f32,
        hf: f32,
        c: Rgba8,
    ) {
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

    /// This row's slice of stroked polylines (`ShapeKind::Stroke`) mapped into `rect`: every
    /// pixel within half the stroke width (plus the one-pixel ramp) of the NEAREST segment is
    /// covered by that distance — round caps and joins by construction, and a pixel two
    /// segments share is painted once (no seam, no double blend at a join).
    pub(crate) fn stroke_paths_row(
        &mut self,
        paths: &[PathShape],
        rect: (f32, f32, f32, f32),
        width_milli: u16,
        c: Rgba8,
    ) {
        let (x, y, w, h) = rect;
        let (sx, sy) = (w / 1000.0, h / 1000.0);
        let reach = width_milli as f32 * sx.min(sy) * 0.5 + 0.5;
        let py = self.y as f32 + 0.5;
        if py < y - reach || py > y + h + reach {
            return;
        }
        let map = |p: &PathPoint| (x + p.x_milli as f32 * sx, y + p.y_milli as f32 * sy);
        for xx in floor_i32(x - reach)..ceil_i32(x + w + reach) {
            let px = xx as f32 + 0.5;
            let mut best = reach * reach;
            let mut hit = false;
            for path in paths {
                let pts = &path.points;
                let segments =
                    pts.len().saturating_sub(1) + usize::from(path.closed && pts.len() > 2);
                if pts.len() == 1 {
                    // A lone point is a dot: a capsule of length zero.
                    let a = map(&pts[0]);
                    let d2 = segment_distance2(px, py, a, a);
                    if d2 < best {
                        best = d2;
                        hit = true;
                    }
                }
                for s in 0..segments {
                    let (a, b) = (map(&pts[s]), map(&pts[(s + 1) % pts.len()]));
                    if a.1.min(b.1) - reach > py || a.1.max(b.1) + reach < py {
                        continue;
                    }
                    if a.0.min(b.0) - reach > px || a.0.max(b.0) + reach < px {
                        continue;
                    }
                    let d2 = segment_distance2(px, py, a, b);
                    if d2 < best {
                        best = d2;
                        hit = true;
                    }
                }
            }
            if hit {
                self.blend_covered(xx, c, reach - sqrt_f32(best));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba8 = Rgba8 { r: 255, g: 255, b: 255, a: 255 };

    /// Paints `f` over a black row `y` of `width` pixels; the blue channel per pixel.
    fn row(y: i32, width: i32, f: impl FnOnce(&mut RowCanvas<'_>)) -> Vec<u8> {
        let mut buf = vec![0u8; width as usize * 4];
        let mut canvas = RowCanvas::new(&mut buf, y, width);
        f(&mut canvas);
        buf.chunks_exact(4).map(|p| p[0]).collect()
    }

    fn pt(x: u16, y: u16) -> PathPoint {
        PathPoint::new(x, y)
    }

    #[test]
    fn the_root_is_exact_across_the_painters_range() {
        for v in [0.0001f32, 0.25, 1.0, 2.0, 10.0, 1234.5, 1.0e6] {
            assert!((sqrt_f32(v) - v.sqrt()).abs() <= v.sqrt() * 1e-4, "sqrt({v})");
        }
        assert_eq!(sqrt_f32(0.0), 0.0);
        assert_eq!(sqrt_f32(-1.0), 0.0);
    }

    /// A rounded corner is anti-aliased (partly covered pixels on the arc); the straight edges
    /// of a rect on whole pixels stay exactly as hard as before (no blur on a flat edge).
    #[test]
    fn a_rounded_corner_is_anti_aliased_and_a_straight_edge_stays_hard() {
        let corner = row(1, 24, |c| c.fill_round_rect_row(2, 0, 20, 20, 8, WHITE));
        assert_eq!(corner[0..2], [0, 0], "left of the box");
        assert!(corner[2..8].iter().any(|&v| v > 0 && v < 255), "the arc is soft: {corner:?}");
        assert_eq!(corner[12], 255, "between the arcs");
        let mid = row(10, 24, |c| c.fill_round_rect_row(2, 0, 20, 20, 8, WHITE));
        assert_eq!((mid[1], mid[2], mid[21], mid[22]), (0, 255, 255, 0), "hard straight edges");
    }

    /// A circle's edge pixels are partly covered, its centre fully, outside untouched.
    #[test]
    fn an_ellipse_is_anti_aliased() {
        let r = row(10, 22, |c| c.fill_ellipse_row((1.0, 1.0, 20.0, 20.0), WHITE));
        assert_eq!(r[0], 0);
        assert_eq!(r[11], 255);
        assert!(r[1..4].iter().any(|&v| v > 0 && v < 255), "{r:?}");
    }

    /// A diagonal polygon edge is anti-aliased: on a row through a triangle, the pixel the edge
    /// crosses is partly covered.
    #[test]
    fn a_polygon_edge_is_anti_aliased() {
        let tri = [(0.0f32, 0.0f32), (16.0, 16.0), (0.0, 16.0)];
        let r = row(8, 18, |c| c.fill_polygon_row(3, |i| tri[i], WHITE));
        assert_eq!(r[2], 255, "inside");
        assert!(r[8] > 0 && r[8] < 255, "the diagonal crosses pixel 8: {r:?}");
        assert_eq!(r[12], 0, "outside");
    }

    /// A stroke reaches past its end point by half its width (a round cap), is soft at the
    /// sides, and a pixel two joined segments share is painted ONCE — a translucent colour shows
    /// no darker seam at the join.
    #[test]
    fn a_stroke_has_round_caps_and_paints_a_join_once() {
        let grey = Rgba8 { r: 200, g: 200, b: 200, a: 128 };
        // A polyline in a 20x20 box, mapped 1000 → 20 px: (2,10.5) → (10,10.5) → (18,10.5) —
        // on row 10's pixel centres — 2 px wide.
        let line =
            PathShape { points: vec![pt(100, 525), pt(500, 525), pt(900, 525)], closed: false };
        let r =
            row(10, 22, |c| c.stroke_paths_row(&[line.clone()], (0.0, 0.0, 20.0, 20.0), 100, grey));
        let mid = r[6];
        assert!(mid > 0, "the stroke is painted");
        assert_eq!(r[10], mid, "the join pixel equals a plain one (no double blend)");
        assert!(r[1] > 0, "the round cap reaches past the end point");
        assert_eq!(r[0], 0, "but no further than half the width plus the ramp");
        // One row above the centre line the stroke is partly covered.
        let above = row(9, 22, |c| c.stroke_paths_row(&[line], (0.0, 0.0, 20.0, 20.0), 100, grey));
        assert!(above[6] > 0 && above[6] < mid, "{above:?}");
    }

    /// Strokes outside the row, empty polylines and a lone point (a dot) are all bounded: only
    /// the dot paints, and only around itself.
    #[test]
    fn test_reject_strokes_beyond_the_row_and_degenerate_polylines() {
        let far = PathShape { points: vec![pt(0, 0), pt(1000, 0)], closed: false };
        let empty = PathShape { points: vec![], closed: true };
        let r =
            row(19, 20, |c| c.stroke_paths_row(&[far, empty], (0.0, 0.0, 20.0, 20.0), 100, WHITE));
        assert!(r.iter().all(|&v| v == 0), "{r:?}");
        let dot = PathShape { points: vec![pt(500, 500)], closed: false };
        let r = row(10, 20, |c| c.stroke_paths_row(&[dot], (0.0, 0.0, 20.0, 20.0), 100, WHITE));
        assert!(r[10] > 0 && r[3] == 0 && r[17] == 0, "{r:?}");
    }

    /// The glass reset at an anti-aliased corner takes the tint over the covered fraction only.
    #[test]
    fn a_replaced_corner_keeps_the_uncovered_fraction() {
        let mut buf = vec![255u8; 24 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 1, 24);
        canvas.fill_round_rect_row_replace(2, 0, 20, 20, 8, Rgba8 { r: 0, g: 0, b: 0, a: 255 });
        let blue: Vec<u8> = buf.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(blue[0], 255, "outside untouched");
        assert_eq!(blue[12], 0, "inside replaced");
        assert!(blue[2..8].iter().any(|&v| v > 0 && v < 255), "{blue:?}");
    }
}
