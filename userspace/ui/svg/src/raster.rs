// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

use crate::elements::{FillRule, SvgDocument, Transform};
use crate::gradient::ShapePaint;
use crate::limits::OUTPUT_BYTES_PER_PIXEL;
use crate::math::F32Math;
use crate::tessellate::{tessellate_document_with, Edge};

/// Rasterized BGRA8888 output.
#[derive(Debug, Clone)]
pub struct RasterOutput {
    pub width: u32,
    pub height: u32,
    pub buffer: Vec<u8>,
}

/// Rasterize an SVG document to a BGRA8888 buffer at its intrinsic size.
pub fn rasterize_document(doc: &SvgDocument) -> Result<RasterOutput, crate::error::SvgError> {
    let width = (doc.width + 0.99999_f32) as u32;
    let height = (doc.height + 0.99999_f32) as u32;
    rasterize_document_at(doc, width, height)
}

/// Rasterize a document into an explicit `out_w × out_h` target, scaling geometry
/// to fit (the document's intrinsic box maps onto the output). Coverage is
/// computed in the target grid and curves flatten to the scaled transform, so the
/// result is crisp at HiDPI/5K — this is the asset pipeline's render entry.
pub fn rasterize_document_at(
    doc: &SvgDocument,
    out_w: u32,
    out_h: u32,
) -> Result<RasterOutput, crate::error::SvgError> {
    if out_w == 0 || out_h == 0 {
        return Ok(RasterOutput { width: out_w, height: out_h, buffer: Vec::new() });
    }
    // Bound the actual raster allocation (`out_w × out_h × 4`). This is the real memory
    // guard — the document's coordinate extent (viewBox) may legitimately exceed this
    // while the render target stays small.
    if out_w as f32 > crate::limits::MAX_SVG_DIMENSION
        || out_h as f32 > crate::limits::MAX_SVG_DIMENSION
    {
        return Err(crate::error::SvgError::DimensionTooLarge {
            width: out_w as f32,
            height: out_h as f32,
            max: crate::limits::MAX_SVG_DIMENSION,
        });
    }
    let sx = out_w as f32 / doc.width.max(1e-3);
    let sy = out_h as f32 / doc.height.max(1e-3);
    let root = Transform { a: sx, b: 0.0, c: 0.0, d: sy, e: 0.0, f: 0.0 };

    let (edges, paints) = tessellate_document_with(doc, &root);

    let size = (out_w as usize) * (out_h as usize) * OUTPUT_BYTES_PER_PIXEL;
    let mut buffer = vec![0u8; size];

    scanline_fill(&edges, &paints, out_w as usize, out_h as usize, &mut buffer);

    Ok(RasterOutput { width: out_w, height: out_h, buffer })
}

/// Simple scanline polygon fill with alpha blending. `paints[shape_id]` gives the
/// fill (solid or gradient) for each shape; gradients are evaluated per pixel.
fn scanline_fill(edges: &[Edge], paints: &[ShapePaint], w: usize, h: usize, buffer: &mut [u8]) {
    if edges.is_empty() {
        return;
    }

    let mut start = 0;
    while start < edges.len() {
        let shape_id = edges[start].shape_id;
        let mut end = start + 1;
        while end < edges.len() && edges[end].shape_id == shape_id {
            end += 1;
        }
        if let Some(paint) = paints.get(shape_id as usize) {
            scanline_fill_shape(&edges[start..end], paint, w, h, buffer);
        }
        start = end;
    }
}

/// Vertical supersamples per pixel row. Combined with the analytic horizontal
/// span coverage below, this yields `SUBSAMPLES_Y`×(continuous-x) anti-aliasing
/// — smooth edges at any size (cursor/icons stay sharp when scaled for
/// HiDPI/5K because coverage is computed in the target-resolution grid).
const SUBSAMPLES_Y: usize = 4;

