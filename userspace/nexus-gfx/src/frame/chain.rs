// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The geometry of one glass layer's re-blur (TASK-0215 5c-3 P4b, TASK-0335 P1/P3): the scene
//! below it into the scratch over the reach of the horizontal taps, the horizontal pass into
//! the half-resolution scratch (its rows at `half_row`), the vertical pass into the glass's
//! cache rows — each limited to the band the backdrop changed in (P1), and placeable at any
//! half rows so several glasses blur in the same passes (P3). Pure: the board decides which
//! passes carry these, the executor draws what the board says.

use super::board::{blur_radius, blur_scale};
use super::damage::Area;
use super::layers::FrameLayer;
use crate::backend::gpu::quads::Quad;

/// An inclusive pass area.
pub type PassArea = (u32, u32, u32, u32);

/// One glass's re-blur: the three areas and the two blur quads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    pub radius: u32,
    /// The scene below the glass, in the scratch (display pixels).
    pub scene_area: PassArea,
    /// The horizontal pass: into the half-resolution scratch at `half_row` (half coordinates).
    pub half_quad: Quad,
    pub half_area: PassArea,
    /// Half rows the horizontal result takes (`hh`), and the scratch's rows in all.
    pub half_rows_used: u32,
    pub half_rows_total: u32,
    /// The vertical pass: into the cache rows at `row`.
    pub rows_quad: Quad,
    pub rows_area: PassArea,
}

/// The re-blur of `l` whose cache rows start at `row`, placed at half rows `half_row`, for the
/// backdrop change `band` (`Full` = the whole glass); `None` when the half scratch cannot hold
/// it at that place.
pub fn chain(
    l: &FrameLayer,
    band: Area,
    row: u32,
    half_row: u32,
    display: (u32, u32),
    half_rows: u32,
) -> Option<Chain> {
    let (dw, dh) = display;
    let r = blur_radius(l.backdrop_blur);
    let s = blur_scale(r);
    let taps = r / s;
    let (x, y, w, h) = (l.dst_x, l.dst_y, l.width, l.height);
    let (vw, vh) = (w.div_ceil(s) + 2, h.div_ceil(s) + 2);
    let (hh, halo) = (vh + 2 * taps, s * taps);
    if half_row.saturating_add(hh) > half_rows {
        return None;
    }
    let reach = s * taps + s;
    let (x0, y0) = (x.saturating_sub(reach), y.saturating_sub(reach));
    let x1 = (x + s * vw + s * taps - s).min(dw);
    let y1 = (y + s * vh + s * taps - s).min(dh);
    let (mut scene_area, mut half_area, mut rows_area) = (
        (x0, y0, x1.max(x0 + 1) - 1, y1.max(y0 + 1) - 1),
        (0, half_row, vw - 1, half_row + hh - 1),
        (0, row, vw - 1, row + vh - 1),
    );
    // TASK-0335 P1: a write for a band blurs the band alone — the scene below it over the
    // band's reach, the horizontal pass over the band's half-resolution columns and the rows
    // the vertical taps read, the vertical pass over the band's cache rows; the rest of the
    // rows keep. One texel of margin each side for the bilinear fetches.
    if let Area::Rect { x0: bx0, y0: by0, x1: bx1, y1: by1 } = band {
        let clamp = |lo: u32, hi: u32, a: u32, b: u32| (a.max(lo).min(hi), b.min(hi).max(lo));
        let (sx0, sy0) = (bx0.saturating_sub(reach), by0.saturating_sub(reach));
        let (sx1, sy1) = ((bx1 + reach).min(x1), (by1 + reach).min(y1));
        let (cx0, cx1) = clamp(scene_area.0, scene_area.2, sx0, sx1.max(sx0 + 1) - 1);
        let (cy0, cy1) = clamp(scene_area.1, scene_area.3, sy0, sy1.max(sy0 + 1) - 1);
        scene_area = (cx0, cy0, cx1, cy1);
        // Half-resolution columns and rows of the band in the horizontal pass's frame (its
        // origin `x - s`, `y - s - halo`, at `half_row`) and the cache's (`y - s`, at `row`).
        let (ox, oy) = (x.saturating_sub(s), y.saturating_sub(s));
        let col = |px: u32| px.saturating_sub(ox) / s;
        let (c0, c1) = clamp(0, vw - 1, col(bx0).saturating_sub(1), col(bx1 - 1) + 1);
        let hrow = |py: u32| (py + halo).saturating_sub(oy) / s;
        let (h0, h1) = clamp(
            half_row,
            half_row + hh - 1,
            half_row + hrow(by0).saturating_sub(taps + 1),
            half_row + hrow(by1 - 1) + taps + 1,
        );
        half_area = (c0, h0, c1, h1);
        let vrow = |py: u32| py.saturating_sub(oy) / s;
        let (v0, v1) =
            clamp(row, row + vh - 1, row + vrow(by0).saturating_sub(1), row + vrow(by1 - 1) + 1);
        rows_area = (c0, v0, c1, v1);
    }
    let (fw, fh, sf, hr) = (dw as f32, dh as f32, s as f32, half_rows as f32);
    let (ox, oy) = (x as f32 - sf, y as f32 - sf);
    let source = [sf * vw as f32 / fw, sf * hh as f32 / fh, ox / fw, (oy - halo as f32) / fh];
    let half_quad =
        Quad::blur(0.0, half_row as f32, vw as f32, hh as f32, source, [sf / fw, 0.0], 1.0);
    let source = [vw as f32 / fw, vh as f32 / hr, 0.0, (half_row + taps) as f32 / hr];
    let rows_quad = Quad::blur(0.0, row as f32, vw as f32, vh as f32, source, [0.0, 1.0 / hr], 1.0);
    Some(Chain {
        radius: r,
        scene_area,
        half_quad,
        half_area,
        half_rows_used: hh,
        half_rows_total: half_rows,
        rows_quad,
        rows_area,
    })
}

