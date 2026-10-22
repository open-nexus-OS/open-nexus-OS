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

use super::chain::{chain, clip_blur, Chain, PassArea};
use super::damage::Area;
use super::glass::CacheStep;
use super::layers::{FrameLayer, MAX_LAYERS};
use super::merge::{mergeable, Written};
use super::occlude::{self, Px};
use super::pieces::{Content, LayerDraws};
use super::plan::{Frame, Planned};
use crate::backend::gpu::quads::Quad;

/// The radii the library blurs at (windowd's glass levels).
pub const BLUR_RADII: [u32; 4] = [8, 20, 30, 40];

/// The radius the library blurs `asked` at: the nearest, the smaller on a tie (the QEMU path's
/// rule).
pub fn blur_radius(asked: u32) -> u32 {
    BLUR_RADII.iter().copied().min_by_key(|r| r.abs_diff(asked)).unwrap_or(20)
}

/// The resolution the library blurs at `radius` (a library radius): a pixel every `scale`
/// full-resolution pixels — half for the smallest radius, quarter from 20 on (TASK-0215 5c-3
/// P6, the stock stack's `glass-quarter`; RFC-0101 §8d). The pipelines' `scale` override.
pub fn blur_scale(radius: u32) -> u32 {
    if radius < 20 {
        2
    } else {
        4
    }
}

/// The program kinds `Board::work_by_program` counts: opaque, straight, rounded, shadow, blur.
pub const PROGRAM_KINDS: usize = 5;

