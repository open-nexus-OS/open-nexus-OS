// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The draws of one layer in a scene pass, as pieces (TASK-0215 5c-3 P4b, TASK-0335
//! P2): a layer is its glass drawn back from its cache rows, its drop shadow and its content —
//! a rounded content one to one with its band as the middle band, the side bands and the four
//! corner squares (the compositor draws the rest unrounded) — and every one of those cut to
//! the pieces around the opaque layer above it (`occlude`): the piece's quad is the layer's
//! part, its texels the part's. Split out of `board.rs` (the module-size ratchet); pure.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_board.rs`
//! INVARIANTS: a piece samples exactly the texels its pixels showed in the whole draw (the
//!   atlas one to one; the cache rows on their scale's grid); a content spread over a larger
//!   layer is one draw, never cut (its texels are not one to one).

use super::board::{blur_radius, blur_scale, DrawSpec, Pipe, Tex};
use super::layers::FrameLayer;
use super::occlude::{self, Px};
use crate::backend::gpu::quads::Quad;

/// A part of a rounded layer: its rectangle, and whether a corner of the layer is in it.
type Part = Option<(Px, bool)>;

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

    /// The parts of a rounded layer drawn one to one: the middle band, the left and right
    /// bands, the four corner squares — the empty ones skipped.
    fn parts(&self) -> [Part; 7] {
        let (x, y, w, h) = self.rect;
        let r = self.radius;
        let some = |p: Px, rounded| (p.2 > 0 && p.3 > 0).then_some((p, rounded));
        [
            some((x + r, y, w - 2 * r, h), false),
            some((x, y + r, r, h - 2 * r), false),
            some((x + w - r, y + r, r, h - 2 * r), false),
            some((x, y, r, r), true),
            some((x + w - r, y, r, r), true),
            some((x, y + h - r, r, r), true),
            some((x + w - r, y + h - r, r, r), true),
        ]
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

    /// The draw of piece `p` (on the scale's grid from the layer's origin) of the glass back.
    fn draw(&self, p: Px) -> DrawSpec {
        let (x, y, w, h) = self.rect;
        let s = self.scale;
        let f = |v: u32| v as f32;
        let r = self.radius as f32;
        let whole = p == self.rect;
        let quad = if whole {
            Quad::layer(f(x), f(y), f(w), f(h), [1.0; 4], r)
        } else {
            Quad::part((f(x), f(y), f(w), f(h)), (f(p.0), f(p.1), f(p.2), f(p.3)), [1.0; 4], r)
        };
        DrawSpec {
            pipe: if self.radius == 0 { Pipe::Opaque } else { Pipe::Rounded },
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
        let back = self.back.map_or(0, |b| occlude::count(b.rect, self.back_occluder(&b)));
        let shadow = self.shadow.map_or(0, |s| occlude::count(s.quad_rect(), self.occluder));
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
            let n = occlude::count(b.rect, occ);
            if k < n {
                return Some(b.draw(occlude::piece(b.rect, occ, k)?));
            }
            k -= n;
        }
        if let Some(s) = self.shadow {
            let n = occlude::count(s.quad_rect(), self.occluder);
            if k < n {
                return Some(s.draw(occlude::piece(s.quad_rect(), self.occluder, k)?));
            }
            k -= n;
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
