// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The geometry of one glass layer's re-blur (TASK-0215 5c-3 P4b, TASK-0335 P1/P3, TASK-0351
//! P19): the scene below it drawn into the scratch at the blur's resolution — one texel per
//! tap, the texel the mean of the four pixels at the point the tap stands for — the horizontal
//! pass into the half-resolution scratch (its rows at `half_row`), the vertical pass into the
//! glass's cache rows; each limited to the band the backdrop changed in (P1), and placeable at
//! any scene rows and half rows so several glasses blur in the same passes (P3). Both blur
//! passes read neighbours one texel apart: both run the kernel table's paired program, the
//! pass's direction its step. Pure: the board decides which passes carry these, the executor
//! draws what the board says.
//!
//! The scene's texels are the taps that lie on the display. A tap past the display's edge
//! reads the scene's edge texel: the horizontal draw's texture is the scene's own rectangle of
//! the scratch, and the sampler clamps there — never a texel no pass drew.

use super::board::{blur_radius, blur_scale, DrawSpec, Pipe, Tex, BLUR_RADII};
use super::clip::clip_draw;
use super::damage::Area;
use super::layers::FrameLayer;
use crate::backend::gpu::quads::Quad;

/// An inclusive pass area.
pub type PassArea = (u32, u32, u32, u32);

/// How a scene pass into the scratch places a draw of the display: the scratch's texel
/// `(c, r)` stands for the `scale` × `scale` pixels from `origin + (c, r) · scale`, and holds
/// the mean of the four at their middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reduced {
    /// Pixels a texel (`blur_scale`).
    pub scale: u32,
    /// The display pixel the scratch's first texel starts at (off the display for a glass at
    /// its edge, or a scene placed down the scratch).
    pub origin: (i32, i32),
}

impl Reduced {
    /// The display pixels the texels of `area` (inclusive, in the scratch) stand for, inclusive,
    /// from the display's first pixel on.
    pub fn pixels(&self, (x0, y0, x1, y1): PassArea) -> PassArea {
        let s = i64::from(self.scale.max(1));
        let at = |o: i32, v: u32| i64::from(o) + i64::from(v) * s;
        let u = |v: i64| u32::try_from(v.max(0)).unwrap_or(u32::MAX);
        (
            u(at(self.origin.0, x0)),
            u(at(self.origin.1, y0)),
            u(at(self.origin.0, x1) + s - 1),
            u(at(self.origin.1, y1) + s - 1),
        )
    }

    /// `d` as a scene pass over the texels `area` draws it: cut to the pixels the texels stand
    /// for (`clip_draw` — the cut lies on the texels' blocks, and the four pixels a texel reads
    /// lie inside its block, so no texel reads across a cut), then at `1 / scale` of its size.
    /// A draw showing its texels one to one shows them `scale` to one: the bilinear fetch at a
    /// texel's middle is the mean of the four pixels there.
    pub fn place(&self, d: &DrawSpec, area: PassArea) -> DrawSpec {
        let cut = clip_draw(d, self.pixels(area)).unwrap_or(*d);
        let (k, o) = (1.0 / self.scale.max(1) as f32, (self.origin.0 as f32, self.origin.1 as f32));
        let quad = match cut.pipe {
            Pipe::Blur(_) => cut.quad,
            Pipe::Shadow => cut.quad.scaled_shadow(k, o),
            Pipe::Opaque | Pipe::Straight | Pipe::Rounded | Pipe::Premultiplied => {
                cut.quad.scaled(k, o)
            }
        };
        DrawSpec { quad, ..cut }
    }
}

/// One glass's re-blur: the three areas and the two blur quads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    pub radius: u32,
    /// The scene below the glass at the blur's resolution: how its pass places a draw, the
    /// pass's area in the scratch, and the scene's rectangle there — the horizontal pass's
    /// texture (`Tex::Scratch`), `scene_rows_used` rows from the scene row it was placed at.
    pub scene: Reduced,
    pub scene_area: PassArea,
    pub scene_tex: Tex,
    pub scene_rows_used: u32,
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

