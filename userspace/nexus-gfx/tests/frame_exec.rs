// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's passes run on the host, shared by `frame_rest.rs`, `frame_reduced_scene.rs` and
//! `frame_waves.rs`: a pass's draws walked and cut as the driver does (`Board::walk`,
//! `clip_draw`), each drawn into the pass's area as the board's GPU does — a pixel whose centre
//! the quad covers, the quad's mapping, bilinear fetches clamped to the texture's own rectangle
//! with the fraction held to eight bits, the blur the kernel table's paired program
//! (`blur.wgsl`'s `vertical`: the centre, then two taps a fetch placed by their weights). One
//! value a texel. The scratch and the half scratch are stale at a frame's start, so a read of
//! a texel no pass drew this frame shows in whatever is computed from it. The opaque program
//! replaces; the blending ones mix half and half with what lies below (enough for a layer
//! above to depend on the one below); a shadow is not drawn.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::frame::{clip_draw, Board, DrawSpec, Gaussian, PassSpec, Pipe, Target, Tex};

pub const DISPLAY: (u32, u32) = (1920, 1080);
pub const HALF_ROWS: u32 = 600;
pub const W: usize = DISPLAY.0 as usize;
const H: usize = DISPLAY.1 as usize;

/// What a texel holds where no pass drew it this frame: any read of it shows.
pub const STALE: f32 = 1000.0;

/// The board's targets, modelled: the scratch, the half scratch and the first cache plane.
pub struct Gpu {
    pub scratch: Vec<f32>,
    pub half: Vec<f32>,
    pub cache: Vec<f32>,
}

/// `get` (a texture of `w` x `h` texels) sampled at `(x, y)` in texels as the GPU does:
/// bilinear, clamped to its edge, the fraction held to eight bits.
fn bilinear(get: &dyn Fn(u32, u32) -> f32, (w, h): (u32, u32), x: f64, y: f64) -> f32 {
    let axis = |c: f64, n: u32| {
        let c = c - 0.5;
        let (i, f) = (c.floor(), ((c - c.floor()) * 256.0).round() / 256.0);
        let (i, f) = if f >= 1.0 { (i + 1.0, 0.0) } else { (i, f) };
        let at = |k: f64| (k.max(0.0) as u32).min(n.max(1) - 1);
        (at(i), at(i + 1.0), f as f32)
    };
    let ((x0, x1, fx), (y0, y1, fy)) = (axis(x, w), axis(y, h));
    let row = |y: u32| {
        let (a, b) = (get(x0, y), get(x1, y));
        if fx == 0.0 {
            a
        } else {
            a * (1.0 - fx) + b * fx
        }
    };
    if fy == 0.0 {
        row(y0)
    } else {
        row(y0) * (1.0 - fy) + row(y1) * fy
    }
}

impl Gpu {
    pub fn new(cache_rows: usize) -> Self {
        Self {
            scratch: vec![STALE; W * H],
            half: vec![STALE; W * HALF_ROWS as usize],
            cache: vec![STALE; W * cache_rows],
        }
    }

    /// A frame starts: what the last one left in the scratches is stale.
    pub fn begin(&mut self) {
        self.scratch.fill(STALE);
        self.half.fill(STALE);
    }

