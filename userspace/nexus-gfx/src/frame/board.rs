// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan as the board GPU's passes (TASK-0215 5c-3 P4b). Per glass whose cache step is
//! a write, three passes first: the scene below it drawn again into the scratch (over its reach
//! only), the horizontal blur into the half-resolution scratch, the vertical blur into its cache
//! rows. Then the slot's pass: the wallpaper (or the frame a capture freeze holds, the base the
//! executor names), per layer its glass drawn back from the cache, its shadow, its content. A
//! pass clears its area, so the slot's pass draws the whole scene. A
//! rounded layer's content is its unrounded bands and its four corner squares (the rounded
//! program for those alone, a part's push constants naming the whole layer); a content band
//! spread over a larger layer is one draw. The glass's geometry and the shadow's are the QEMU
//! path's. Pure and allocation-free: the executor names the pipelines and writes the textures'
//! descriptors.

use super::glass::CacheStep;
use super::layers::{FrameLayer, MAX_LAYERS};
use super::plan::{Frame, Planned};
use crate::backend::gpu::quads::Quad;

/// The radii the library blurs at (windowd's glass levels).
pub const BLUR_RADII: [u32; 4] = [8, 20, 30, 40];

/// The radius the library blurs `asked` at: the nearest, the smaller on a tie (the QEMU path's
/// rule).
pub fn blur_radius(asked: u32) -> u32 {
    BLUR_RADII.iter().copied().min_by_key(|r| r.abs_diff(asked)).unwrap_or(20)
}

/// What a draw runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pipe {
    Opaque,
    Straight,
    Rounded,
    Shadow,
    BlurH(u32),
    BlurV(u32),
}

/// What a draw samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tex {
    None,
    /// A rectangle of the shared layout's rows (the wallpaper plane, the atlas): its first
    /// texel's column and absolute row, its size.
    Layout {
        x: u32,
        row: u32,
        w: u32,
        h: u32,
    },
    /// The scratch the scene below a glass is drawn into (the display's size).
    Scratch,
    /// The half-resolution scratch: the display's width, `rows` high.
    Half {
        rows: u32,
    },
    /// A rectangle of the glass cache.
    Cache {
        x: u32,
        row: u32,
        w: u32,
        h: u32,
    },
}

/// One draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawSpec {
    pub pipe: Pipe,
    pub quad: Quad,
    pub tex: Tex,
}

/// What a pass writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Slot,
    Scratch,
    Half,
    Cache,
}

/// A pass's draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PassKind {
    /// The wallpaper and the plan's layers before `upto`.
    Scene { upto: usize },
    /// One draw.
    One(DrawSpec),
}

/// One pass: its target, its area (inclusive corners) and its draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassSpec {
    pub target: Target,
    pub area: (u32, u32, u32, u32),
    pub kind: PassKind,
}

/// The glass counts of a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlassCount {
    pub writes: u32,
    pub reads: u32,
    /// No cache rows left: drawn without its blurred backdrop.
    pub off: u32,
}

/// The frame's plan lowered for the board.
pub struct Board<'a> {
    frame: &'a Frame,
    layers: usize,
    display: (u32, u32),
    /// The layout's rows: a source past them is not read.
    layout_rows: u32,
    /// Rows the half-resolution scratch holds.
    half_rows: u32,
    /// The first row of the plane the scene starts from.
    base_row: u32,
    /// The slot's pass area (inclusive corners): the whole display unless the executor limits
    /// it to what the slot lacks.
    area: (u32, u32, u32, u32),
    /// Draws of the layers before `j`: `first[j]`.
    first: [u16; MAX_LAYERS + 1],
    /// The glass layers written this frame, in walk order.
    writes: [u8; MAX_LAYERS],
    write_count: usize,
    pub glass: GlassCount,
}