/// A blur quad cut to `area` (inclusive corners): the quad's rectangle clipped, its source
/// window moved and shrunk with it, so the pixels it still covers sample exactly what the whole
/// quad would have — several glasses can then share one pass with one scissor (TASK-0335 P3).
pub fn clip_blur(q: Quad, area: PassArea) -> Quad {
    let (ax0, ay0, ax1, ay1) =
        (area.0 as f32, area.1 as f32, area.2 as f32 + 1.0, area.3 as f32 + 1.0);
    let (x0, y0) = (q.x.max(ax0), q.y.max(ay0));
    let (x1, y1) = ((q.x + q.w).min(ax1), (q.y + q.h).min(ay1));
    if x1 <= x0 || y1 <= y0 || q.w <= 0.0 || q.h <= 0.0 {
        return Quad::blur(0.0, 0.0, 0.0, 0.0, [0.0; 4], [0.0; 2], 1.0);
    }
    let (fx0, fy0) = ((x0 - q.x) / q.w, (y0 - q.y) / q.h);
    let (fw, fh) = ((x1 - x0) / q.w, (y1 - y0) / q.h);
    let (sx, sy, ox, oy) = (q.push[0], q.push[1], q.push[2], q.push[3]);
    Quad::blur(
        x0,
        y0,
        x1 - x0,
        y1 - y0,
        [sx * fw, sy * fh, ox + sx * fx0, oy + sy * fy0],
        [q.push[4], q.push[5]],
        q.push[6],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clipped_blur_quad_samples_what_the_whole_one_would_have() {
        let q = Quad::blur(0.0, 100.0, 200.0, 50.0, [0.5, 0.25, 0.1, 0.2], [0.01, 0.0], 1.0);
        let c = clip_blur(q, (50, 110, 149, 129));
        assert_eq!((c.x, c.y, c.w, c.h), (50.0, 110.0, 100.0, 20.0));
        // The centre of the clipped quad's first pixel maps where the whole quad mapped it.
        let whole = |px: f32, py: f32| {
            let (u, v) = ((px - q.x) / q.w, (py - q.y) / q.h);
            (u * q.push[0] + q.push[2], v * q.push[1] + q.push[3])
        };
        let cut = |px: f32, py: f32| {
            let (u, v) = ((px - c.x) / c.w, (py - c.y) / c.h);
            (u * c.push[0] + c.push[2], v * c.push[1] + c.push[3])
        };
        for (px, py) in [(50.5, 110.5), (100.0, 120.0), (149.5, 129.5)] {
            let (a, b) = (whole(px, py), cut(px, py));
            assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "{a:?} vs {b:?}");
        }
        assert_eq!((c.push[4], c.push[5], c.push[6]), (0.01, 0.0, 1.0));
    }

    #[test]
    fn test_reject_a_clip_outside_the_quad() {
        let q = Quad::blur(0.0, 0.0, 10.0, 10.0, [1.0, 1.0, 0.0, 0.0], [0.0, 0.0], 1.0);
        let c = clip_blur(q, (20, 20, 29, 29));
        assert_eq!((c.w, c.h), (0.0, 0.0));
    }
}
