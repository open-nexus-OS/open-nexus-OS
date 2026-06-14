// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

use crate::elements::SvgDocument;
use crate::limits::OUTPUT_BYTES_PER_PIXEL;
use crate::math::F32Math;
use crate::tessellate::{tessellate_document, Edge};

/// Rasterized BGRA8888 output.
#[derive(Debug, Clone)]
pub struct RasterOutput {
    pub width: u32,
    pub height: u32,
    pub buffer: Vec<u8>,
}

/// Rasterize an SVG document to a BGRA8888 buffer.
pub fn rasterize_document(doc: &SvgDocument) -> Result<RasterOutput, crate::error::SvgError> {
    let width = (doc.width + 0.99999_f32) as u32;
    let height = (doc.height + 0.99999_f32) as u32;

    if width == 0 || height == 0 {
        return Ok(RasterOutput { width, height, buffer: Vec::new() });
    }

    let edges = tessellate_document(doc);

    let size = (width as usize) * (height as usize) * OUTPUT_BYTES_PER_PIXEL;
    let mut buffer = vec![0u8; size];

    scanline_fill(&edges, width as usize, height as usize, &mut buffer);

    Ok(RasterOutput { width, height, buffer })
}

/// Simple scanline polygon fill with alpha blending.
fn scanline_fill(edges: &[Edge], w: usize, h: usize, buffer: &mut [u8]) {
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
        scanline_fill_shape(&edges[start..end], w, h, buffer);
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
fn scanline_fill_shape(edges: &[Edge], w: usize, h: usize, buffer: &mut [u8]) {
    if edges.is_empty() || w == 0 {
        return;
    }

    let color = edges[0].color;
    if color.a == 0 {
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

    // Per-row coverage accumulator + reusable crossing scratch.
    let mut cov = vec![0f32; w];
    let mut xs: Vec<f32> = Vec::new();
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
                // once, so even-odd spans stay correctly paired.
                if yf >= e.y0 && yf < e.y1 {
                    let t = (yf - e.y0) / (e.y1 - e.y0);
                    xs.push(e.x0 + t * (e.x1 - e.x0));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            let mut i = 0;
            while i + 1 < xs.len() {
                add_span_coverage(&mut cov, xs[i], xs[i + 1], inv_ss, w);
                i += 2;
            }
        }
        let row = y * w;
        for (x, &c) in cov.iter().enumerate() {
            if c > 0.0009 {
                let idx = (row + x) * OUTPUT_BYTES_PER_PIXEL;
                blend_pixel_cov(&mut buffer[idx..idx + 4], color, c.min(1.0));
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
