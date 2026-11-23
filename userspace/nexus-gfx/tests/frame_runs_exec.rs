// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's passes run on the host in exact arithmetic, for `frame_runs.rs`: every pass of
//! a `Board` — the glass chain, the runs' own passes, the slot's — its draws walked and cut as
//! the driver does (`Board::walk`, `clip_draw`), each drawn into the pass's target with the
//! library's blends (`layer.wgsl` and the pipelines' state): the opaque program replaces with
//! the texel as it is; the straight and the rounded one lay the texel, premultiplied by its
//! alpha times the tint's, over the target on every channel; the premultiplied one lays the
//! texel times the tint over it. A texel is a colour and an alpha, each a number modulo a
//! prime: the blends are polynomials, so two pictures that are equal here are equal as
//! formulas — whatever the order of the blends, with no rounding to hide a difference or to
//! make one (an eight-bit target rounds each blend, which is the executor's, not the plan's).
//! A pass clears its area to texels that are not transparent (the board's clear colour), the
//! scratches are stale at a frame's start, a run's plane keeps what was drawn into it and its
//! rows past the display stay transparent. A blur is three taps of its quad's mapping (enough
//! for a result to depend on what its source holds); a shadow and a corner's coverage are not
//! drawn.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::frame::{
    clip_draw, run_plane_rows, Board, DrawSpec, PassSpec, Pipe, Target, Tex, RUN_TARGETS,
};

pub const DISPLAY: (u32, u32) = (192, 128);
pub const HALF_ROWS: u32 = 128;
pub const CACHE_ROWS: u32 = 2048;
pub const LAYOUT_ROWS: u32 = 1 << 20;
const W: usize = DISPLAY.0 as usize;

/// The field: numbers modulo 2^61 − 1.
pub const P: u64 = (1 << 61) - 1;
/// A texel: its colour (premultiplied in a target, straight in a surface) and its alpha.
pub type Texel = [u64; 2];
/// What a pass's clear leaves, and what a scratch holds before a frame draws into it.
const CLEARED: Texel = [0x0123_4567_89AB, 0x0FED_CBA9_8765];
const STALE: Texel = [0x0AAA_5555_AAAA, 0x0555_AAAA_5555];

pub fn mul(a: u64, b: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) % u128::from(P)) as u64
}

pub fn add(a: u64, b: u64) -> u64 {
    (a + b) % P
}

pub fn sub(a: u64, b: u64) -> u64 {
    (a + P - b) % P
}

/// `n / d` in the field.
pub fn frac(n: u64, d: u64) -> u64 {
    let (mut inv, mut base, mut e) = (1, d % P, P - 2);
    while e > 0 {
        if e & 1 == 1 {
            inv = mul(inv, base);
        }
        base = mul(base, base);
        e >>= 1;
    }
    mul(n % P, inv)
}

/// `src` (premultiplied) over `dst`: `src + dst·(1 − a)` on both channels.
fn over(src: Texel, dst: Texel) -> Texel {
    let keep = sub(1, src[1]);
    [add(src[0], mul(dst[0], keep)), add(src[1], mul(dst[1], keep))]
}

/// The board's targets.
pub struct Gpu {
    pub slot: Vec<Texel>,
    scratch: Vec<Texel>,
    half: Vec<Texel>,
    cache: Vec<Texel>,
    runs: [Vec<Texel>; RUN_TARGETS],
    /// 1 / 256 and 1 / 255: a bilinear fraction's and a tint's unit.
    units: (u64, u64),
}

impl Gpu {
    pub fn new() -> Self {
        let plane = |rows: u32, fill: Texel| vec![fill; W * rows as usize];
        let run = || plane(run_plane_rows(DISPLAY.1), [0, 0]);
        Self {
            slot: plane(DISPLAY.1, STALE),
            scratch: plane(DISPLAY.1, STALE),
            half: plane(HALF_ROWS, STALE),
            cache: plane(CACHE_ROWS, STALE),
            runs: [run(), run()],
            units: (frac(1, 256), frac(1, 255)),
        }
    }

    /// A frame starts: what the last one left in the scratches is stale.
    pub fn begin(&mut self) {
        self.scratch.fill(STALE);
        self.half.fill(STALE);
    }

    fn target(&mut self, to: Target) -> &mut Vec<Texel> {
        match to {
            Target::Slot => &mut self.slot,
            Target::Scratch => &mut self.scratch,
            Target::Half => &mut self.half,
            Target::Cache(0) => &mut self.cache,
            Target::Run(slot) => &mut self.runs[usize::from(slot)],
            Target::Cache(plane) => panic!("the model holds one cache plane, not plane {plane}"),
        }
    }

    /// `tex`'s size in texels.
    fn size(tex: Tex) -> (u32, u32) {
        match tex {
            Tex::None => (1, 1),
            Tex::Half { rows } => (DISPLAY.0, rows),
            Tex::Layout { w, h, .. }
            | Tex::Scratch { w, h, .. }
            | Tex::Cache { w, h, .. }
            | Tex::Run { w, h, .. }
            | Tex::Buffer { w, h, .. } => (w, h),
        }
    }

