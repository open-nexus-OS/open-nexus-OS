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
    /// The vertical pass: into the cache plane `plane`, at the glass's rows inside it.
    pub plane: u32,
    pub rows_quad: Quad,
    pub rows_area: PassArea,
}

/// The re-blur of `l` whose cache rows start at `row` (counted through planes of `plane_rows`
/// rows: the pass targets the row's plane, at the row inside it), placed at half rows
/// `half_row`, for the backdrop change `band` (`Full` = the whole glass); `None` when the half
/// scratch cannot hold it at that place.
pub fn chain(
    l: &FrameLayer,
    band: Area,
    (row, plane_rows): (u32, u32),
    half_row: u32,
    display: (u32, u32),
    half_rows: u32,
) -> Option<Chain> {
    let (plane, row) = (row / plane_rows.max(1), row % plane_rows.max(1));
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
    // TASK-0335 P1: a write for a band blurs again only what the band's change reaches; the
    // rest of the rows keep — and must be what a whole write of the same scene would leave
    // (CW1 Finding 5: the rows of the band alone kept a stale fringe one blur radius wide).
    if let Area::Rect { x0: bx0, y0: by0, x1: bx1, y1: by1 } = band {
        let (scene, half, rows) =
            band_areas((bx0, by0, bx1, by1), (x, y), (s, taps), (vw, vh, hh), (row, half_row));
        // The half and the result areas lie inside the whole write's by construction; the
        // scene is held to the whole write's (the display's edge).
        let (whole, clamp) = (scene_area, |lo: u32, hi: u32, v: u32| v.max(lo).min(hi));
        scene_area = (
            clamp(whole.0, whole.2, scene.0),
            clamp(whole.1, whole.3, scene.1),
            clamp(whole.0, whole.2, scene.2),
            clamp(whole.1, whole.3, scene.3),
        );
        (half_area, rows_area) = (half, rows);
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
        plane,
        rows_quad,
        rows_area,
    })
}

/// The areas a write for the band `(bx0, by0)..(bx1, by1)` runs over (inclusive corners: the
/// scene in display pixels, the half rows from `half_row`, the result from `row`), worked out
/// backward from what each pass reads, in the result's own frame — its origin `(x − s, y − s)`,
/// off the display for a glass at its edge:
/// - the result texels the change reaches: the band's texels ± `taps` (the horizontal pass
///   spreads it across, the vertical one down), one more of margin;
/// - the half rows those read: result row `v` reads half rows `v ..= v + 2·taps` (its taps
///   about half row `v + taps`, below the halo);
/// - the scene pixels those read: half texel `(c, h)` fetches at the pixel corner
///   `(ox + c·s + s/2, oy − halo + h·s + s/2)`, its taps `s` pixels across, each fetch the
///   pixels on both sides of the corner.
///
/// So every texel the band's write puts down is computed from inputs drawn in this frame, and a
/// texel it leaves is one the change cannot reach. The scene is not yet held to the whole
/// write's area.
fn band_areas(
    (bx0, by0, bx1, by1): (u32, u32, u32, u32),
    (x, y): (u32, u32),
    (s, taps): (u32, u32),
    (vw, vh, hh): (u32, u32, u32),
    (row, half_row): (u32, u32),
) -> (PassArea, PassArea, PassArea) {
    let (s, taps) = (i64::from(s.max(1)), i64::from(taps));
    let (ox, oy, halo, corner) = (i64::from(x) - s, i64::from(y) - s, s * taps, s / 2);
    let texel = |p: u32, o: i64| (i64::from(p) - o).div_euclid(s);
    let within = |v: i64, n: u32| v.clamp(0, i64::from(n.max(1)) - 1);
    let (c0, c1) = (
        within(texel(bx0, ox) - taps - 1, vw),
        within(texel(bx1.saturating_sub(1), ox) + taps + 1, vw),
    );
    let (v0, v1) = (
        within(texel(by0, oy) - taps - 1, vh),
        within(texel(by1.saturating_sub(1), oy) + taps + 1, vh),
    );
    let (h0, h1) = (v0, within(v1 + 2 * taps, hh));
    let scene = (
        ox + c0 * s + corner - 1 - halo,
        oy - halo + h0 * s + corner - 1,
        ox + c1 * s + corner + halo,
        oy - halo + h1 * s + corner,
    );
    let u = |v: i64| u32::try_from(v.max(0)).unwrap_or(u32::MAX);
    (
        (u(scene.0), u(scene.1), u(scene.2), u(scene.3)),
        (u(c0), half_row + u(h0), u(c1), half_row + u(h1)),
        (u(c0), row + u(v0), u(c1), row + u(v1)),
    )
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
