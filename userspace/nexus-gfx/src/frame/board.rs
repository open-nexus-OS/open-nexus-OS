// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan as the board GPU's passes (TASK-0215 5c-3 P4b). Per glass whose cache step is
//! a write, three steps first: the scene below it drawn again into the scratch at the blur's
//! resolution (over its reach only — `chain::Reduced` places each draw, TASK-0351 P19), the
//! horizontal blur into the half-resolution scratch, the vertical blur into its cache rows —
//! the writes in waves that share the horizontal pass (`waves`). Then the passes that draw a
//! run's plane again (`plan::runs`, RFC-0109), and the slot's pass: the wallpaper (or the frame
//! a capture freeze holds, the base the executor names), per layer its glass drawn back from the
//! cache, its shadow, its content — a run's quad in place of its layers. A
//! pass clears its area, so the slot's pass draws the whole scene. A
//! rounded layer's content is its unrounded bands and its four corner squares (the rounded
//! program for those alone, a part's push constants naming the whole layer); a content band
//! spread over a larger layer is one draw. The glass's geometry and the shadow's are the QEMU
//! path's. Pure and allocation-free: the executor names the pipelines and writes the textures'
//! descriptors.

use super::chain::{chain, clip_blur, Chain, PassArea, Reduced};
use super::damage::Area;
use super::glass::CacheStep;
use super::layers::{FrameLayer, MAX_LAYERS};
use super::occlude::{self, Px};
use super::pieces::{Content, LayerDraws};
use super::plan::{Frame, Planned, RunSpec, RUN_TARGETS};
use super::walk::Draws;
use super::waves::{Fit, Step, Waves, PLACED};
use crate::backend::gpu::quads::Quad;

pub use super::kernels::{blur_radius, blur_scale, BLUR_RADII};
pub use super::waves::MERGE_MAX;

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
    /// A blur pass at a library radius: the kernel table's paired program, across or down as
    /// its quad's step says (both passes of a write read a source at the blur's resolution).
    Blur(u32),
    /// A group run drawn back: its premultiplied texels times the tint, over the target.
    Premultiplied,
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
    /// A rectangle of the scratch (a plane of the display's size): the scene below a glass at
    /// the blur's resolution. The sampler clamps at the rectangle, so a tap past the display's
    /// edge reads the scene's edge texel.
    Scratch {
        x: u32,
        row: u32,
        w: u32,
        h: u32,
    },
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
    /// A rectangle of a surface buffer an app draws into (RFC-0114 §6): its source and slot,
    /// its first texel's column and row in the slot, its size. The executor addresses the
    /// buffer's own base and stride; gpud checked the rectangle against the buffer at intake.
    Buffer {
        source: u32,
        slot: u8,
        x: u32,
        row: u32,
        w: u32,
        h: u32,
    },
    /// A rectangle of run plane `slot`; its rows past the display hold transparent texels.
    Run {
        slot: u8,
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
    /// A plane of the glass cache.
    Cache(u32),
    /// A run's plane.
    Run(u8),
}