impl<'a> Board<'a> {
    /// The plan `planned` of `frame` for a display of `display` pixels, a layout of
    /// `layout_rows` rows and a half-resolution scratch of `half_rows` rows.
    pub fn new(
        frame: &'a Frame,
        planned: Planned,
        display: (u32, u32),
        layout_rows: u32,
        half_rows: u32,
    ) -> Self {
        let mut b = Self {
            frame,
            layers: planned.layers.min(MAX_LAYERS),
            display,
            layout_rows,
            half_rows,
            base_row: 0,
            area: (0, 0, display.0.saturating_sub(1), display.1.saturating_sub(1)),
            first: [0; MAX_LAYERS + 1],
            writes: [0; MAX_LAYERS],
            write_count: 0,
            glass: GlassCount::default(),
        };
        for j in 0..b.layers {
            let n = b.layer_draws(j);
            b.first[j + 1] = b.first[j] + n as u16;
            let cache = frame.step(j).and_then(|s| s.glass).map(|g| g.cache);
            match cache {
                Some(CacheStep::Write(_)) => {
                    b.glass.writes += 1;
                    b.writes[b.write_count] = j as u8;
                    b.write_count += 1;
                }
                Some(CacheStep::Read(_)) => b.glass.reads += 1,
                Some(CacheStep::Off) => b.glass.off += 1,
                None => {}
            }
        }
        b
    }

    /// The scene starts from the plane at layout row `row`: the wallpaper plane's 0 unless the
    /// executor names another (the retained plane while a capture freeze holds the frame).
    pub fn with_base(mut self, row: u32) -> Self {
        self.base_row = row;
        self
    }

    /// The slot's pass limited to `area` (inclusive corners, inside the display): what the slot
    /// lacks — this frame's damage and the frames' since the slot was drawn (RFC-0101 §8).
    pub fn with_area(mut self, area: (u32, u32, u32, u32)) -> Self {
        let (w, h) = self.display;
        self.area =
            (area.0, area.1, area.2.min(w.saturating_sub(1)), area.3.min(h.saturating_sub(1)));
        self
    }

    /// Passes of the frame: three per glass written, then the slot's.
    pub fn pass_count(&self) -> usize {
        3 * self.write_count + 1
    }

    /// Pass `p` of the frame.
    pub fn pass(&self, p: usize) -> Option<PassSpec> {
        if p == 3 * self.write_count {
            return Some(PassSpec {
                target: Target::Slot,
                area: self.area,
                kind: PassKind::Scene { upto: self.layers },
            });
        }
        let j = usize::from(*self.writes[..self.write_count].get(p / 3)?);
        self.chain(j).map(|c| c[p % 3])
    }

    /// Draws of pass `pass`.
    pub fn draws(&self, pass: &PassSpec) -> usize {
        match pass.kind {
            PassKind::Scene { upto } => 1 + usize::from(self.first[upto.min(self.layers)]),
            PassKind::One(_) => 1,
        }
    }

    /// Draw `i` of pass `pass`.
    pub fn draw(&self, pass: &PassSpec, i: usize) -> Option<DrawSpec> {
        match pass.kind {
            PassKind::One(d) => (i == 0).then_some(d),
            PassKind::Scene { upto } => {
                if i == 0 {
                    return Some(self.wallpaper());
                }
                let k = (i - 1) as u16;
                let j = (0..upto.min(self.layers)).find(|&j| k < self.first[j + 1])?;
                self.layer_draw(j, usize::from(k - self.first[j]))
            }
        }
    }

    fn wallpaper(&self) -> DrawSpec {
        let (w, h) = self.display;
        DrawSpec {
            pipe: Pipe::Opaque,
            quad: Quad::layer(0.0, 0.0, w as f32, h as f32, [1.0; 4], 0.0),
            tex: Tex::Layout { x: 0, row: self.base_row, w, h },
        }
    }

    fn layer(&self, j: usize) -> Option<(FrameLayer, Option<CacheStep>)> {
        let s = self.frame.step(j)?;
        Some((s.layer, s.glass.map(|g| g.cache)))
    }

