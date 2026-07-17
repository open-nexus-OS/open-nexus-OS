// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Scanline rasterizer for the SVG subset — full-image entry points
//! plus the band API (`plan_document_at` + `RasterPlan::rasterize_rows`):
//! tessellate ONCE into an immutable device-space plan, rasterize any row
//! band byte-identically to the full image (rows carry no cross-row state).
//! The band API is the compute-broker's parallel SVG contract.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below + tests/band_parity.rs (split/scratch
//!   parity) + golden tests; QEMU `SELFTEST: pinched svg ok`
//! ADR: docs/adr/0045-pinched-compute-broker-and-backends.md

use alloc::vec::Vec;

use crate::elements::{Color, FillRule, SvgDocument, Transform};
use crate::gradient::ShapePaint;
use crate::limits::OUTPUT_BYTES_PER_PIXEL;
use crate::math::F32Math;
use crate::tessellate::{tessellate_document_with, Edge};

/// Rasterized BGRA8888 output (premultiplied alpha).
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
    let plan = plan_document_at(doc, out_w, out_h)?;
    let size = (out_w as usize) * (out_h as usize) * OUTPUT_BYTES_PER_PIXEL;
    let mut buffer = vec![0u8; size];
    let mut scratch = plan.scratch();
    plan.rasterize_rows(0, out_h, &mut scratch, &mut buffer)?;
    Ok(RasterOutput { width: out_w, height: out_h, buffer })
}

/// Device-space raster plan: the document tessellated ONCE for a fixed
/// `width × height` target, rasterizable in independent row bands. Rows carry
/// no cross-row state, so disjoint bands rendered by different workers (or the
/// same worker, in any order) are byte-identical to one full rasterize — the
/// contract the compute-broker's parallel SVG job is built on.
pub struct RasterPlan {
    width: u32,
    height: u32,
    edges: Vec<Edge>,
    paints: Vec<ShapePaint>,
    fill_rules: Vec<FillRule>,
    solids: Vec<Option<[f32; 4]>>,
    /// Covered row range (clamped to the target); `y_end < y_start` = nothing.
    y_start: usize,
    y_end: isize,
}

/// Reusable per-worker sweep scratch — one allocation per worker per plan,
/// never per row (bump-allocator-friendly).
pub struct RasterScratch {
    acc: Vec<[f32; 4]>,
    xs: Vec<(f32, u32, i32)>,
    winds: Vec<ShapeWind>,
}

/// Build the device-space [`RasterPlan`] for an explicit target size — the
/// tessellation half of [`rasterize_document_at`], shareable across workers.
pub fn plan_document_at(
    doc: &SvgDocument,
    out_w: u32,
    out_h: u32,
) -> Result<RasterPlan, crate::error::SvgError> {
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
    let n_shapes = paints.len();

    // Global covered y-range.
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for e in &edges {
        min_y = min_y.min(e.y0);
        max_y = max_y.max(e.y1);
    }
    let (y_start, y_end) = if edges.is_empty() || n_shapes == 0 || out_w == 0 || out_h == 0 {
        (1usize, 0isize) // empty range: nothing to draw
    } else {
        (
            (min_y.nexus_floor() as isize).max(0) as usize,
            (max_y.nexus_ceil() as isize - 1).min(out_h as isize - 1),
        )
    };

    // Per-shape fill rule (all edges of a shape agree — set in tessellation)
    // and the premultiplied solid fast path (None = gradient, per-pixel eval).
    let mut fill_rules = vec![FillRule::NonZero; n_shapes];
    for e in &edges {
        if let Some(f) = fill_rules.get_mut(e.shape_id as usize) {
            *f = e.fill_rule;
        }
    }
    let mut solids: Vec<Option<[f32; 4]>> = Vec::with_capacity(n_shapes);
    for p in &paints {
        solids.push(match p {
            ShapePaint::Solid(c) => Some(premult(*c)),
            ShapePaint::Gradient(_) => None,
        });
    }

    Ok(RasterPlan {
        width: out_w,
        height: out_h,
        edges,
        paints,
        fill_rules,
        solids,
        y_start,
        y_end,
    })
}

