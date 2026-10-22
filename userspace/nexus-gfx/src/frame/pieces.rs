// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The draws of one layer in a scene pass, as pieces (TASK-0215 5c-3 P4b, TASK-0335
//! P2, P4): a layer is its glass drawn back from its cache rows, its drop shadow and its
//! content. A rounded rectangle is drawn as its cross — the middle column and the side bands,
//! where its coverage is 1 — and its four corner squares, the rounded program for the corners
//! alone: the glass back's cross runs the opaque program, a content's the straight or the
//! opaque one, so a frame that changes the inside of a window (a scroll) computes no corner. The
//! shadow is drawn as its ring — its quad less the layer's shape, plus the shape's corner
//! squares: inside the shape's cross its knockout leaves nothing (`shadow.wgsl`). Every one of
//! those is cut to the pieces around the opaque layer above it (`occlude`): the piece's quad is
//! the layer's part, its texels the part's. Split out of `board.rs` (the module-size ratchet);
//! pure.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_board.rs`, `tests/frame_occlusion.rs`; the parts below
//! INVARIANTS: a piece samples exactly the texels its pixels showed in the whole draw (the
//!   atlas one to one; the cache rows on their scale's grid); only a corner square holds a pixel
//!   the rounded rectangle covers partly or its shadow's knockout lets through; a content
//!   spread over a larger layer is one draw, never cut (its texels are not one to one).

use super::board::{blur_radius, blur_scale, DrawSpec, Pipe, Tex};
use super::layers::FrameLayer;
use super::occlude::{self, Px};
use crate::backend::gpu::quads::Quad;

/// A part of a rounded layer: its rectangle, and whether a corner of the layer is in it.
type Part = Option<(Px, bool)>;

/// The parts of the rectangle `rect` with corner radius `r` drawn apart: the middle column, the
/// side bands, the four corner squares (the empty ones skipped). The columns and rows are cut on
/// the grid of `s` pixels from the rectangle's origin — the near corner squares grown to it, the
/// far ones taking the rest — so the texels of a part on that grid start on a texel (`s` = 1:
/// one to one). The straight parts lie inside the rectangle's cross, where its coverage is 1
/// and its shadow's knockout 0. At radius 0 the rectangle is one straight part.
pub(super) fn rounded_parts(rect: Px, r: u32, s: u32) -> [Part; 7] {
    let (x, y, w, h) = rect;
    if r == 0 {
        let whole = (w > 0 && h > 0).then_some((rect, false));
        return [whole, None, None, None, None, None, None];
    }
    let s = s.max(1);
    let near = |len: u32| r.div_ceil(s).saturating_mul(s).min(len);
    let far = |len: u32, near: u32| near + len.saturating_sub(near + r) / s * s;
    let (c1, r1) = (near(w), near(h));
    let (c2, r2) = (far(w, c1), far(h, r1));
    let some = |p: Px, rounded| (p.2 > 0 && p.3 > 0).then_some((p, rounded));
    [
        some((x + c1, y, c2 - c1, h), false),
        some((x, y + r1, c1, r2 - r1), false),
        some((x + c2, y + r1, w - c2, r2 - r1), false),
        some((x, y, c1, r1), true),
        some((x + c2, y, w - c2, r1), true),
        some((x, y + r2, c1, h - r2), true),
        some((x + c2, y + r2, w - c2, h - r2), true),
    ]
}

/// A layer's content as drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Content {
    pub rect: Px,
    pub band: Px,
    pub tint: [f32; 4],
    pub radius: u32,
    /// One colour (`LAYER_SOLID`): any part of the band draws any part of the layer.
    pub solid: bool,
    /// Opaque inside the rounded rectangle at full opacity: its straight parts run the opaque
    /// program (no blend — 1.2 instead of 4.9 ns a pixel on the board, TASK-0335 P2).
    pub opaque: bool,
}

