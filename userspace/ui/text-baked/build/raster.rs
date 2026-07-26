// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Build-time glyph rasterizer for the `wght`-instanced faces (RFC-0082).
//!
//! `fontdue` has no variation-axis API, so Light 300 and SemiBold 600 come
//! from `ttf-parser` outlines that still have to be filled. This is that fill:
//! contours flattened by chord length, then scanline-filled with the NONZERO
//! winding rule — the rule TrueType outlines are authored against. Even-odd
//! (or ignoring contour direction) would fill the counters of `o`, `e`, `8`.
//!
//! 16 vertical subsamples per output row; horizontal coverage is EXACT (span
//! overlap), so vertical supersampling is the only approximation.

// ---------------------------------------------------------------- rasterizer

/// A rasterized glyph in the shape the atlas writer needs — deliberately the
/// same fields `fontdue::Metrics` gives us, so both rasterizers feed one
/// code path.
pub(crate) struct GlyphRaster {
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// Left side bearing in whole pixels.
    pub(crate) xmin: i32,
    /// Baseline → bitmap bottom, positive upwards (fontdue's convention).
    pub(crate) ymin: i32,
    pub(crate) advance: f32,
    pub(crate) bitmap: Vec<u8>,
}

impl GlyphRaster {
    /// An absent glyph: no coverage, no advance. The reader treats this as
    /// "not in this face's charset" and falls back.
    pub(crate) fn absent() -> Self {
        Self { width: 0, height: 0, xmin: 0, ymin: 0, advance: 0.0, bitmap: Vec::new() }
    }
}

/// Collects a glyph outline as flattened polygons in font units. Curves are
/// subdivided by chord length so the flattening error stays well under a
/// device pixel at every size we bake.
struct Flattener {
    contours: Vec<Vec<(f32, f32)>>,
    current: Vec<(f32, f32)>,
    /// Font units per device pixel — drives the subdivision count.
    units_per_px: f32,
}

impl Flattener {
    fn steps(&self, approx_len: f32) -> usize {
        // ~1/3 px per segment, clamped so a hairline curve is not 1 segment
        // and a 120 px bowl is not thousands.
        let px = approx_len / self.units_per_px;
        ((px * 3.0).ceil() as usize).clamp(4, 64)
    }

    fn push(&mut self, x: f32, y: f32) {
        self.current.push((x, y));
    }

    fn finish_contour(&mut self) {
        if self.current.len() >= 3 {
            let done = std::mem::take(&mut self.current);
            self.contours.push(done);
        } else {
            self.current.clear();
        }
    }

    fn last(&self) -> (f32, f32) {
        self.current.last().copied().unwrap_or((0.0, 0.0))
    }
}

impl ttf_parser::OutlineBuilder for Flattener {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_contour();
        self.push(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let n = self.steps((x1 - x0).abs() + (y1 - y0).abs() + (x - x1).abs() + (y - y1).abs());
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let px = mt * mt * x0 + 2.0 * mt * t * x1 + t * t * x;
            let py = mt * mt * y0 + 2.0 * mt * t * y1 + t * t * y;
            self.push(px, py);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = self.last();
        let n = self.steps(
            (x1 - x0).abs()
                + (y1 - y0).abs()
                + (x2 - x1).abs()
                + (y2 - y1).abs()
                + (x - x2).abs()
                + (y - y2).abs(),
        );
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
            self.push(a * x0 + b * x1 + c * x2 + d * x, a * y0 + b * y1 + c * y2 + d * y);
        }
    }

    fn close(&mut self) {
        self.finish_contour();
    }
}

/// Vertical subsamples per output row. Horizontal coverage is exact (span
/// overlap), so this is the only approximation and 16 is plenty even at 13 px.
const SUBSAMPLES: usize = 16;