    /// Draws of layer `j`: its glass drawn back (a cache read or write), its shadow, its content.
    fn layer_draws(&self, j: usize) -> usize {
        let Some((l, cache)) = self.layer(j) else { return 0 };
        let glass = matches!(cache, Some(CacheStep::Read(_) | CacheStep::Write(_)));
        usize::from(glass) + usize::from(shadow(&l).is_some()) + content_count(&self.content(&l))
    }

    fn layer_draw(&self, j: usize, mut k: usize) -> Option<DrawSpec> {
        let (l, cache) = self.layer(j)?;
        if let Some(CacheStep::Read(row) | CacheStep::Write(row)) = cache {
            if k == 0 {
                return Some(glass_back(&l, row));
            }
            k -= 1;
        }
        if let Some(s) = shadow(&l) {
            if k == 0 {
                return Some(s);
            }
            k -= 1;
        }
        content_draw(&self.content(&l), k)
    }

    /// The layer's content: its rectangle, its band in the layout (shrunk to the layout's rows),
    /// its tint and corner radius.
    fn content(&self, l: &FrameLayer) -> Content {
        let (bw, bh) = (
            if l.content_w > 0 { l.content_w } else { l.width },
            if l.content_h > 0 { l.content_h } else { l.height },
        );
        let room = self.layout_rows.saturating_sub(l.src_row_abs);
        let (h, bh) = (l.height.min(room), bh.min(room));
        Content {
            rect: (l.dst_x, l.dst_y, l.width, h),
            band: (l.src_x, l.src_row_abs, bw, bh),
            tint: [1.0, 1.0, 1.0, l.opacity.min(255) as f32 / 255.0],
            radius: l.corner_radius.min(l.width / 2).min(h / 2),
        }
    }

    /// The three passes of glass layer `j`: the scene below it into the scratch over the reach of
    /// the horizontal pass's taps, the horizontal pass into the half-resolution scratch, the
    /// vertical pass into its cache rows (the QEMU path's geometry).
    fn chain(&self, j: usize) -> Option<[PassSpec; 3]> {
        let (l, cache) = self.layer(j)?;
        let Some(CacheStep::Write(row)) = cache else { return None };
        let (dw, dh) = self.display;
        let r = blur_radius(l.backdrop_blur);
        let taps = r / 2;
        let (x, y, w, h) = (l.dst_x, l.dst_y, l.width, l.height);
        let (vw, vh) = (w.div_ceil(2) + 2, h.div_ceil(2) + 2);
        let (hh, halo) = (vh + 2 * taps, 2 * taps);
        let reach = 2 * taps + 2;
        let (x0, y0) = (x.saturating_sub(reach), y.saturating_sub(reach));
        let x1 = (x + 2 * vw + 2 * taps - 2).min(dw);
        let y1 = (y + 2 * vh + 2 * taps - 2).min(dh);
        let scene = PassSpec {
            target: Target::Scratch,
            area: (x0, y0, x1.max(x0 + 1) - 1, y1.max(y0 + 1) - 1),
            kind: PassKind::Scene { upto: j },
        };
        let (fw, fh) = (dw as f32, dh as f32);
        let (ox, oy) = (x as f32 - 2.0, y as f32 - 2.0);
        let source = [2.0 * vw as f32 / fw, 2.0 * hh as f32 / fh, ox / fw, (oy - halo as f32) / fh];
        let across = Quad::blur(0.0, 0.0, vw as f32, hh as f32, source, [2.0 / fw, 0.0], 1.0);
        let half = PassSpec {
            target: Target::Half,
            area: (0, 0, vw - 1, hh.min(self.half_rows) - 1),
            kind: PassKind::One(DrawSpec { pipe: Pipe::BlurH(r), quad: across, tex: Tex::Scratch }),
        };
        let hr = self.half_rows as f32;
        let source = [vw as f32 / fw, vh as f32 / hr, 0.0, taps as f32 / hr];
        let down = Quad::blur(0.0, row as f32, vw as f32, vh as f32, source, [0.0, 1.0 / hr], 1.0);
        let rows = PassSpec {
            target: Target::Cache,
            area: (0, row, vw - 1, row + vh - 1),
            kind: PassKind::One(DrawSpec {
                pipe: Pipe::BlurV(r),
                quad: down,
                tex: Tex::Half { rows: self.half_rows },
            }),
        };
        Some([scene, half, rows])
    }
}