    /// `tex` read at `(x, y)` texels from its first as the GPU does: bilinear, clamped to its
    /// own rectangle, the fraction held to eight bits. `layout`: the shared layout's content.
    fn fetch(&self, tex: Tex, (x, y): (f64, f64), layout: &dyn Fn(u32, u32) -> Texel) -> Texel {
        let plane = |buf: &[Texel], x0: u32, row0: u32, i: i64, j: i64| {
            buf[(i64::from(row0) + j) as usize * W + (i64::from(x0) + i) as usize]
        };
        let get = |i: i64, j: i64| match tex {
            Tex::None => [0, 0],
            Tex::Layout { x: x0, row, .. } => layout(x0 + i as u32, row + j as u32),
            Tex::Scratch { x: x0, row, .. } => plane(&self.scratch, x0, row, i, j),
            Tex::Half { .. } => plane(&self.half, 0, 0, i, j),
            Tex::Cache { x: x0, row, .. } => plane(&self.cache, x0, row, i, j),
            Tex::Run { slot, x: x0, row, .. } => {
                plane(&self.runs[usize::from(slot)], x0, row, i, j)
            }
            Tex::Buffer { .. } => panic!("the model holds no surface buffer"),
        };
        let (w, h) = Self::size(tex);
        // The board's sampler clamps at the rectangle (R-own: a glass back is cut only around
        // its own window's layers, so the frames with and without a run cut alike).
        let past = 0i64;
        let axis = |c: f64, n: u32| {
            let c = c - 0.5;
            let (i, f) = (c.floor() as i64, ((c - c.floor()) * 256.0).round() as u64);
            let (i, f) = if f >= 256 { (i + 1, 0) } else { (i, f) };
            let at = |k: i64| k.clamp(-past, i64::from(n.max(1)) - 1 + past);
            (at(i), at(i + 1), f)
        };
        let ((x0, x1, fx), (y0, y1, fy)) = (axis(x, w), axis(y, h));
        let mix = |a: Texel, b: Texel, f: u64| {
            let t = mul(f, self.units.0);
            [0, 1].map(|c| add(mul(a[c], sub(1, t)), mul(b[c], t)))
        };
        let row = |y: i64| if fx == 0 { get(x0, y) } else { mix(get(x0, y), get(x1, y), fx) };
        if fy == 0 {
            row(y0)
        } else {
            mix(row(y0), row(y1), fy)
        }
    }

    /// A blur at `uv` of `q`: its quad's mapping, a quarter of the tap before, half of the
    /// centre, a quarter of the tap behind.
    fn blurred(&self, q: &Quad, tex: Tex, (u, v): (f64, f64)) -> Texel {
        let (tw, th) = Self::size(tex);
        let p = |i: usize| f64::from(q.push[i]);
        let none = |_: u32, _: u32| STALE;
        let at = |d: f64| {
            let x = (p(2) + u * p(0) + d * p(4)) * f64::from(tw);
            let y = (p(3) + v * p(1) + d * p(5)) * f64::from(th);
            self.fetch(tex, (x, y), &none)
        };
        let (a, b, c, quarter) = (at(-1.0), at(0.0), at(1.0), frac(1, 4));
        [0, 1].map(|k| mul(add(add(a[k], c[k]), add(b[k], b[k])), quarter))
    }

    /// One pass of `b`: its area cleared, its draws walked, cut to the area and drawn in
    /// order — each as `alter` hands it on (the identity for the board's own frame).
    pub fn pass(
        &mut self,
        b: &Board<'_>,
        spec: &PassSpec,
        layout: &dyn Fn(u32, u32) -> Texel,
        alter: &dyn Fn(DrawSpec) -> DrawSpec,
    ) {
        let (ax0, ay0, ax1, ay1) = spec.area;
        let dst = self.target(spec.target);
        for (x, y) in (ay0..=ay1).flat_map(|y| (ax0..=ax1).map(move |x| (x, y))) {
            dst[y as usize * W + x as usize] = CLEARED;
        }
        let draws: Vec<DrawSpec> = b.walk(spec).filter_map(|d| clip_draw(&d, spec.area)).collect();
        for d in draws {
            self.draw(spec, &alter(d), layout);
        }
    }

    fn draw(&mut self, spec: &PassSpec, d: &DrawSpec, layout: &dyn Fn(u32, u32) -> Texel) {
        let (q, f) = (d.quad, |v: f32| f64::from(v));
        let (ax0, ay0, ax1, ay1) = spec.area;
        if d.pipe == Pipe::Shadow || q.w <= 0.0 || q.h <= 0.0 {
            return;
        }
        let tint = |i: usize| mul((q.push[i] * 255.0).round() as u64, self.units.1);
        let (tint, (tw, th)) = ([tint(0), tint(3)], Self::size(d.tex));
        // The pixels of the area whose centre the quad covers.
        let first = |lo: f32, a: u32| ((f(lo) - 0.5).ceil().max(0.0) as u32).max(a);
        let past = |hi: f32, a: u32| ((f(hi) - 0.5).ceil().max(0.0) as u32).min(a + 1);
        for py in first(q.y, ay0)..past(q.y + q.h, ay1) {
            for px in first(q.x, ax0)..past(q.x + q.w, ax1) {
                let uv = (
                    (f64::from(px) + 0.5 - f(q.x)) / f(q.w),
                    (f64::from(py) + 0.5 - f(q.y)) / f(q.h),
                );
                let src = match d.pipe {
                    Pipe::Blur(_) => self.blurred(&q, d.tex, uv),
                    _ => self.fetch(d.tex, (uv.0 * f64::from(tw), uv.1 * f64::from(th)), layout),
                };
                let k = py as usize * W + px as usize;
                let dst = self.target(spec.target);
                dst[k] = match d.pipe {
                    Pipe::Opaque | Pipe::Blur(_) => src,
                    Pipe::Straight | Pipe::Rounded => {
                        let a = mul(src[1], tint[1]);
                        over([mul(src[0], a), a], dst[k])
                    }
                    Pipe::Premultiplied => {
                        over([mul(src[0], tint[0]), mul(src[1], tint[1])], dst[k])
                    }
                    Pipe::Shadow => dst[k],
                };
            }
        }
    }
}