/// Fills flattened contours with the NONZERO winding rule (the rule TrueType
/// outlines are authored against — even-odd would fill the counters of `o`,
/// `e`, `8`). Contours arrive in font units with y up; this flips to device
/// space (y down) while scaling.
fn fill_contours(contours: &[Vec<(f32, f32)>], scale: f32) -> GlyphRaster {
    // Device-space edges, y flipped.
    let mut edges: Vec<((f32, f32), (f32, f32))> = Vec::new();
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for contour in contours {
        for i in 0..contour.len() {
            let (ax, ay) = contour[i];
            let (bx, by) = contour[(i + 1) % contour.len()];
            let a = (ax * scale, -ay * scale);
            let b = (bx * scale, -by * scale);
            for &(x, y) in &[a, b] {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
            if (a.1 - b.1).abs() > f32::EPSILON {
                edges.push((a, b));
            }
        }
    }
    if edges.is_empty() {
        return GlyphRaster::absent();
    }

    let x0 = min_x.floor() as i32;
    let y0 = min_y.floor() as i32;
    let w = (max_x.ceil() as i32 - x0).max(1) as usize;
    let h = (max_y.ceil() as i32 - y0).max(1) as usize;

    let mut acc = vec![0f32; w * h];
    let mut xs: Vec<(f32, i32)> = Vec::new();
    for row in 0..h {
        for sub in 0..SUBSAMPLES {
            let sample_y = y0 as f32 + row as f32 + (sub as f32 + 0.5) / SUBSAMPLES as f32;
            xs.clear();
            for &((ax, ay), (bx, by)) in &edges {
                let (lo, hi) = if ay < by { (ay, by) } else { (by, ay) };
                if sample_y < lo || sample_y >= hi {
                    continue;
                }
                let t = (sample_y - ay) / (by - ay);
                xs.push((ax + t * (bx - ax), if by > ay { 1 } else { -1 }));
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0i32;
            let mut span_start = 0f32;
            for &(x, dir) in xs.iter() {
                if winding == 0 {
                    span_start = x;
                }
                winding += dir;
                if winding == 0 {
                    add_span(&mut acc[row * w..(row + 1) * w], x0, span_start, x);
                }
            }
        }
    }

    let bitmap = acc
        .iter()
        .map(|&v| ((v / SUBSAMPLES as f32).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    GlyphRaster {
        width: w,
        height: h,
        xmin: x0,
        // fontdue's `ymin` is baseline → bitmap BOTTOM, positive upwards; in
        // our y-down space the bottom sits at `y0 + h`.
        ymin: -(y0 + h as i32),
        advance: 0.0,
        bitmap,
    }
}

/// Accumulate exact horizontal coverage of `[from, to)` into one row.
fn add_span(row: &mut [f32], x0: i32, from: f32, to: f32) {
    if to <= from {
        return;
    }
    let lo = ((from.floor() as i32) - x0).max(0) as usize;
    let hi = ((to.ceil() as i32) - x0).clamp(0, row.len() as i32) as usize;
    for (i, cell) in row.iter_mut().enumerate().take(hi).skip(lo) {
        let px_lo = (x0 + i as i32) as f32;
        let overlap = (to.min(px_lo + 1.0) - from.max(px_lo)).max(0.0);
        *cell += overlap;
    }
}

/// Rasterize one codepoint from a `wght`-instanced face.
pub(crate) fn rasterize_instanced(face: &ttf_parser::Face, ch: char, px: f32) -> GlyphRaster {
    let Some(gid) = face.glyph_index(ch) else {
        return GlyphRaster::absent();
    };
    let upem = f32::from(face.units_per_em().max(1));
    let scale = px / upem;
    let advance = f32::from(face.glyph_hor_advance(gid).unwrap_or(0)) * scale;
    let mut flat =
        Flattener { contours: Vec::new(), current: Vec::new(), units_per_px: upem / px.max(1.0) };
    // A glyph with no outline (space) still has an advance.
    if face.outline_glyph(gid, &mut flat).is_none() {
        return GlyphRaster { advance, ..GlyphRaster::absent() };
    }
    flat.finish_contour();
    let mut raster = fill_contours(&flat.contours, scale);
    raster.advance = advance;
    raster
}