/// A rectangle in pixels: x, y, width, height.
type Px = (u32, u32, u32, u32);

/// A part of a rounded layer: its rectangle, and whether a corner of the layer is in it.
type Part = Option<(Px, bool)>;

/// A layer's content as drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Content {
    rect: Px,
    band: Px,
    tint: [f32; 4],
    radius: u32,
}

impl Content {
    /// The band shown at the layer's own size: its parts can be drawn apart.
    fn one_to_one(&self) -> bool {
        (self.band.2, self.band.3) == (self.rect.2, self.rect.3)
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
}

fn content_count(c: &Content) -> usize {
    let (_, _, w, h) = c.rect;
    if w == 0 || h == 0 || c.band.2 == 0 || c.band.3 == 0 {
        0
    } else if c.radius == 0 || !c.one_to_one() {
        1
    } else {
        c.parts().iter().flatten().count()
    }
}

fn content_draw(c: &Content, k: usize) -> Option<DrawSpec> {
    if k >= content_count(c) {
        return None;
    }
    let (x, y, w, h) = c.rect;
    let (sx, srow, bw, bh) = c.band;
    let r = c.radius as f32;
    if c.radius == 0 || !c.one_to_one() {
        let pipe = if c.radius == 0 { Pipe::Straight } else { Pipe::Rounded };
        let quad = Quad::layer(x as f32, y as f32, w as f32, h as f32, c.tint, r);
        return Some(DrawSpec { pipe, quad, tex: Tex::Layout { x: sx, row: srow, w: bw, h: bh } });
    }
    let ((px, py, pw, ph), rounded) = c.parts().into_iter().flatten().nth(k)?;
    let tex = Tex::Layout { x: sx + (px - x), row: srow + (py - y), w: pw, h: ph };
    let f = |v: u32| v as f32;
    let quad = if rounded {
        Quad::part((f(x), f(y), f(w), f(h)), (f(px), f(py), f(pw), f(ph)), c.tint, r)
    } else {
        Quad::layer(f(px), f(py), f(pw), f(ph), c.tint, 0.0)
    };
    let pipe = if rounded { Pipe::Rounded } else { Pipe::Straight };
    Some(DrawSpec { pipe, quad, tex })
}

/// The layer's drop shadow, when it has one (the QEMU path's).
fn shadow(l: &FrameLayer) -> Option<DrawSpec> {
    if l.shadow_blur == 0 || l.width == 0 || l.height == 0 {
        return None;
    }
    let r = l.corner_radius.min(l.width / 2).min(l.height / 2) as f32;
    let alpha = l.shadow_alpha.min(255) as f32 / 255.0;
    let quad = Quad::shadow(
        l.dst_x as f32,
        l.dst_y as f32,
        l.width as f32,
        l.height as f32,
        r,
        l.shadow_blur as f32,
        [0.0, l.shadow_offset_y as f32],
        [0.0, 0.0, 0.0, alpha],
    );
    Some(DrawSpec { pipe: Pipe::Shadow, quad, tex: Tex::None })
}

/// A glass's blurred backdrop drawn back over it from its cache rows (the half-resolution result
/// inside its one-texel margin), its corners cut.
fn glass_back(l: &FrameLayer, row: u32) -> DrawSpec {
    let r = l.corner_radius.min(l.width / 2).min(l.height / 2);
    let (x, y, w, h) = (l.dst_x as f32, l.dst_y as f32, l.width as f32, l.height as f32);
    DrawSpec {
        pipe: if r == 0 { Pipe::Opaque } else { Pipe::Rounded },
        quad: Quad::layer(x, y, w, h, [1.0; 4], r as f32),
        tex: Tex::Cache { x: 1, row: row + 1, w: l.width.div_ceil(2), h: l.height.div_ceil(2) },
    }
}