impl Content {
    /// The band shown at the layer's own size — or one colour: its parts can be drawn apart.
    fn one_to_one(&self) -> bool {
        self.solid || (self.band.2, self.band.3) == (self.rect.2, self.rect.3)
    }

    /// The parts of a rounded layer drawn one to one (`rounded_parts` on the atlas's grid).
    fn parts(&self) -> [Part; 7] {
        rounded_parts(self.rect, self.radius, 1)
    }

    /// The draws of the content: one when it is empty-or-spread-or-square, else its parts.
    fn count(&self) -> usize {
        let (_, _, w, h) = self.rect;
        if w == 0 || h == 0 || self.band.2 == 0 || self.band.3 == 0 {
            0
        } else if self.radius == 0 || !self.one_to_one() {
            1
        } else {
            self.parts().iter().flatten().count()
        }
    }

    /// Draw `k` of the content: its rectangle and whether a corner is in it (the whole content
    /// counts as "rounded" when its radius is and it cannot be split).
    fn part(&self, k: usize) -> Option<(Px, bool)> {
        if k >= self.count() {
            return None;
        }
        if self.radius == 0 || !self.one_to_one() {
            return Some((self.rect, self.radius > 0));
        }
        self.parts().into_iter().flatten().nth(k)
    }

    /// Whether the content's draws may be cut into pieces (its texels are one to one).
    fn cuttable(&self) -> bool {
        self.one_to_one()
    }

    /// The draw of the piece `p` of a part of the layer (`rounded`: a corner is in the part).
    fn draw(&self, rounded: bool, p: Px) -> DrawSpec {
        let (x, y, w, h) = self.rect;
        let (sx, srow, bw, bh) = self.band;
        let r = self.radius as f32;
        let f = |v: u32| v as f32;
        if !self.one_to_one() {
            // The band spread over the layer: one draw, the whole band as its texture.
            let pipe = if self.radius == 0 { Pipe::Straight } else { Pipe::Rounded };
            let quad = Quad::layer(f(x), f(y), f(w), f(h), self.tint, r);
            return DrawSpec { pipe, quad, tex: Tex::Layout { x: sx, row: srow, w: bw, h: bh } };
        }
        let tex = if self.solid {
            // One colour: the whole band serves every part.
            Tex::Layout { x: sx, row: srow, w: bw, h: bh }
        } else {
            Tex::Layout { x: sx + (p.0 - x), row: srow + (p.1 - y), w: p.2, h: p.3 }
        };
        let quad = if rounded {
            Quad::part((f(x), f(y), f(w), f(h)), (f(p.0), f(p.1), f(p.2), f(p.3)), self.tint, r)
        } else {
            Quad::layer(f(p.0), f(p.1), f(p.2), f(p.3), self.tint, 0.0)
        };
        let pipe = match (rounded, self.opaque) {
            (true, _) => Pipe::Rounded,
            (false, true) => Pipe::Opaque,
            (false, false) => Pipe::Straight,
        };
        DrawSpec { pipe, quad, tex }
    }
}

/// The drop shadow's geometry: the shape, its radius and blur, the quad it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shadow {
    shape: Px,
    radius: f32,
    blur: u32,
    offset_y: i32,
    alpha: f32,
}

impl Shadow {
    fn of(l: &FrameLayer) -> Option<Self> {
        if l.shadow_blur == 0 || l.width == 0 || l.height == 0 {
            return None;
        }
        Some(Self {
            shape: (l.dst_x, l.dst_y, l.width, l.height),
            radius: l.corner_radius.min(l.width / 2).min(l.height / 2) as f32,
            blur: l.shadow_blur,
            offset_y: l.shadow_offset_y,
            alpha: l.shadow_alpha.min(255) as f32 / 255.0,
        })
    }

    /// The quad the whole shadow covers (the shape grown by the blur, moved by the offset),
    /// clamped to the display's origin.
    fn quad_rect(&self) -> Px {
        let (x, y, w, h) = self.shape;
        let b = self.blur;
        let x0 = x.saturating_sub(b);
        let y0 = (y as i64 - i64::from(b) + i64::from(self.offset_y)).max(0) as u32;
        (x0, y0, w + 2 * b, h + 2 * b)
    }