/// Of `n` texels along one axis, texel `u` standing for the `s` pixels from `first + u · s`:
/// those whose middle lies on a display `extent` pixels long, strictly inside it (`lo..hi`) —
/// a quad that ends at the display's edge covers every one of them whatever the fill rule
/// makes of a pixel centre on an edge. Never empty: a glass wholly off the display keeps one
/// texel (nothing shows it).
fn seen(first: i64, n: u32, s: i64, extent: u32) -> (u32, u32) {
    let middle = first + s / 2;
    let lo = (1 - middle).max(0).div_euclid(s) + i64::from((1 - middle).max(0) % s != 0);
    let hi = (i64::from(extent) - 1 - middle).div_euclid(s) + 1;
    let n = i64::from(n.max(1));
    let lo = lo.clamp(0, n - 1);
    let hi = hi.clamp(lo + 1, n);
    (lo as u32, hi as u32)
}

/// The re-blur of `l` whose cache rows start at `row` (counted through planes of `plane_rows`
/// rows: the pass targets the row's plane, at the row inside it), its scene placed at the
/// scratch's row `scene_row` and its horizontal result at half rows `half_row`, for the
/// backdrop change `band` (`Full` = the whole glass); `None` when the scratch (the display's
/// size) or the half scratch cannot hold it at that place.
pub fn chain(
    l: &FrameLayer,
    band: Area,
    (row, plane_rows): (u32, u32),
    (scene_row, half_row): (u32, u32),
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
    let hh = vh + 2 * taps;
    if half_row.saturating_add(hh) > half_rows {
        return None;
    }
    // The scene's texel (u, t) is the tap half texel (u − taps, t) reads at its centre: the
    // pixels from `first + (u, t) · s`. Result texel (c, v) starts at the glass less one texel;
    // the horizontal pass spreads `taps` texels across, the vertical one `taps` half rows down.
    let (si, halo) = (i64::from(s), i64::from(s * taps));
    let first = (i64::from(x) - si - halo, i64::from(y) - si - halo);
    let ((ua, ub), (ta, tb)) = (seen(first.0, vw + 2 * taps, si, dw), seen(first.1, hh, si, dh));
    let (tw, th) = (ub - ua, tb - ta);
    if scene_row.saturating_add(th) > dh {
        return None;
    }
    let origin =
        (first.0 + i64::from(ua) * si, first.1 + (i64::from(ta) - i64::from(scene_row)) * si);
    let scene = Reduced {
        scale: s,
        origin: (
            i32::try_from(origin.0).unwrap_or(i32::MIN),
            i32::try_from(origin.1).unwrap_or(i32::MIN),
        ),
    };
    // TASK-0335 P1: a write for a band blurs again only what the band's change reaches; the
    // rest of the rows keep — and must be what a whole write of the same scene would leave
    // (CW1 Finding 5: the rows of the band alone kept a stale fringe one blur radius wide).
    let ((c0, c1), (v0, v1)) = match band {
        Area::Rect { x0, y0, x1, y1 } => band_texels((x0, y0, x1, y1), (x, y), (s, taps), (vw, vh)),
        _ => ((0, vw - 1), (0, vh - 1)),
    };
    // What each pass reads, backward from the result: result row `v` reads half rows
    // `v ..= v + 2·taps` (its taps about half row `v + taps`, below the halo); half texel
    // `(c, h)` reads scene texels `(c ..= c + 2·taps, h)`, a texel off the display read as the
    // scene's edge texel. So every texel the band's write puts down is computed from texels
    // drawn in this frame, and a texel it leaves is one the change cannot reach.
    let (h0, h1) = (v0, v1 + 2 * taps);
    let col = |u: u32| u.clamp(ua, ub - 1) - ua;
    let line = |t: u32| scene_row + t.clamp(ta, tb - 1) - ta;
    let scene_area = (col(c0), line(h0), col(c1 + 2 * taps), line(h1));
    let half_area = (c0, half_row + h0, c1, half_row + h1);
    let rows_area = (c0, row + v0, c1, row + v1);
    // The horizontal pass: half texel (c, h) centred on scene texel (c + taps, h), in the
    // scene's rectangle; a tap one texel across.
    let (fw, hr, tf) = (dw as f32, half_rows as f32, (tw as f32, th as f32));
    let source =
        [vw as f32 / tf.0, hh as f32 / tf.1, (taps as f32 - ua as f32) / tf.0, -(ta as f32) / tf.1];
    let half_quad =
        Quad::blur(0.0, half_row as f32, vw as f32, hh as f32, source, [1.0 / tf.0, 0.0], 1.0);
    let source = [vw as f32 / fw, vh as f32 / hr, 0.0, (half_row + taps) as f32 / hr];
    let rows_quad = Quad::blur(0.0, row as f32, vw as f32, vh as f32, source, [0.0, 1.0 / hr], 1.0);
    Some(Chain {
        radius: r,
        scene,
        scene_area,
        scene_tex: Tex::Scratch { x: 0, row: scene_row, w: tw, h: th },
        scene_rows_used: th,
        half_quad,
        half_area,
        half_rows_used: hh,
        half_rows_total: half_rows,
        plane,
        rows_quad,
        rows_area,
    })
}