impl RasterPlan {
    /// FNV-1a over the plan's device-space content (dimensions, edges,
    /// paints' solidity). A drift-detector: two builds of the same document
    /// at the same size must produce the same digest — used by the task #14
    /// pipeline-stage probes to separate plan building from rasterization.
    #[must_use]
    pub fn debug_digest(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut eat = |v: u64| {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        };
        eat(self.width as u64);
        eat(self.height as u64);
        eat(self.edges.len() as u64);
        for e in &self.edges {
            eat(e.x0.to_bits() as u64);
            eat(e.y0.to_bits() as u64);
            eat(e.x1.to_bits() as u64);
            eat(e.y1.to_bits() as u64);
            eat(e.shape_id as u64);
            eat(e.dir as u64 & 0xFFFF_FFFF);
        }
        for sp in &self.solids {
            match sp {
                Some(c) => {
                    for ch in c {
                        eat(ch.to_bits() as u64);
                    }
                }
                None => eat(u64::MAX),
            }
        }
        h
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Fresh sweep scratch sized for this plan (one per worker).
    pub fn scratch(&self) -> RasterScratch {
        RasterScratch {
            acc: vec![[0f32; 4]; self.width as usize],
            xs: Vec::new(),
            winds: vec![ShapeWind::default(); self.paints.len()],
        }
    }

    /// Rasterize rows `[y0, y1)` into `out` (which must hold exactly
    /// `(y1 - y0) * width * 4` bytes and start zeroed/pre-composited: rows
    /// composite OVER the existing bytes, exactly like the full rasterize).
    /// Row `y` lands at `(y - y0) * width * 4` — bands from different calls
    /// concatenate to the full image, byte-identical.
    pub fn rasterize_rows(
        &self,
        y0: u32,
        y1: u32,
        scratch: &mut RasterScratch,
        out: &mut [u8],
    ) -> Result<(), crate::error::SvgError> {
        if y0 > y1 || y1 > self.height {
            return Err(crate::error::SvgError::DimensionTooLarge {
                width: y0 as f32,
                height: y1 as f32,
                max: self.height as f32,
            });
        }
        let w = self.width as usize;
        let rows = (y1 - y0) as usize;
        if out.len() != rows * w * OUTPUT_BYTES_PER_PIXEL {
            return Err(crate::error::SvgError::DimensionTooLarge {
                width: out.len() as f32,
                height: (rows * w * OUTPUT_BYTES_PER_PIXEL) as f32,
                max: self.height as f32,
            });
        }
        fill_rows(self, y0 as usize, y1 as usize, scratch, out);
        Ok(())
    }
}

/// Vertical supersamples per pixel row. Combined with the analytic horizontal
/// span coverage below, this yields `SUBSAMPLES_Y`×(continuous-x) anti-aliasing
/// — smooth edges at any size (cursor/icons stay sharp when scaled for
/// HiDPI/5K because coverage is computed in the target-resolution grid).
const SUBSAMPLES_Y: usize = 4;

/// One shape's crossing state during the left→right sweep of a sub-row.
#[derive(Clone, Copy, Default)]
struct ShapeWind {
    /// Running nonzero winding number.
    wind: i32,
    /// Crossings seen so far (parity drives the even-odd rule).
    crossings: u32,
}

#[inline]
fn shape_inside(s: ShapeWind, rule: FillRule) -> bool {
    match rule {
        FillRule::NonZero => s.wind != 0,
        FillRule::EvenOdd => s.crossings % 2 == 1,
    }
}

/// Straight `Color` → premultiplied `[b, g, r, a]`, channels scaled 0..255.
#[inline]
fn premult(c: Color) -> [f32; 4] {
    let a = c.a as f32 / 255.0;
    [c.b as f32 * a, c.g as f32 * a, c.r as f32 * a, c.a as f32]
}

/// Composite premultiplied `top` OVER the premultiplied accumulator `col`
/// (painter's order: callers fold shapes bottom→top).
#[inline]
fn apply_over(col: &mut [f32; 4], top: [f32; 4]) {
    let inv = 1.0 - top[3] / 255.0;
    for (c, t) in col.iter_mut().zip(top.iter()) {
        *c = t + *c * inv;
    }
}

/// Conflation-free scanline fill: ALL shapes are swept together per sub-row.
///
/// The old renderer filled one shape at a time, alpha-compositing each shape's
/// fractional coverage onto the output. At a shared edge between two abutting
/// shapes that conflates coverage with alpha: 0.5-coverage-B OVER
/// 0.5-coverage-A leaves total alpha 0.75, not 1.0 — a visible seam along
/// every interior contour (the icon "outlines" artifact).
///
/// Here each sub-row is partitioned at the sorted crossings of *every* shape.
/// Between two consecutive crossings the covering set is constant, so the
/// shapes in that interval composite ONCE, in painter's order with their
/// intrinsic alphas, into a premultiplied colour; that colour is then written
/// with the interval's exact analytic pixel overlap. Abutting shapes tile the
/// row — their fractional endpoint weights sum to 1, so no seam — while
/// genuinely overlapping translucent shapes still blend `src OVER dst`. The
/// row accumulates in premultiplied f32 and is composited onto the output
/// once per row.
#[cfg(test)]
fn scanline_fill(edges: &[Edge], paints: &[ShapePaint], w: usize, h: usize, buffer: &mut [u8]) {
    // Test-only wrapper over the banded core: builds the plan bits from raw
    // edges (the unit tests construct edges by hand, without a document).
    if w == 0 || h == 0 {
        return;
    }
    let n_shapes = paints.len();
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for e in edges {
        min_y = min_y.min(e.y0);
        max_y = max_y.max(e.y1);
    }
    let (y_start, y_end) = if edges.is_empty() || n_shapes == 0 {
        (1usize, 0isize)
    } else {
        (
            (min_y.nexus_floor() as isize).max(0) as usize,
            (max_y.nexus_ceil() as isize - 1).min(h as isize - 1),
        )
    };
    let mut fill_rules = vec![FillRule::NonZero; n_shapes];
    for e in edges {
        if let Some(f) = fill_rules.get_mut(e.shape_id as usize) {
            *f = e.fill_rule;
        }
    }
    let mut solids: Vec<Option<[f32; 4]>> = Vec::with_capacity(n_shapes);
    for p in paints {
        solids.push(match p {
            ShapePaint::Solid(c) => Some(premult(*c)),
            ShapePaint::Gradient(_) => None,
        });
    }
    let plan = RasterPlan {
        width: w as u32,
        height: h as u32,
        edges: edges.to_vec(),
        paints: paints.to_vec(),
        fill_rules,
        solids,
        y_start,
        y_end,
    };
    let mut scratch = plan.scratch();
    fill_rows(&plan, 0, h, &mut scratch, buffer);
}

/// The banded scanline core: rasterizes plan rows `[band_y0, band_y1)` into
/// `out` at row offset `y - band_y0`. Every row is computed from the plan's
/// immutable edge list alone (scratch is reset per row/sub-row), so any band
/// partition reproduces the full image byte-identically.
fn fill_rows(
    plan: &RasterPlan,
    band_y0: usize,
    band_y1: usize,
    scratch: &mut RasterScratch,
    out: &mut [u8],
) {
    let w = plan.width as usize;
    if w == 0 || band_y1 <= band_y0 {
        return;
    }
    let edges = &plan.edges;
    let paints = &plan.paints;
    let fill_rules = &plan.fill_rules;
    let solids = &plan.solids;
    let n_shapes = paints.len();
    let y_last = plan.y_end.min(band_y1 as isize - 1);
    if edges.is_empty() || n_shapes == 0 || y_last < plan.y_start as isize {
        return;
    }
    let y_start = plan.y_start.max(band_y0);
    let y_end = y_last as usize;
    if y_end < y_start {
        return;
    }

    // Reused row/sweep scratch — no per-row allocations.
    let acc = &mut scratch.acc;
    let xs = &mut scratch.xs;
    let winds = &mut scratch.winds;
    let inv_ss = 1.0 / SUBSAMPLES_Y as f32;

    for y in y_start..=y_end {
        for a in acc.iter_mut() {
            *a = [0.0; 4];
        }
        let mut row_any = false;

        for sy in 0..SUBSAMPLES_Y {
            let yf = y as f32 + (sy as f32 + 0.5) * inv_ss;
            xs.clear();
            for e in edges {
                // Half-open [y0, y1): a vertex shared by two edges is counted
                // once, so spans stay correctly paired.
                if yf >= e.y0 && yf < e.y1 && (e.shape_id as usize) < n_shapes {
                    let t = (yf - e.y0) / (e.y1 - e.y0);
                    xs.push((e.x0 + t * (e.x1 - e.x0), e.shape_id, e.dir));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
            for s in winds.iter_mut() {
                *s = ShapeWind::default();
            }

            for k in 0..xs.len() - 1 {
                let (cx, sid, dir) = xs[k];
                let s = &mut winds[sid as usize];
                s.wind += dir;
                s.crossings += 1;
                let left = cx.max(0.0);
                let right = xs[k + 1].0.min(w as f32);
                if right <= left {
                    continue;
                }
                // Covering set for this interval; note whether any member
                // needs per-pixel (gradient) evaluation.
                let mut any = false;
                let mut per_pixel = false;
                for sid2 in 0..n_shapes {
                    if shape_inside(winds[sid2], fill_rules[sid2])
                        && !paints[sid2].is_fully_transparent()
                    {
                        any = true;
                        if solids[sid2].is_none() {
                            per_pixel = true;
                        }
                    }
                }
                if !any {
                    continue;
                }

                let first = left.nexus_floor() as usize;
                let last = (right.nexus_ceil() as usize).saturating_sub(1).min(w - 1);
                if !per_pixel {
                    // All solids: fold the stack once, spread analytically.
                    let mut col = [0f32; 4];
                    for sid2 in 0..n_shapes {
                        if shape_inside(winds[sid2], fill_rules[sid2]) {
                            if let Some(sp) = solids[sid2] {
                                apply_over(&mut col, sp);
                            }
                        }
                    }
                    if col[3] <= 0.0 {
                        continue;
                    }
                    for (x, slot) in acc.iter_mut().enumerate().take(last + 1).skip(first) {
                        let px = x as f32;
                        let overlap = right.min(px + 1.0) - left.max(px);
                        if overlap > 0.0 {
                            let wgt = overlap * inv_ss;
                            for (a, c) in slot.iter_mut().zip(col.iter()) {
                                *a += c * wgt;
                            }
                            row_any = true;
                        }
                    }
                } else {
                    // Gradient in the stack: evaluate per pixel centre.
                    for (x, slot) in acc.iter_mut().enumerate().take(last + 1).skip(first) {
                        let px = x as f32;
                        let overlap = right.min(px + 1.0) - left.max(px);
                        if overlap <= 0.0 {
                            continue;
                        }
                        let mut col = [0f32; 4];
                        for sid2 in 0..n_shapes {
                            if shape_inside(winds[sid2], fill_rules[sid2]) {
                                let c = paints[sid2].color_at(px + 0.5, yf);
                                if c.a > 0 {
                                    apply_over(&mut col, premult(c));
                                }
                            }
                        }
                        if col[3] <= 0.0 {
                            continue;
                        }
                        let wgt = overlap * inv_ss;
                        for (a, c) in slot.iter_mut().zip(col.iter()) {
                            *a += c * wgt;
                        }
                        row_any = true;
                    }
                }
            }
        }

        if !row_any {
            continue;
        }
        // Composite the accumulated premultiplied row OVER the output once
        // (band-relative row offset).
        let row = (y - band_y0) * w;
        for (x, px) in acc.iter().enumerate() {
            if px[3] <= 0.24 {
                continue; // < ~0.001 alpha
            }
            let idx = (row + x) * OUTPUT_BYTES_PER_PIXEL;
            let inv = 1.0 - (px[3] / 255.0).min(1.0);
            let d = &mut out[idx..idx + 4];
            for (b, a) in d.iter_mut().zip(px.iter()) {
                *b = (a + *b as f32 * inv).nexus_round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Color;
    use crate::tessellate::Edge;

    fn v(shape_id: u32, x: f32, y0: f32, y1: f32, dir: i32, fill_rule: FillRule) -> Edge {
        Edge { x0: x, y0, x1: x, y1, shape_id, dir, fill_rule }
    }

    /// Axis-aligned rect as two vertical edges (CW: right edge down +1, left
    /// edge up −1) — the minimal closed shape for the sweep.
    fn rect(shape_id: u32, x0: f32, x1: f32, y0: f32, y1: f32) -> [Edge; 2] {
        [
            v(shape_id, x1, y0, y1, 1, FillRule::NonZero),
            v(shape_id, x0, y0, y1, -1, FillRule::NonZero),
        ]
    }

    fn fill(edges: &[Edge], paints: &[ShapePaint], w: usize, h: usize) -> Vec<u8> {
        let mut buf = vec![0u8; w * h * OUTPUT_BYTES_PER_PIXEL];
        scanline_fill(edges, paints, w, h, &mut buf);
        buf
    }

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * OUTPUT_BYTES_PER_PIXEL;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    // Two nested squares wound the SAME direction, as one shape. Under nonzero
    // the centre (inside both) stays filled (winding ±2 ≠ 0); under even-odd the
    // centre is a hole (inside count 2 = even). This is the rule that makes real
    // icons render correctly.
    fn nested_squares(fill_rule: FillRule) -> Vec<Edge> {
        // Outer CW: right edge x=90 down (+1), left edge x=10 up (−1).
        // Inner CW: right edge x=70 down (+1), left edge x=30 up (−1).
        vec![
            v(0, 90.0, 10.0, 90.0, 1, fill_rule),
            v(0, 10.0, 10.0, 90.0, -1, fill_rule),
            v(0, 70.0, 30.0, 70.0, 1, fill_rule),
            v(0, 30.0, 30.0, 70.0, -1, fill_rule),
        ]
    }

    fn center_alpha(fill_rule: FillRule) -> u8 {
        let (w, h) = (100usize, 100usize);
        let paints = [ShapePaint::Solid(Color { r: 255, g: 255, b: 255, a: 255 })];
        let buf = fill(&nested_squares(fill_rule), &paints, w, h);
        px(&buf, w, 50, 50)[3]
    }

    #[test]
    fn nonzero_fills_nested_same_winding_center() {
        assert!(center_alpha(FillRule::NonZero) > 200, "nonzero fills the centre");
    }

    #[test]
    fn even_odd_leaves_nested_center_hole() {
        assert_eq!(center_alpha(FillRule::EvenOdd), 0, "even-odd punches a hole");
    }

    // The conflation regression: two opaque same-colour shapes abutting at a
    // FRACTIONAL x must tile seamlessly. The old per-shape OVER compositing
    // left the shared-edge pixel at alpha ≈ 0.79 (0.3-coverage white OVER'd by
    // 0.7-coverage white) — the visible seam. Partitioned compositing makes
    // the endpoint weights sum to 1.
    #[test]
    fn abutting_same_color_shapes_leave_no_seam() {
        let (w, h) = (100usize, 100usize);
        let mut edges = Vec::new();
        edges.extend_from_slice(&rect(0, 10.0, 50.3, 10.0, 90.0));
        edges.extend_from_slice(&rect(1, 50.3, 90.0, 10.0, 90.0));
        let white = ShapePaint::Solid(Color { r: 255, g: 255, b: 255, a: 255 });
        let buf = fill(&edges, &[white.clone(), white], w, h);
        let p = px(&buf, w, 50, 50); // the pixel split 0.3/0.7 between the shapes
        assert!(p[3] >= 254, "seam pixel must be fully opaque, got alpha {}", p[3]);
        assert!(p[0] >= 254 && p[1] >= 254 && p[2] >= 254, "seam pixel must be full white: {p:?}");
    }

    // Genuinely overlapping translucent shapes must still composite
    // `src OVER dst` in painter's order (the fix must not turn overlap into
    // replacement).
    #[test]
    fn translucent_overlap_composites_over() {
        let (w, h) = (100usize, 100usize);
        let mut edges = Vec::new();
        edges.extend_from_slice(&rect(0, 10.0, 60.0, 10.0, 90.0));
        edges.extend_from_slice(&rect(1, 40.0, 90.0, 10.0, 90.0));
        let red = ShapePaint::Solid(Color { r: 255, g: 0, b: 0, a: 128 });
        let green = ShapePaint::Solid(Color { r: 0, g: 255, b: 0, a: 128 });
        let buf = fill(&edges, &[red, green], w, h);
        let p = px(&buf, w, 50, 50); // inside both
                                     // a = 0.502 + 0.502·0.498 ≈ 0.752 → 192; g(top) ≈ 128; r(bottom) ≈ 64.
        assert!((p[3] as i32 - 192).abs() <= 2, "overlap alpha OVER, got {}", p[3]);
        assert!((p[1] as i32 - 128).abs() <= 2, "top premult green, got {}", p[1]);
        assert!((p[2] as i32 - 64).abs() <= 2, "bottom attenuated red, got {}", p[2]);
    }

    // A shape hidden by a later opaque shape must not bleed through at the
    // shared column (painter's order inside one interval).
    #[test]
    fn opaque_top_shape_hides_bottom() {
        let (w, h) = (100usize, 100usize);
        let mut edges = Vec::new();
        edges.extend_from_slice(&rect(0, 10.0, 90.0, 10.0, 90.0));
        edges.extend_from_slice(&rect(1, 10.0, 90.0, 10.0, 90.0));
        let red = ShapePaint::Solid(Color { r: 255, g: 0, b: 0, a: 255 });
        let blue = ShapePaint::Solid(Color { r: 0, g: 0, b: 255, a: 255 });
        let buf = fill(&edges, &[red, blue], w, h);
        let p = px(&buf, w, 50, 50);
        assert_eq!([p[0], p[2], p[3]], [255, 0, 255], "top opaque blue wins: {p:?}");
    }
}