/// A pass's draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PassKind {
    /// The wallpaper and the plan's layers before `upto`.
    Scene { upto: usize },
    /// One draw.
    One(DrawSpec),
    /// The horizontal blur of a wave's writes in one pass (TASK-0351 P19): the `count` writes
    /// from `from`, each into its own half rows.
    Blurs { from: usize, count: usize },
    /// A group run's plane: transparent texels, then the plan's layers `from..to` at rest.
    Rest { from: usize, to: usize },
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
    pub(super) layers: usize,
    pub(super) display: (u32, u32),
    /// The layout's rows: a source past them is not read.
    layout_rows: u32,
    /// Rows the half-resolution scratch holds.
    half_rows: u32,
    /// The first row of the plane the scene starts from.
    base_row: u32,
    /// The slot's pass area (inclusive corners): the whole display unless the executor limits
    /// it to what the slot lacks.
    area: (u32, u32, u32, u32),
    /// Glass layers written this frame (`write_at` finds each in the walk).
    write_count: usize,
    /// The writes in waves: where each one's scene and half rows lie.
    waves: Waves,
    /// The runs the slot's pass draws from, by target (`Frame::take_runs` decided them).
    pub(super) runs: [Option<RunSpec>; RUN_TARGETS],
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
            write_count: 0,
            waves: Waves::lay_out(0, |_, _, _| None),
            runs: frame.runs(),
            glass: GlassCount::default(),
        };
        for j in 0..b.layers {
            match frame.cache_step(j) {
                Some(CacheStep::Write(_)) => {
                    b.glass.writes += 1;
                    b.write_count += 1;
                }
                Some(CacheStep::Read(_)) => b.glass.reads += 1,
                Some(CacheStep::Off) => b.glass.off += 1,
                None => {}
            }
        }
        b.waves = b.lay_out();
        b
    }

    /// The frame's writes in waves (`waves`): a write joins the wave while the scratches hold
    /// its scene and its half rows there and its scene shows no glass the wave writes.
    fn lay_out(&self) -> Waves {
        // Write `w`'s layer, found once.
        let mut index = [0usize; PLACED];
        for (w, j) in (0..self.layers).filter(|j| self.written(*j)).take(PLACED).enumerate() {
            index[w] = j;
        }
        let drawn = |w: usize| {
            self.layer(index[w], false)
                .map_or(Area::None, |(l, _)| Area::rect(l.dst_x, l.dst_y, l.width, l.height))
        };
        Waves::lay_out(self.write_count, |w, first, at| {
            let c = self.chain_of(index[w], at)?;
            let (x0, y0, x1, y1) = c.scene.pixels(c.scene_area);
            let scene = Area::Rect { x0, y0, x1: x1.saturating_add(1), y1: y1.saturating_add(1) };
            Some(Fit {
                rows: (c.scene_rows_used, c.half_rows_used),
                shows: (first..w).any(|k| drawn(k).touches(scene)),
            })
        })
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

    /// Passes of the frame: a scene and a vertical blur a write, a horizontal blur a wave, one
    /// a run drawn again, then the slot's.
    pub fn pass_count(&self) -> usize {
        self.waves.passes() + self.run_passes() + 1
    }

    /// Waves the frame's writes run in.
    pub fn wave_count(&self) -> usize {
        self.waves.count()
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
        if let Some(r) = p.checked_sub(self.waves.passes()) {
            return self.run_pass(r);
        }
        Some(match self.waves.step(p)? {
            Step::Scene(w) => PassSpec {
                target: Target::Scratch,
                area: self.chain_at(w)?.scene_area,
                kind: PassKind::Scene { upto: self.write_at(w)? },
            },
            Step::Across { from, count: 1 } => {
                let c = self.chain_at(from)?;
                PassSpec {
                    target: Target::Half,
                    area: c.half_area,
                    kind: PassKind::One(Self::blur_draw(&c, false, None)),
                }
            }
            // One rectangle around every write's half rows: what it clears beside them is the
            // half scratch's, which holds nothing a wave did not draw.
            Step::Across { from, count } => PassSpec {
                target: Target::Half,
                area: (from..from + count)
                    .filter_map(|w| self.chain_at(w))
                    .map(|c| c.half_area)
                    .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))?,
                kind: PassKind::Blurs { from, count },
            },
            // A pass a write: the cache plane holds what must stay beside its rows.
            Step::Down(w) => {
                let c = self.chain_at(w)?;
                PassSpec {
                    target: Target::Cache(c.plane),
                    area: c.rows_area,
                    kind: PassKind::One(Self::blur_draw(&c, true, None)),
                }
            }
        })
    }

    /// Draws of pass `pass`.
    pub fn draws(&self, pass: &PassSpec) -> usize {
        match pass.kind {
            _ if self.by_walk(pass) => self.walk(pass).count(),
            PassKind::Scene { upto } => self.scene_draws(upto.min(self.layers)),
            PassKind::One(_) => 1,
            PassKind::Rest { .. } => 0,
            PassKind::Blurs { count, .. } => count,
        }
    }

    /// Draw `i` of pass `pass`.
    pub fn draw(&self, pass: &PassSpec, i: usize) -> Option<DrawSpec> {
        match pass.kind {
            _ if self.by_walk(pass) => self.walk(pass).nth(i),
            PassKind::Rest { .. } => None,
            PassKind::One(d) => (i == 0).then_some(d),
            PassKind::Blurs { from, count } => {
                let c = (i < count).then(|| self.chain_at(from + i)).flatten()?;
                // Each write's quad cut to its own band: one scissor serves them all.
                Some(Self::blur_draw(&c, false, Some(c.half_area)))
            }
            PassKind::Scene { upto } => {
                let d = self.scene_draw(upto.min(self.layers), i)?;
                Some(self.reduction(pass).map_or(d, |r| r.place(&d, pass.area)))
            }
        }
    }

    /// How `pass` places its draws when it is a write's scene pass into the scratch: at the
    /// blur's resolution (`chain::Reduced`); `None` for every other pass — its draws are the
    /// display's pixels.
    pub(super) fn reduction(&self, pass: &PassSpec) -> Option<Reduced> {
        let (Target::Scratch, PassKind::Scene { upto }) = (pass.target, pass.kind) else {
            return None;
        };
        // The scene below write `w` is the layers before it: the writes before `upto`.
        let w = (0..upto.min(self.layers)).filter(|j| self.written(*j)).count();
        self.chain_at(w).map(|c| c.scene)
    }

    /// The pass's draws walked once, in the indexed walk's order (TASK-0351 P10): the executor
    /// that keeps them asks the plan's layers once a frame, not once a draw.
    pub fn walk(&self, pass: &PassSpec) -> Draws<'_, 'a> {
        Draws::new(self, pass)
    }

    /// A chain's blur draw: across the scene into the half scratch, or down the half rows into
    /// the cache rows; `cut` clips the quad to that area (a shared pass).
    fn blur_draw(c: &Chain, vertical: bool, cut: Option<PassArea>) -> DrawSpec {
        let (quad, tex) = if vertical {
            (c.rows_quad, Tex::Half { rows: c.half_rows_total })
        } else {
            (c.half_quad, c.scene_tex)
        };
        let quad = cut.map_or(quad, |a| clip_blur(quad, a));
        DrawSpec { pipe: Pipe::Blur(c.radius), quad, tex }
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
        for d in self.walk(pass) {
            let k = match d.pipe {
                Pipe::Opaque => 0,
                Pipe::Straight | Pipe::Premultiplied => 1,
                Pipe::Rounded => 2,
                Pipe::Shadow => 3,
                Pipe::Blur(_) => 4,
            };
            px[k] += drawn_px(&d, pass.area);
        }
        px
    }

    /// The blended pixels (the straight and rounded programs) a scene pass draws, by the kind
    /// of layer they belong to (`layer_wire::layer_key_kind`: 1 body, 2 region, 3 slice, 4
    /// wash, 5 solid, 6 bar, 7 caret; 0 a layer without a key) — TASK-0351 P14: which layers
    /// the board blends.
    pub fn blended_by_kind(&self, pass: &PassSpec) -> [u64; 8] {
        let mut px = [0u64; 8];
        let PassKind::Scene { upto } = pass.kind else { return px };
        let (upto, reduce) = (upto.min(self.layers), self.reduction(pass));
        for j in (0..upto).filter(|j| !self.in_run(pass, *j)) {
            let Some((l, _)) = self.layer(j, false) else { continue };
            let Some(d) = self.layer_draws(j, upto, false) else { continue };
            let kind = (crate::command::layer_wire::layer_key_kind(l.key) as usize).min(7);
            for i in 0..d.count() {
                if let Some(s) = d.draw(i) {
                    let s = reduce.map_or(s, |r| r.place(&s, pass.area));
                    if matches!(s.pipe, Pipe::Straight | Pipe::Rounded) {
                        px[kind] += drawn_px(&s, pass.area);
                    }
                }
            }
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
    pub(super) fn wallpaper_piece(&self, upto: usize, p: Px) -> DrawSpec {
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
    /// of a draw of `target` in a scene pass up to `upto` (`rest`: the layers at rest).
    pub(super) fn occluder(&self, target: Px, from: usize, upto: usize, rest: bool) -> Option<Px> {
        self.occluder_of(0, (target, from, upto), rest)
    }

    /// `occluder` among the layers of window `id` alone (`0`: of every layer).
    fn occluder_of(&self, id: u32, at: (Px, usize, usize), rest: bool) -> Option<Px> {
        let (target, from, upto) = at;
        let mut best: Option<Px> = None;
        for k in from..upto.min(self.layers) {
            let Some((l, cache)) = self.layer(k, rest) else { continue };
            if id != 0 && l.layer_id != id {
                continue;
            }
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

    /// Layer `j` as drawn, or at `rest` (as its present encoded it), and its glass's step.
    fn layer(&self, j: usize, rest: bool) -> Option<(FrameLayer, Option<CacheStep>)> {
        let s = self.frame.step(j)?;
        let l = if rest { self.frame.layers.rest(j)? } else { s.layer };
        Some((l, s.glass.map(|g| g.cache)))
    }

    /// The draws of layer `j` in a scene pass up to `upto`: its glass back, its shadow, its
    /// content — in pieces around the opaque layer above it, its back around the one of its own
    /// window (R-own, `pieces`). At `rest` (a group run's pass): the layer as its present
    /// encoded it, around the layers of its group at rest.
    pub(super) fn layer_draws(&self, j: usize, upto: usize, rest: bool) -> Option<LayerDraws> {
        let (step, (l, _)) = (self.frame.step(j)?, self.layer(j, rest)?);
        // The back from its rows, which hold the blur of the rest rectangle (`Glass.at`).
        let back = step.glass.and_then(|g| match g.cache {
            CacheStep::Read(row) | CacheStep::Write(row) => Some((row, (g.at.2, g.at.3))),
            CacheStep::Off => None,
        });
        let pad =
            if l.shadow_blur > 0 { l.shadow_blur + l.shadow_offset_y.unsigned_abs() } else { 0 };
        let extent = (
            l.dst_x.saturating_sub(pad),
            l.dst_y.saturating_sub(pad),
            l.width + 2 * pad,
            l.height + 2 * pad,
        );
        let occluder = self.occluder(extent, j + 1, upto, rest);
        // Asked for a glass with a window alone: no other layer's draws are cut around it.
        let own = occluder
            .filter(|_| back.is_some() && l.layer_id != 0)
            .and_then(|_| self.occluder_of(l.layer_id, (extent, j + 1, upto), rest));
        Some(LayerDraws::new(&l, back, self.content(&l), (occluder, own)))
    }

    /// The wallpaper's pieces and every layer's, in walk order.
    fn scene_draws(&self, upto: usize) -> usize {
        let (w, h) = self.display;
        let wall = occlude::count((0, 0, w, h), self.occluder((0, 0, w, h), 0, upto, false));
        let layer = |j| self.layer_draws(j, upto, false).map(|d| d.count());
        wall + (0..upto).filter_map(layer).sum::<usize>()
    }

    fn scene_draw(&self, upto: usize, mut i: usize) -> Option<DrawSpec> {
        let (w, h) = self.display;
        let occ = self.occluder((0, 0, w, h), 0, upto, false);
        let wall = occlude::count((0, 0, w, h), occ);
        if i < wall {
            return Some(self.wallpaper_piece(upto, occlude::piece((0, 0, w, h), occ, i)?));
        }
        i -= wall;
        for j in 0..upto {
            let Some(d) = self.layer_draws(j, upto, false) else { continue };
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
        // A layout band is shrunk to the layout's rows; a buffer's was checked against its
        // buffer when the present came in (RFC-0114 §6).
        let room =
            if l.source == 0 { self.layout_rows.saturating_sub(l.src_row_abs) } else { u32::MAX };
        let (h, bh) = (l.height.min(room), bh.min(room));
        Content {
            source: l.source,
            slot: l.slot,
            rect: (l.dst_x, l.dst_y, l.width, h),
            band: (l.src_x, l.src_row_abs, bw, bh),
            tint: [1.0, 1.0, 1.0, l.opacity.min(255) as f32 / 255.0],
            radius: l.corner_radius.min(l.width / 2).min(h / 2),
            solid: l.solid,
            opaque: l.opaque && l.opacity >= 255,
        }
    }

    /// The re-blur of written glass `j` placed at the scene row and the half row `at`, for
    /// the band the plan recorded (`chain::chain`) — of its REST rectangle (`Glass.at`),
    /// wherever its layer is drawn; `None` when it is not a write or the scratch or the half
    /// scratch cannot hold it there.
    fn chain_of(&self, j: usize, at: (u32, u32)) -> Option<Chain> {
        let step = self.frame.step(j)?;
        let g = step.glass?;
        let CacheStep::Write(row) = g.cache else { return None };
        let (dst_x, dst_y, width, height) = g.at;
        let l = FrameLayer { dst_x, dst_y, width, height, ..step.layer };
        let rows = (row, self.frame.glass.plane_rows());
        chain(&l, g.band, rows, at, self.display, self.half_rows)
    }

    /// Whether the plan writes layer `j`'s glass this frame.
    fn written(&self, j: usize) -> bool {
        matches!(self.frame.cache_step(j), Some(CacheStep::Write(_)))
    }

    /// The walk's index of write `w`: the `w`-th glass the plan writes.
    fn write_at(&self, w: usize) -> Option<usize> {
        (0..self.layers).filter(|j| self.written(*j)).nth(w)
    }

    /// The re-blur of write `w`, at its wave's place in the scratch and the half scratch.
    fn chain_at(&self, w: usize) -> Option<Chain> {
        self.chain_of(self.write_at(w)?, self.waves.rows(w))
    }
}