    /// The shadow's ring (TASK-0335 P4): its quad less the layer's shape — the bands around the
    /// shape — and the shape's four corner squares, outside whose arcs it shows. Inside the
    /// shape's cross the knockout leaves nothing (`shadow.wgsl`: zero at every pixel centre whose
    /// distance to the rounded rectangle is not positive), so nothing is drawn there.
    fn ring(&self) -> [Option<Px>; 8] {
        let q = self.quad_rect();
        let [above, below, left, right] = occlude::cut(q, Some(self.shape));
        let (x, y, w, h) = self.shape;
        let r = self.radius as u32;
        let corner = |p: Px| if r == 0 { None } else { occlude::intersection(p, q) };
        [
            above,
            below,
            left,
            right,
            corner((x, y, r, r)),
            corner((x + w - r, y, r, r)),
            corner((x, y + h - r, r, r)),
            corner((x + w - r, y + h - r, r, r)),
        ]
    }

    fn whole(&self) -> Quad {
        let (x, y, w, h) = self.shape;
        Quad::shadow(
            x as f32,
            y as f32,
            w as f32,
            h as f32,
            self.radius,
            self.blur as f32,
            [0.0, self.offset_y as f32],
            [0.0, 0.0, 0.0, self.alpha],
        )
    }

    /// The draw of piece `p` of the shadow's quad.
    fn draw(&self, p: Px) -> DrawSpec {
        let whole = self.whole();
        let quad = if (p.0 as f32, p.1 as f32, p.2 as f32, p.3 as f32)
            == (whole.x, whole.y, whole.w, whole.h)
        {
            whole
        } else {
            whole.part_of((p.0 as f32, p.1 as f32, p.2 as f32, p.3 as f32))
        };
        DrawSpec { pipe: Pipe::Shadow, quad, tex: Tex::None }
    }
}

/// A glass's blurred backdrop drawn back over it from its cache rows (the reduced result inside
/// its one-texel margin), its corners cut.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Back {
    rect: Px,
    radius: u32,
    /// The cache rows' scale and the rows' first row.
    scale: u32,
    row: u32,
}

impl Back {
    fn of(l: &FrameLayer, row: u32) -> Self {
        Self {
            rect: (l.dst_x, l.dst_y, l.width, l.height),
            radius: l.corner_radius.min(l.width / 2).min(l.height / 2),
            scale: blur_scale(blur_radius(l.backdrop_blur)),
            row,
        }
    }

    /// The back's parts on its scale's grid (`rounded_parts`): its cross runs the opaque
    /// program — the cache rows are opaque and the coverage is 1 there — its corner squares the
    /// rounded one (TASK-0335 P4: the rounded program over the whole glass was a scroll frame's
    /// largest cost).
    fn parts(&self) -> [Part; 7] {
        rounded_parts(self.rect, self.radius, self.scale)
    }

    /// The draw of piece `p` (on the scale's grid from the layer's origin) of a part of the
    /// glass back (`rounded`: a corner square).
    fn draw(&self, rounded: bool, p: Px) -> DrawSpec {
        let (x, y, w, h) = self.rect;
        let s = self.scale;
        let f = |v: u32| v as f32;
        let (pipe, quad) = if rounded {
            let r = self.radius as f32;
            let whole = (f(x), f(y), f(w), f(h));
            (Pipe::Rounded, Quad::part(whole, (f(p.0), f(p.1), f(p.2), f(p.3)), [1.0; 4], r))
        } else {
            (Pipe::Opaque, Quad::layer(f(p.0), f(p.1), f(p.2), f(p.3), [1.0; 4], 0.0))
        };
        DrawSpec {
            pipe,
            quad,
            tex: Tex::Cache {
                x: 1 + (p.0 - x) / s,
                row: self.row + 1 + (p.1 - y) / s,
                w: p.2.div_ceil(s),
                h: p.3.div_ceil(s),
            },
        }
    }
}