/// The pixels of `d`'s quad inside the pass area `area` (inclusive corners): what it shades.
pub fn drawn_px(d: &DrawSpec, area: (u32, u32, u32, u32)) -> u64 {
    let (ax0, ay0, ax1, ay1) = area;
    let (x0, y0) = (d.quad.x.max(0.0) as u32, d.quad.y.max(0.0) as u32);
    let (x1, y1) = ((d.quad.x + d.quad.w).max(0.0) as u32, (d.quad.y + d.quad.h).max(0.0) as u32);
    let (cx0, cy0) = (x0.max(ax0), y0.max(ay0));
    let (cx1, cy1) = (x1.min(ax1 + 1), y1.min(ay1 + 1));
    u64::from(cx1.saturating_sub(cx0)) * u64::from(cy1.saturating_sub(cy0))
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
    /// The blur pass of every glass written this frame, in one (TASK-0335 P3): the horizontal
    /// passes into their half rows, or the vertical ones into their cache rows.
    Blurs { vertical: bool },
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
    /// The glass layers written this frame, in walk order.
    writes: [u8; MAX_LAYERS],
    write_count: usize,
    pub glass: GlassCount,
    /// Each write's re-blur geometry, at the half rows the frame packed it to (TASK-0335 P3).
    chains: [Option<Chain>; MAX_LAYERS],
    /// The writes share one scene pass, one horizontal and one vertical (`merge::mergeable`,
    /// and every chain fit the half scratch).
    merged: bool,
    /// The shared passes' areas: the scenes' union, the half areas', the cache rows'.
    unions: [PassArea; 3],
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
            writes: [0; MAX_LAYERS],
            write_count: 0,
            glass: GlassCount::default(),
            chains: [None; MAX_LAYERS],
            merged: false,
            unions: [(0, 0, 0, 0); 3],
        };
        for j in 0..b.layers {
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
        b.lay_out_chains();
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

    /// Passes of the frame: the writes' chains — three shared when the writes may share them
    /// (TASK-0335 P3), three per write otherwise — then the slot's.
    pub fn pass_count(&self) -> usize {
        if self.write_count == 0 {
            1
        } else if self.merged {
            4
        } else {
            3 * self.write_count + 1
        }
    }

    /// Pass `p` of the frame.
    pub fn pass(&self, p: usize) -> Option<PassSpec> {
        if p + 1 == self.pass_count() {
            return Some(PassSpec {
                target: Target::Slot,
                area: self.area,
                kind: PassKind::Scene { upto: self.layers },
            });
        }
        if self.merged {
            let low = usize::from(*self.writes[..self.write_count].first()?);
            return Some(match p {
                0 => PassSpec {
                    target: Target::Scratch,
                    area: self.unions[0],
                    kind: PassKind::Scene { upto: low },
                },
                1 => PassSpec {
                    target: Target::Half,
                    area: self.unions[1],
                    kind: PassKind::Blurs { vertical: false },
                },
                2 => PassSpec {
                    target: Target::Cache,
                    area: self.unions[2],
                    kind: PassKind::Blurs { vertical: true },
                },
                _ => return None,
            });
        }
        let w = p / 3;
        let (j, c) = (usize::from(*self.writes[..self.write_count].get(w)?), self.chains[w]?);
        Some(match p % 3 {
            0 => PassSpec {
                target: Target::Scratch,
                area: c.scene_area,
                kind: PassKind::Scene { upto: j },
            },
            1 => PassSpec {
                target: Target::Half,
                area: c.half_area,
                kind: PassKind::One(Self::blur_draw(&c, false, None)),
            },
            _ => PassSpec {
                target: Target::Cache,
                area: c.rows_area,
                kind: PassKind::One(Self::blur_draw(&c, true, None)),
            },
        })
    }

    /// Draws of pass `pass`.
    pub fn draws(&self, pass: &PassSpec) -> usize {
        match pass.kind {
            PassKind::Scene { upto } => self.scene_draws(upto.min(self.layers)),
            PassKind::One(_) => 1,
            PassKind::Blurs { .. } => self.write_count,
        }
    }

    /// Draw `i` of pass `pass`.
    pub fn draw(&self, pass: &PassSpec, i: usize) -> Option<DrawSpec> {
        match pass.kind {
            PassKind::One(d) => (i == 0).then_some(d),
            PassKind::Blurs { vertical } => {
                let c = self.chains.get(i).copied().flatten()?;
                // Each write's quad cut to its own band: one scissor serves them all.
                let own = if vertical { c.rows_area } else { c.half_area };
                Some(Self::blur_draw(&c, vertical, Some(own)))
            }
            PassKind::Scene { upto } => self.scene_draw(upto.min(self.layers), i),
        }
    }

    /// A chain's blur draw: horizontal into the half scratch, vertical into the cache rows;
    /// `cut` clips the quad to that area (a shared pass).
    fn blur_draw(c: &Chain, vertical: bool, cut: Option<PassArea>) -> DrawSpec {
        let (pipe, quad, tex) = if vertical {
            (Pipe::BlurV(c.radius), c.rows_quad, Tex::Half { rows: c.half_rows_total })
        } else {
            (Pipe::BlurH(c.radius), c.half_quad, Tex::Scratch)
        };
        let quad = cut.map_or(quad, |a| clip_blur(quad, a));
        DrawSpec { pipe, quad, tex }
    }

    /// The textured work of a pass in pixels: every draw's quad clipped to the pass's area,
    /// summed (RFC-0108 / TASK-0335 P0 — the board's GPU is memory-bound at a few ns per
    /// textured pixel, so a frame's cost is this count, not its draws).
    pub fn work_px(&self, pass: &PassSpec) -> u64 {
        self.work_by_program(pass).iter().sum()
    }

    /// `work_px` by program (TASK-0335 P4): the textured pixels of the pass's opaque, straight,
    /// rounded and shadow draws, and of its blurs — which program a frame's time goes to.
    pub fn work_by_program(&self, pass: &PassSpec) -> [u64; PROGRAM_KINDS] {
        let mut px = [0u64; PROGRAM_KINDS];
        for d in (0..self.draws(pass)).filter_map(|i| self.draw(pass, i)) {
            let k = match d.pipe {
                Pipe::Opaque => 0,
                Pipe::Straight => 1,
                Pipe::Rounded => 2,
                Pipe::Shadow => 3,
                Pipe::BlurH(_) | Pipe::BlurV(_) => 4,
            };
            px[k] += drawn_px(&d, pass.area);
        }
        px
    }

    /// The whole wallpaper as one draw.
    fn wallpaper(&self) -> DrawSpec {
        let (w, h) = self.display;
        DrawSpec {
            pipe: Pipe::Opaque,
            quad: Quad::layer(0.0, 0.0, w as f32, h as f32, [1.0; 4], 0.0),
            tex: Tex::Layout { x: 0, row: self.base_row, w, h },
        }
    }

    /// Piece `p` of the wallpaper around the opaque layer over it (TASK-0335 P2).
    fn wallpaper_piece(&self, upto: usize, p: Px) -> DrawSpec {
        let whole = self.wallpaper();
        let (w, h) = self.display;
        if p == (0, 0, w, h) {
            return whole;
        }
        let _ = upto;
        DrawSpec {
            pipe: Pipe::Opaque,
            quad: Quad::layer(p.0 as f32, p.1 as f32, p.2 as f32, p.3 as f32, [1.0; 4], 0.0),
            tex: Tex::Layout { x: p.0, row: self.base_row + p.1, w: p.2, h: p.3 },
        }
    }

    /// The largest opaque rectangle (`occlude::opaque_rect`: an opaque layer's, or a glass's
    /// whose back is drawn) of the layers `from..upto` that touches `target`: what hides part
    /// of a draw of `target` in a scene pass up to `upto`.
    fn occluder(&self, target: Px, from: usize, upto: usize) -> Option<Px> {
        let mut best: Option<Px> = None;
        for k in from..upto.min(self.layers) {
            let Some((l, cache)) = self.layer(k) else { continue };
            let back = matches!(cache, Some(CacheStep::Read(_) | CacheStep::Write(_)));
            let Some(o) = occlude::opaque_rect(&l, back) else { continue };
            if !occlude::intersects(o, target) {
                continue;
            }
            if best.is_none_or(|b| occlude::area(o) > occlude::area(b)) {
                best = Some(o);
            }
        }
        best
    }

    fn layer(&self, j: usize) -> Option<(FrameLayer, Option<CacheStep>)> {
        let s = self.frame.step(j)?;
        Some((s.layer, s.glass.map(|g| g.cache)))
    }

    /// The draws of layer `j` in a scene pass up to `upto`: its glass back, its shadow, its
    /// content — in pieces around the opaque layer above it.
    fn layer_draws(&self, j: usize, upto: usize) -> Option<LayerDraws> {
        let (l, cache) = self.layer(j)?;
        let back = match cache {
            Some(CacheStep::Read(row) | CacheStep::Write(row)) => Some(row),
            _ => None,
        };
        let pad =
            if l.shadow_blur > 0 { l.shadow_blur + l.shadow_offset_y.unsigned_abs() } else { 0 };
        let extent = (
            l.dst_x.saturating_sub(pad),
            l.dst_y.saturating_sub(pad),
            l.width + 2 * pad,
            l.height + 2 * pad,
        );
        let occluder = self.occluder(extent, j + 1, upto);
        Some(LayerDraws::new(&l, back, self.content(&l), occluder))
    }

    /// The wallpaper's pieces and every layer's, in walk order.
    fn scene_draws(&self, upto: usize) -> usize {
        let (w, h) = self.display;
        let wall = occlude::count((0, 0, w, h), self.occluder((0, 0, w, h), 0, upto));
        wall + (0..upto).filter_map(|j| self.layer_draws(j, upto)).map(|d| d.count()).sum::<usize>()
    }

    fn scene_draw(&self, upto: usize, mut i: usize) -> Option<DrawSpec> {
        let (w, h) = self.display;
        let occ = self.occluder((0, 0, w, h), 0, upto);
        let wall = occlude::count((0, 0, w, h), occ);
        if i < wall {
            return Some(self.wallpaper_piece(upto, occlude::piece((0, 0, w, h), occ, i)?));
        }
        i -= wall;
        for j in 0..upto {
            let Some(d) = self.layer_draws(j, upto) else { continue };
            let n = d.count();
            if i < n {
                return d.draw(i);
            }
            i -= n;
        }
        None
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
            solid: l.solid,
            opaque: l.opaque && l.opacity >= 255,
        }
    }

    /// The re-blur of written glass `j` placed at half rows `half_row`, for the band the plan
    /// recorded (`chain::chain`); `None` when it is not a write or the half scratch cannot
    /// hold it there.
    fn chain_of(&self, j: usize, half_row: u32) -> Option<Chain> {
        let (l, cache) = self.layer(j)?;
        let Some(CacheStep::Write(row)) = cache else { return None };
        let band = self.frame.step(j).and_then(|s| s.glass).map_or(Area::Full, |g| g.band);
        chain(&l, band, row, half_row, self.display, self.half_rows)
    }

    /// Lays the writes' chains out (TASK-0335 P3): packed one after another in the half
    /// scratch when they all fit and may share the scene (`merge::mergeable`) — then the frame
    /// runs four passes; otherwise each at half row 0, three passes each, as before.
    fn lay_out_chains(&mut self) {
        let mut cursor = 0u32;
        let mut written = [Written { index: 0, scene: Area::None, rect: Area::None }; MAX_LAYERS];
        let mut fits = self.write_count > 1;
        let writes = self.writes;
        for (w, j) in writes[..self.write_count].iter().map(|j| usize::from(*j)).enumerate() {
            let Some(c) = self.chain_of(j, cursor) else {
                fits = false;
                break;
            };
            let (x0, y0, x1, y1) = c.scene_area;
            let rect = self
                .layer(j)
                .map_or(Area::None, |(l, _)| Area::rect(l.dst_x, l.dst_y, l.width, l.height));
            written[w] =
                Written { index: j, scene: Area::rect(x0, y0, x1 - x0 + 1, y1 - y0 + 1), rect };
            self.chains[w] = Some(c);
            cursor += c.half_rows_used;
        }
        let top = self.layers;
        let extent = |i: usize| {
            self.layer(i).map_or(Area::None, |(l, _)| {
                let pad = if l.shadow_blur > 0 {
                    l.shadow_blur + l.shadow_offset_y.unsigned_abs()
                } else {
                    0
                };
                Area::rect(l.dst_x, l.dst_y, l.width, l.height).grown(pad)
            })
        };
        let low = self.writes.first().map_or(0, |j| usize::from(*j));
        if fits && mergeable(&written[..self.write_count], low, top, extent) {
            self.merged = true;
            let union = |pick: &dyn Fn(&Chain) -> PassArea| {
                self.chains[..self.write_count]
                    .iter()
                    .flatten()
                    .map(pick)
                    .fold((u32::MAX, u32::MAX, 0, 0), |u, a| {
                        (u.0.min(a.0), u.1.min(a.1), u.2.max(a.2), u.3.max(a.3))
                    })
            };
            self.unions =
                [union(&|c| c.scene_area), union(&|c| c.half_area), union(&|c| c.rows_area)];
        } else {
            // One by one, each chain at the half scratch's top.
            for w in 0..self.write_count {
                let j = usize::from(self.writes[w]);
                self.chains[w] = self.chain_of(j, 0);
            }
        }
    }
}