    /// `tex` (its size in texels) read at `(x, y)` texels from its first; `layout` is the
    /// shared layout's content (the wallpaper plane, the atlas).
    pub fn fetch(&self, tex: Tex, (x, y): (f64, f64), layout: &dyn Fn(u32, u32) -> f32) -> f32 {
        let plane = |buf: &'_ [f32], x0: u32, row0: u32, i: u32, j: u32| {
            buf[(row0 + j) as usize * W + (x0 + i) as usize]
        };
        match tex {
            Tex::None => 0.0,
            Tex::Layout { x: x0, row, w, h } => {
                bilinear(&|i, j| layout(x0 + i, row + j), (w, h), x, y)
            }
            Tex::Scratch { x: x0, row, w, h } => {
                bilinear(&|i, j| plane(&self.scratch, x0, row, i, j), (w, h), x, y)
            }
            Tex::Half { rows } => {
                bilinear(&|i, j| plane(&self.half, 0, 0, i, j), (W as u32, rows), x, y)
            }
            Tex::Cache { x: x0, row, w, h } => {
                bilinear(&|i, j| plane(&self.cache, x0, row, i, j), (w, h), x, y)
            }
            Tex::Buffer { .. } => panic!("the model holds no surface buffer"),
            Tex::Run { .. } => panic!("the model takes no run"),
        }
    }

    /// The paired blur at radius `r` at `uv` of `q`: the centre, then the taps two a fetch.
    fn blurred(&self, r: u32, q: &Quad, tex: Tex, (u, v): (f64, f64)) -> f32 {
        let (tw, th) = match tex {
            Tex::Scratch { w, h, .. } => (f64::from(w), f64::from(h)),
            Tex::Half { rows } => (W as f64, f64::from(rows)),
            other => panic!("a blur reads the scratch or the half scratch, not {other:?}"),
        };
        let none = |_: u32, _: u32| STALE;
        let p = |i: usize| f64::from(q.push[i]);
        let at = |d: f64| {
            let (x, y) = ((p(2) + u * p(0) + d * p(4)) * tw, (p(3) + v * p(1) + d * p(5)) * th);
            self.fetch(tex, (x, y), &none)
        };
        let weights = Gaussian::library(r).weights();
        let taps = weights.taps() as i32;
        let mut sum = weights.at(0) * at(0.0);
        let mut first = 0.0f32;
        for j in 1..=taps {
            let w = weights.at(j);
            let (pair, d) = if j % 2 == 1 {
                first = w;
                if j == taps {
                    (w, f64::from(j))
                } else {
                    (0.0, 0.0)
                }
            } else {
                (first + w, f64::from(j - 1) + f64::from(w / (first + w)))
            };
            if pair > 0.0 {
                sum += pair * at(d);
                sum += pair * at(-d);
            }
        }
        sum
    }

    /// One pass of `b`: its draws walked, cut to its area and drawn in order. A pass clears
    /// its area: a texel of it no draw covered holds nothing afterwards (stale).
    pub fn pass(&mut self, b: &Board<'_>, spec: &PassSpec, layout: &dyn Fn(u32, u32) -> f32) {
        let draws: Vec<DrawSpec> = b.walk(spec).filter_map(|d| clip_draw(&d, spec.area)).collect();
        let (ax0, ay0, ax1, ay1) = spec.area;
        let across = (ax1 + 1 - ax0) as usize;
        let mut drawn = vec![false; across * (ay1 + 1 - ay0) as usize];
        for d in draws {
            self.draw(spec, &d, layout, &mut drawn);
        }
        let dst = self.target(spec.target);
        for (k, _) in drawn.iter().enumerate().filter(|(_, d)| !**d) {
            dst[(ay0 as usize + k / across) * W + ax0 as usize + k % across] = STALE;
        }
    }

    fn target(&mut self, to: Target) -> &mut Vec<f32> {
        match to {
            Target::Scratch => &mut self.scratch,
            Target::Half => &mut self.half,
            Target::Cache(0) => &mut self.cache,
            other => panic!("the model draws no pass into {other:?}"),
        }
    }

    fn draw(
        &mut self,
        spec: &PassSpec,
        d: &DrawSpec,
        layout: &dyn Fn(u32, u32) -> f32,
        drawn: &mut [bool],
    ) {
        let (q, f) = (d.quad, |v: f32| f64::from(v));
        let (ax0, ay0, ax1, ay1) = spec.area;
        if d.pipe == Pipe::Shadow || q.w <= 0.0 || q.h <= 0.0 {
            return;
        }
        // The pixels of the area whose centre the quad covers (an edge's centre: the left and
        // the top one in, the right and the bottom one out).
        let first = |lo: f32, a: u32| ((f(lo) - 0.5).ceil().max(0.0) as u32).max(a);
        let past = |hi: f32, a: u32| ((f(hi) - 0.5).ceil().max(0.0) as u32).min(a + 1);
        for py in first(q.y, ay0)..past(q.y + q.h, ay1) {
            for px in first(q.x, ax0)..past(q.x + q.w, ax1) {
                let uv = (
                    (f64::from(px) + 0.5 - f(q.x)) / f(q.w),
                    (f64::from(py) + 0.5 - f(q.y)) / f(q.h),
                );
                let value = match (d.pipe, d.tex) {
                    (Pipe::Blur(r), tex) => self.blurred(r, &q, tex, uv),
                    (_, Tex::Layout { w, h, .. } | Tex::Cache { w, h, .. }) => {
                        let at = (uv.0 * f64::from(w), uv.1 * f64::from(h));
                        self.fetch(d.tex, at, layout)
                    }
                    (_, other) => panic!("a layer samples the layout or the cache, not {other:?}"),
                };
                let k = py as usize * W + px as usize;
                drawn[(py - ay0) as usize * (ax1 + 1 - ax0) as usize + (px - ax0) as usize] = true;
                let dst = self.target(spec.target);
                dst[k] = match d.pipe {
                    Pipe::Straight | Pipe::Rounded => 0.5 * value + 0.5 * dst[k],
                    _ => value,
                };
            }
        }
    }
}