/// The draws of one layer in a scene pass: its glass back, its shadow, its content's parts —
/// each cut to its pieces around `occluder`, the opaque rectangle above it (if one touches
/// it).
#[derive(Clone, Copy, Debug)]
pub(super) struct LayerDraws {
    back: Option<Back>,
    shadow: Option<Shadow>,
    content: Content,
    occluder: Option<Px>,
}

impl LayerDraws {
    pub(super) fn new(
        l: &FrameLayer,
        back_row: Option<u32>,
        content: Content,
        occluder: Option<Px>,
    ) -> Self {
        Self {
            back: back_row.map(|row| Back::of(l, row)),
            shadow: Shadow::of(l),
            content,
            occluder,
        }
    }

    /// The glass back's occluder on its scale's grid.
    fn back_occluder(&self, b: &Back) -> Option<Px> {
        self.occluder.and_then(|o| occlude::on_grid(o, (b.rect.0, b.rect.1), b.scale))
    }

    /// How many draws the layer takes.
    pub(super) fn count(&self) -> usize {
        let back = self.back.map_or(0, |b| {
            let occ = self.back_occluder(&b);
            b.parts().iter().flatten().map(|&(p, _)| occlude::count(p, occ)).sum()
        });
        let shadow = self.shadow.map_or(0, |s| {
            s.ring().iter().flatten().map(|&p| occlude::count(p, self.occluder)).sum()
        });
        let content = (0..self.content.count())
            .map(|k| {
                let (rect, _) = self.content.part(k).unwrap_or(((0, 0, 0, 0), false));
                if self.content.cuttable() {
                    occlude::count(rect, self.occluder)
                } else {
                    1
                }
            })
            .sum::<usize>();
        back + shadow + content
    }