/// Fill one shape at a time. A global edge list breaks overlapping filled
/// paths because scanline pairs from different paths get matched together.
///
/// Anti-aliased: each pixel row accumulates fractional coverage from
/// `SUBSAMPLES_Y` sub-rows, each contributing analytic horizontal coverage
/// (exact fractional overlap at span endpoints). The shape colour is then
/// alpha-composited scaled by that coverage. A shape's edges share one fill
/// colour (the SVG fill model), so coverage is colour-independent.
fn scanline_fill_shape(edges: &[Edge], paint: &ShapePaint, w: usize, h: usize, buffer: &mut [u8]) {
    if edges.is_empty() || w == 0 {
        return;
    }

    // A fully transparent solid contributes nothing; gradients are evaluated
    // per pixel below (individual stops may still be transparent).
    if paint.is_fully_transparent() {
        return;
    }

    // Find y-range.
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for e in edges {
        min_y = min_y.min(e.y0);
        max_y = max_y.max(e.y1);
    }
    let y_start = (min_y.nexus_floor() as isize).max(0) as usize;
    let y_end_i = (max_y.nexus_ceil() as isize - 1).min(h as isize - 1);
    if y_end_i < y_start as isize {
        return;
    }
    let y_end = y_end_i as usize;

    // All edges of a shape share one fill rule (set in tessellation).
    let fill_rule = edges[0].fill_rule;

    // Per-row coverage accumulator + reusable crossing scratch (x, winding dir).
    let mut cov = vec![0f32; w];
    let mut xs: Vec<(f32, i32)> = Vec::new();
    let inv_ss = 1.0 / SUBSAMPLES_Y as f32;

    for y in y_start..=y_end {
        for c in cov.iter_mut() {
            *c = 0.0;
        }
        for sy in 0..SUBSAMPLES_Y {
            let yf = y as f32 + (sy as f32 + 0.5) * inv_ss;
            xs.clear();
            for e in edges {
                // Half-open [y0, y1): a vertex shared by two edges is counted
                // once, so spans stay correctly paired.
                if yf >= e.y0 && yf < e.y1 {
                    let t = (yf - e.y0) / (e.y1 - e.y0);
                    xs.push((e.x0 + t * (e.x1 - e.x0), e.dir));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
            // Walk crossings left→right; the interval [xs[i], xs[i+1]] is inside
            // per the fill rule — nonzero: running winding != 0; even-odd: parity
            // (every other interval, preserving the classic paired-span fill).
            let mut wind = 0i32;
            let mut i = 0;
            while i + 1 < xs.len() {
                wind += xs[i].1;
                let inside = match fill_rule {
                    FillRule::NonZero => wind != 0,
                    FillRule::EvenOdd => i % 2 == 0,
                };
                if inside {
                    add_span_coverage(&mut cov, xs[i].0, xs[i + 1].0, inv_ss, w);
                }
                i += 1;
            }
        }
        let row = y * w;
        let yc = y as f32 + 0.5;
        for (x, &c) in cov.iter().enumerate() {
            if c > 0.0009 {
                // Solid shapes hit a constant; gradients sample at the pixel centre.
                let px_color = paint.color_at(x as f32 + 0.5, yc);
                if px_color.a == 0 {
                    continue;
                }
                let idx = (row + x) * OUTPUT_BYTES_PER_PIXEL;
                blend_pixel_cov(&mut buffer[idx..idx + 4], px_color, c.min(1.0));
            }
        }
    }
}

/// Add the analytic horizontal coverage of the span `[x0, x1)` (in pixels) to
/// the row accumulator, weighted by `weight` (per-sub-row contribution).
/// Endpoint pixels get their exact fractional overlap → smooth left/right edges.
fn add_span_coverage(cov: &mut [f32], x0: f32, x1: f32, weight: f32, w: usize) {
    let left = x0.max(0.0);
    let right = x1.min(w as f32);
    if right <= left {
        return;
    }
    let first = left.nexus_floor() as usize;
    let last = (right.nexus_ceil() as usize).saturating_sub(1).min(w - 1);
    for (x, slot) in cov.iter_mut().enumerate().take(last + 1).skip(first) {
        let px = x as f32;
        let overlap = right.min(px + 1.0) - left.max(px);
        if overlap > 0.0 {
            *slot += overlap * weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Color;
    use crate::tessellate::Edge;

    // Two nested squares wound the SAME direction, as one shape. Under nonzero
    // the centre (inside both) stays filled (winding ±2 ≠ 0); under even-odd the
    // centre is a hole (inside count 2 = even). This is the rule that makes real
    // icons render correctly.
    fn nested_squares(fill_rule: FillRule) -> Vec<Edge> {
        let v = |x: f32, y0: f32, y1: f32, dir: i32| Edge {
            x0: x,
            y0,
            x1: x,
            y1,
            shape_id: 0,
            dir,
            fill_rule,
        };
        // Outer CW: right edge x=90 down (+1), left edge x=10 up (−1).
        // Inner CW: right edge x=70 down (+1), left edge x=30 up (−1).
        vec![
            v(90.0, 10.0, 90.0, 1),
            v(10.0, 10.0, 90.0, -1),
            v(70.0, 30.0, 70.0, 1),
            v(30.0, 30.0, 70.0, -1),
        ]
    }

    fn center_alpha(fill_rule: FillRule) -> u8 {
        let (w, h) = (100usize, 100usize);
        let mut buf = vec![0u8; w * h * OUTPUT_BYTES_PER_PIXEL];
        // One shape (id 0), solid white.
        let paints = [ShapePaint::Solid(Color { r: 255, g: 255, b: 255, a: 255 })];
        scanline_fill(&nested_squares(fill_rule), &paints, w, h, &mut buf);
        buf[(50 * w + 50) * OUTPUT_BYTES_PER_PIXEL + 3]
    }

    #[test]
    fn nonzero_fills_nested_same_winding_center() {
        assert!(center_alpha(FillRule::NonZero) > 200, "nonzero fills the centre");
    }

    #[test]
    fn even_odd_leaves_nested_center_hole() {
        assert_eq!(center_alpha(FillRule::EvenOdd), 0, "even-odd punches a hole");
    }
}

/// Alpha-composite `src_color` (straight alpha) onto a BGRA8888 pixel scaled by
/// `coverage` (0..1). Standard `src OVER dst` so painter-order shapes layer.
fn blend_pixel_cov(dst: &mut [u8], src_color: crate::elements::Color, coverage: f32) {
    let sa = (src_color.a as f32 / 255.0) * coverage;
    if sa <= 0.0 {
        return;
    }
    let inv = 1.0 - sa;
    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * inv;
    dst[0] = (src_color.b as f32 * sa + dst[0] as f32 * inv).nexus_round().clamp(0.0, 255.0) as u8;
    dst[1] = (src_color.g as f32 * sa + dst[1] as f32 * inv).nexus_round().clamp(0.0, 255.0) as u8;
    dst[2] = (src_color.r as f32 * sa + dst[2] as f32 * inv).nexus_round().clamp(0.0, 255.0) as u8;
    dst[3] = (out_a * 255.0).nexus_round().clamp(0.0, 255.0) as u8;
}