/// The result texels (columns, rows; inclusive) the change in the band
/// `(bx0, by0)..(bx1, by1)` reaches, in the result's own frame — its origin `(x − s, y − s)`,
/// off the display for a glass at its edge: the band's texels ± `taps` (the horizontal pass
/// spreads it across, the vertical one down), one more of margin.
fn band_texels(
    (bx0, by0, bx1, by1): (u32, u32, u32, u32),
    (x, y): (u32, u32),
    (s, taps): (u32, u32),
    (vw, vh): (u32, u32),
) -> ((u32, u32), (u32, u32)) {
    let (s, taps) = (i64::from(s.max(1)), i64::from(taps));
    let (ox, oy) = (i64::from(x) - s, i64::from(y) - s);
    let texel = |p: u32, o: i64| (i64::from(p) - o).div_euclid(s);
    let within = |v: i64, n: u32| v.clamp(0, i64::from(n.max(1)) - 1) as u32;
    (
        (
            within(texel(bx0, ox) - taps - 1, vw),
            within(texel(bx1.saturating_sub(1), ox) + taps + 1, vw),
        ),
        (
            within(texel(by0, oy) - taps - 1, vh),
            within(texel(by1.saturating_sub(1), oy) + taps + 1, vh),
        ),
    )
}

/// Whether a glass as large as the display, at every radius of the library, has its place in
/// a half scratch of `half_rows` rows of the display's width: its `⌈h / s⌉ + 2` rows and the
/// vertical taps' halo above and below, its `⌈w / s⌉ + 2` columns. (Its scene is the taps on
/// the display — fewer rows than the display's, which the scratch has.) An executor asserts
/// it of its planes when it is built: a write then always has a wave of its own to run in.
pub const fn largest_fits(display: (u32, u32), half_rows: u32) -> bool {
    let mut k = 0;
    while k < BLUR_RADII.len() {
        let s = blur_scale(BLUR_RADII[k]);
        let hh = display.1.div_ceil(s) + 2 + 2 * (BLUR_RADII[k] / s);
        if hh > half_rows || display.0.div_ceil(s) + 2 > display.0 {
            return false;
        }
        k += 1;
    }
    true
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

    #[test]
    fn the_scene_s_texels_are_the_taps_strictly_inside_the_display() {
        // A glass at the display's left edge, quarter resolution, five taps: texel u stands
        // for the pixels from −24 + 4u, its middle at 4u − 22 — the first on the display is 6.
        assert_eq!(seen(-24, 490, 4, 1920), (6, 486));
        // A middle on the display's first corner (pixel 0's left edge) is not inside it.
        assert_eq!(seen(-2, 10, 4, 1920), (1, 10));
        assert_eq!(seen(-1, 10, 2, 1920), (1, 10));
        // Inside the display: every texel.
        assert_eq!(seen(256, 122, 4, 1920), (0, 122));
        // The last middle inside is 1919; one on the display's last corner is not.
        assert_eq!(seen(1900, 10, 4, 1920), (0, 5));
        assert_eq!(seen(1902, 10, 4, 1920), (0, 4));
    }

    #[test]
    fn test_reject_an_empty_scene_for_a_glass_off_the_display() {
        assert_eq!(seen(4000, 10, 4, 1920), (0, 1));
        assert_eq!(seen(-4000, 10, 4, 1920), (9, 10));
    }

    #[test]
    fn the_texels_of_an_area_stand_for_their_blocks_of_pixels() {
        let r = Reduced { scale: 4, origin: (-2, 156) };
        assert_eq!(r.pixels((0, 0, 0, 0)), (0, 156, 1, 159));
        assert_eq!(r.pixels((1, 2, 3, 2)), (2, 164, 13, 167));
    }
}