    /// Draw `k` of the layer.
    pub(super) fn draw(&self, mut k: usize) -> Option<DrawSpec> {
        if let Some(b) = self.back {
            let occ = self.back_occluder(&b);
            for (part, rounded) in b.parts().into_iter().flatten() {
                let n = occlude::count(part, occ);
                if k < n {
                    return Some(b.draw(rounded, occlude::piece(part, occ, k)?));
                }
                k -= n;
            }
        }
        if let Some(s) = self.shadow {
            for part in s.ring().into_iter().flatten() {
                let n = occlude::count(part, self.occluder);
                if k < n {
                    return Some(s.draw(occlude::piece(part, self.occluder, k)?));
                }
                k -= n;
            }
        }
        for i in 0..self.content.count() {
            let part = self.content.part(i)?;
            if !self.content.cuttable() {
                if k == 0 {
                    return Some(self.content.draw(part.1, part.0));
                }
                k -= 1;
                continue;
            }
            let n = occlude::count(part.0, self.occluder);
            if k < n {
                return Some(self.content.draw(part.1, occlude::piece(part.0, self.occluder, k)?));
            }
            k -= n;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layer program's coverage at the pixel `(px, py)` of the rounded rectangle `rect` of
    /// radius `r` (`layer.wgsl`: `saturate(0.5 - sd)` at the pixel's centre).
    fn coverage(rect: Px, r: u32, (px, py): (u32, u32)) -> f32 {
        let (q, r) = offset(rect, r, (px, py));
        let outside = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt();
        (0.5 - (outside + q.0.max(q.1).min(0.0) - r)).clamp(0.0, 1.0)
    }

    /// The shadow program's knockout at the pixel `(px, py)` of the layer's shape
    /// (`shadow.wgsl`: `saturate(length(max(abs(u) - shape.xy, 0)) - radius)`).
    fn knock(rect: Px, r: u32, (px, py): (u32, u32)) -> f32 {
        let (q, r) = offset(rect, r, (px, py));
        ((q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt() - r).clamp(0.0, 1.0)
    }

    /// The pixel centre's offset beyond the rounded rectangle's inner box, and the radius.
    fn offset(rect: Px, r: u32, (px, py): (u32, u32)) -> ((f32, f32), f32) {
        let (x, y, w, h) = rect;
        let (hw, hh, r) = (w as f32 * 0.5, h as f32 * 0.5, r as f32);
        let p = (px as f32 + 0.5 - (x as f32 + hw), py as f32 + 0.5 - (y as f32 + hh));
        ((p.0.abs() - (hw - r), p.1.abs() - (hh - r)), r)
    }

    fn pixels(p: Px) -> impl Iterator<Item = (u32, u32)> {
        (p.1..p.1 + p.3).flat_map(move |y| (p.0..p.0 + p.2).map(move |x| (x, y)))
    }

    fn disjoint(parts: &[Px]) {
        for (i, a) in parts.iter().enumerate() {
            for b in parts.iter().skip(i + 1) {
                assert!(!occlude::intersects(*a, *b), "parts overlap: {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn a_rounded_rectangle_is_its_cross_and_its_corner_squares_on_any_grid() {
        let cases = [
            ((100, 50, 301, 207), 16, 1),
            ((100, 50, 301, 207), 16, 4),
            ((7, 3, 64, 40), 12, 2),
            ((0, 0, 30, 30), 15, 4),
            ((5, 5, 33, 19), 9, 4),
        ];
        for (rect, r, s) in cases {
            let parts: Vec<(Px, bool)> = rounded_parts(rect, r, s).into_iter().flatten().collect();
            let rects: Vec<Px> = parts.iter().map(|&(p, _)| p).collect();
            disjoint(&rects);
            assert_eq!(rects.iter().map(|&p| occlude::area(p)).sum::<u64>(), occlude::area(rect));
            for &(p, rounded) in &parts {
                assert_eq!(((p.0 - rect.0) % s, (p.1 - rect.1) % s), (0, 0), "on the grid: {p:?}");
                if rounded {
                    continue;
                }
                // A straight part: full coverage and no shadow at every pixel — the plain
                // programs draw it as the rounded one did.
                for px in pixels(p) {
                    assert_eq!(coverage(rect, r, px), 1.0, "{rect:?} r={r} s={s} at {px:?}");
                    assert_eq!(knock(rect, r, px), 0.0, "{rect:?} r={r} s={s} at {px:?}");
                }
            }
        }
        let square = rounded_parts((4, 4, 10, 10), 0, 4);
        assert_eq!(square, [Some(((4, 4, 10, 10), false)), None, None, None, None, None, None]);
    }

    #[test]
    fn a_shadow_is_drawn_as_its_ring_and_test_reject_a_lost_shadow_pixel() {
        for (shape, r, offset_y) in [((200, 150, 120, 90), 12, 8), ((40, 30, 300, 200), 0, 0)] {
            let s = Shadow { shape, radius: r as f32, blur: 24, offset_y, alpha: 0.5 };
            let ring: Vec<Px> = s.ring().into_iter().flatten().collect();
            let q = s.quad_rect();
            disjoint(&ring);
            assert!(ring.iter().all(|&p| occlude::covers(q, p)), "the ring lies in the quad");
            // Every pixel of the quad the ring leaves out is knocked out: no shadow is lost.
            for px in pixels(q) {
                if !ring.iter().any(|&p| occlude::intersects(p, (px.0, px.1, 1, 1))) {
                    assert_eq!(knock(shape, r, px), 0.0, "a shadow pixel left out at {px:?}");
                }
            }
            // What is left out is the shape's cross: its rectangle less the corner squares.
            let cross = occlude::area(shape) - 4 * u64::from(r) * u64::from(r);
            assert_eq!(
                ring.iter().map(|&p| occlude::area(p)).sum::<u64>(),
                occlude::area(q) - cross
            );
        }
    }
}
