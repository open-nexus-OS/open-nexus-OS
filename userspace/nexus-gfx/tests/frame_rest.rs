// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A glass drawn from the rows of its REST rectangle on the board (RFC-0109, the executors' half
//! of the hold): T11 — a write for a band, after a whole write, leaves the rows a whole write of
//! the changed scene would (a host model runs the chain's three passes as the board's GPU does:
//! the quads' mapping, the library's taps, bilinear fetches clamped to the texture's edge — and
//! every texel no pass drew this frame stale, so a read of it shows); T12 — a glass drawn at any
//! size, written or read, cut or clipped, samples only its own slot's result.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::chain::PassArea;
use nexus_gfx::frame::{
    blur_radius, blur_scale, clip_draw, Area, Board, CacheStep, DrawSpec, Frame, FrameLayer,
    Gaussian, Glass, PassKind, PassSpec, Pipe, Target, Tex,
};

const DISPLAY: (u32, u32) = (1920, 1080);
const LAYOUT_ROWS: u32 = 14_040;
const HALF_ROWS: u32 = 600;
const W: usize = DISPLAY.0 as usize;
const H: usize = DISPLAY.1 as usize;

fn layer(f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer { src_row_abs: 5400, opacity: 255, ..FrameLayer::default() };
    f(&mut l);
    Command::CompositeLayer {
        src_row_abs: l.src_row_abs,
        src_x: l.src_x,
        width: l.width,
        height: l.height,
        dst_x: l.dst_x,
        dst_y: l.dst_y,
        opacity: l.opacity,
        corner_radius: l.corner_radius,
        shadow_blur: l.shadow_blur,
        shadow_offset_y: l.shadow_offset_y,
        shadow_alpha: l.shadow_alpha,
        backdrop_blur: l.backdrop_blur,
        content_w: l.content_w,
        content_h: l.content_h,
        scroll_band_top_abs: l.scroll_band_top_abs,
        scroll_band_h: l.scroll_band_h,
        layer_id: l.layer_id,
        content_epoch: l.content_epoch,
        flags: nexus_gfx::command::layer_wire::with_slot(
            if l.opaque { nexus_gfx::command::buffer::LAYER_OPAQUE } else { 0 },
            l.slot,
        ),
        key: l.key,
        source: l.source,
    }
}

/// The window below everything (id 1) and a glass (id 2) at `at` blurring `blur`.
fn scene(at: (u32, u32, u32, u32), blur: u32, radius: u32) -> [Command; 2] {
    let window = layer(|l| {
        (l.dst_x, l.dst_y, l.width, l.height) = (0, 0, DISPLAY.0, DISPLAY.1);
        (l.layer_id, l.key) = (1, 1 << 16);
    });
    let glass = layer(|l| {
        (l.dst_x, l.dst_y, l.width, l.height) = at;
        (l.backdrop_blur, l.corner_radius, l.layer_id, l.key) = (blur, radius, 2, 2 << 16);
    });
    [window, glass]
}

fn ready() -> Frame {
    let mut f = Frame::new(2048);
    f.glass.ready = true;
    f
}

fn present(f: &mut Frame, cmds: &[Command]) -> nexus_gfx::frame::Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &|_| true)
}

fn glass_of(f: &Frame, i: usize) -> Glass {
    f.step(i).and_then(|s| s.glass).expect("a glass step")
}

/// The chain of the frame's one write: its scene, horizontal and vertical passes.
fn chain(f: &Frame, p: nexus_gfx::frame::Planned) -> [PassSpec; 3] {
    let b = Board::new(f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(b.pass_count(), 4, "one write: three chain passes and the slot's");
    [0, 1, 2].map(|i| b.pass(i).expect("a chain pass"))
}

/// What a texel holds where no pass drew it this frame: any read of it shows in the rows.
const STALE: f32 = 1000.0;

/// The board's three targets, modelled: the scratch and the half scratch (stale at every
/// frame's start) and the first cache plane (kept).
struct Gpu {
    scratch: Vec<f32>,
    half: Vec<f32>,
    cache: Vec<f32>,
}

impl Gpu {
    fn new(cache_rows: usize) -> Self {
        Self {
            scratch: vec![STALE; W * H],
            half: vec![STALE; W * HALF_ROWS as usize],
            cache: vec![STALE; W * cache_rows],
        }
    }

    /// One chain run over `scene` (a pixel's value).
    fn run(&mut self, passes: &[PassSpec; 3], scene: &dyn Fn(u32, u32) -> f32) {
        self.scratch.fill(STALE);
        self.half.fill(STALE);
        let (x0, y0, x1, y1) = passes[0].area;
        assert_eq!(passes[0].target, Target::Scratch);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.scratch[y as usize * W + x as usize] = scene(x, y);
            }
        }
        let (PassKind::One(h), PassKind::One(v)) = (passes[1].kind, passes[2].kind) else {
            panic!("one write runs its blurs one draw each");
        };
        assert_eq!((passes[1].target, passes[2].target), (Target::Half, Target::Cache(0)));
        blur(&self.scratch, (W, H), &mut self.half, passes[1].area, h);
        blur(&self.half, (W, HALF_ROWS as usize), &mut self.cache, passes[2].area, v);
    }
}

/// The texture `src` (`w` x `h`) sampled at `(x, y)` in texels as the GPU does: bilinear,
/// clamped to its edge, the fraction held to eight bits.
fn bilinear(src: &[f32], (w, h): (usize, usize), x: f64, y: f64) -> f32 {
    let axis = |c: f64, n: usize| {
        let c = c - 0.5;
        let (i, f) = (c.floor(), ((c - c.floor()) * 256.0).round() / 256.0);
        let (i, f) = if f >= 1.0 { (i + 1.0, 0.0) } else { (i, f) };
        let at = |k: f64| (k.max(0.0) as usize).min(n - 1);
        (at(i), at(i + 1.0), f as f32)
    };
    let ((x0, x1, fx), (y0, y1, fy)) = (axis(x, w), axis(y, h));
    let row = |y: usize| {
        let (a, b) = (src[y * w + x0], src[y * w + x1]);
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

/// The blur draw `d` run over the texels of `area` its quad covers, reading `src` (`dims`) into
/// `dst` (the display's width a row).
fn blur(src: &[f32], dims: (usize, usize), dst: &mut [f32], area: PassArea, d: DrawSpec) {
    let (Pipe::BlurH(r) | Pipe::BlurV(r)) = d.pipe else { panic!("a blur draw") };
    let weights = Gaussian::library(r).weights();
    let (q, (tw, th)) = (d.quad, (dims.0 as f64, dims.1 as f64));
    let p = |i: usize| f64::from(q.push[i]);
    let (x1, y1) = ((q.x + q.w) as u32, (q.y + q.h) as u32);
    for ty in area.1.max(q.y as u32)..=area.3.min(y1 - 1) {
        for tx in area.0.max(q.x as u32)..=area.2.min(x1 - 1) {
            let u = (f64::from(tx) + 0.5 - f64::from(q.x)) / f64::from(q.w);
            let v = (f64::from(ty) + 0.5 - f64::from(q.y)) / f64::from(q.h);
            let (sx, sy) = ((p(2) + u * p(0)) * tw, (p(3) + v * p(1)) * th);
            let mut acc = 0.0f32;
            for (j, w) in weights.iter() {
                let j = f64::from(j);
                acc += w * bilinear(src, dims, sx + j * p(4) * tw, sy + j * p(5) * th);
            }
            dst[ty as usize * W + tx as usize] = acc;
        }
    }
}

/// A scene's pixel before the change: values in `[0, 1)` that differ from pixel to pixel.
fn before(x: u32, y: u32) -> f32 {
    ((x.wrapping_mul(7919) ^ y.wrapping_mul(104_729)) % 997) as f32 / 997.0
}

#[test]
fn a_band_write_after_a_whole_write_leaves_the_rows_a_whole_write_would() {
    // T11: per radius (two resolutions, odd and even taps) and per glass (inside the display, at
    // its top-left edge, at its bottom-right edge), changes inside the glass, across its edge,
    // at its corner and outside it within the reach.
    let glasses: [(u32, u32, u32, u32); 3] =
        [(300, 200, 400, 200), (0, 0, 640, 40), (1700, 900, 220, 180)];
    let mut checked = 0;
    for blur_at in [8, 20, 30, 40] {
        let (r, s) = (blur_radius(blur_at), blur_scale(blur_radius(blur_at)));
        for at in glasses {
            let (x, y, w, h) = at;
            let changes = [
                (x + w / 2, y + h / 3, 20, 10),
                (x.saturating_sub(3), y + h / 2, 9, 7),
                // Four pixels a side: a quarter-resolution fetch reads two of every four.
                (x + w - 4, y + h - 4, 4, 4),
                ((x + w + r / 2).min(DISPLAY.0 - 5), y + 1, 4, 4),
            ];
            for (cx, cy, cw, ch) in changes {
                let mut f = ready();
                let whole = present(&mut f, &scene(at, blur_at, 12));
                let row = match glass_of(&f, 1).cache {
                    CacheStep::Write(row) => row,
                    other => panic!("the first frame writes the glass: {other:?}"),
                };
                let whole = chain(&f, whole);
                let change = Command::Damage {
                    rect: TileRect { x: cx, y: cy, width: cw, height: ch },
                    origin: 0,
                };
                let cmds = scene(at, blur_at, 12);
                let band = present(&mut f, &[cmds[0].clone(), cmds[1].clone(), change]);
                let g = glass_of(&f, 1);
                assert_eq!(g.cache, CacheStep::Write(row), "r{r} {at:?} {cx},{cy}: a write");
                assert!(matches!(g.band, Area::Rect { .. }), "r{r} {at:?}: a band, {:?}", g.band);
                let band = chain(&f, band);
                let inside =
                    |px: u32, py: u32| px >= cx && px < cx + cw && py >= cy && py < cy + ch;
                let after = |px: u32, py: u32| {
                    if inside(px, py) {
                        1.0 - before(px, py)
                    } else {
                        before(px, py)
                    }
                };
                let (_, _, c1, v1) = whole[2].area;
                // The rows as the board leaves them: the whole write, then the band's.
                let mut board = Gpu::new(v1 as usize + 1);
                board.run(&whole, &before);
                let old = board.cache.clone();
                board.run(&band, &after);
                // The rows a whole write of the changed scene leaves.
                let mut truth = Gpu::new(v1 as usize + 1);
                truth.run(&whole, &after);
                let (mut moved, mut worst) = (0, (0.0f32, 0, 0));
                for v in row..=v1 {
                    for c in 0..=c1 {
                        let k = v as usize * W + c as usize;
                        assert!(
                            truth.cache[k] < 2.0,
                            "r{r} {at:?}: a whole write read a stale texel"
                        );
                        moved += usize::from(old[k] != truth.cache[k]);
                        let d = (board.cache[k] - truth.cache[k]).abs();
                        if d > worst.0 {
                            worst = (d, c, v - row);
                        }
                    }
                }
                assert!(
                    worst.0 < 1e-4,
                    "r{r} s{s} glass {at:?} change {:?}: texel {:?} off by {}",
                    (cx, cy, cw, ch),
                    (worst.1, worst.2),
                    worst.0
                );
                // Not vacuous: the change reaches the rows, and nothing stale is in them.
                assert!(moved > 0, "r{r} {at:?} {cx},{cy}: the change reaches the glass");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 4 * 3 * 4);
}

/// A tiny deterministic generator: `next(n)` in `0..n`.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 32) % u64::from(n.max(1))) as u32
    }
}

/// Every cache texture of the slot's pass inside the result of `g`'s slot: columns `[1, 1 +
/// at.w.div_ceil(s))`, rows `[row + 1, row + 1 + at.h.div_ceil(s))`, never empty — also cut to
/// a pass area. The cache textures' quads, summed.
fn check_back(f: &Frame, p: nexus_gfx::frame::Planned, g: Glass, s: u32, cut: PassArea) -> f32 {
    let row = match g.cache {
        CacheStep::Read(row) | CacheStep::Write(row) => row,
        CacheStep::Off => return 0.0,
    };
    let (n, m) = (g.at.2.div_ceil(s), g.at.3.div_ceil(s));
    let inside = |t: Tex| match t {
        Tex::Cache { x, row: r, w, h } => {
            w >= 1 && h >= 1 && x >= 1 && x + w <= 1 + n && r > row && r + h <= row + 1 + m
        }
        _ => true,
    };
    let b = Board::new(f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let slot = b.pass(b.pass_count() - 1).expect("the slot's pass");
    let mut area = 0.0;
    for d in b.walk(&slot) {
        if !matches!(d.tex, Tex::Cache { .. }) {
            continue;
        }
        assert!(inside(d.tex), "at {:?} drawn elsewhere: {:?} past the slot's rows", g.at, d.tex);
        if let Some(c) = clip_draw(&d, cut) {
            assert!(inside(c.tex), "cut to {cut:?}: {:?} past the slot's rows", c.tex);
        }
        area += d.quad.w * d.quad.h;
    }
    area
}

#[test]
fn test_reject_sampling_past_a_glass_s_rows() {
    // T12: random rest rectangles drawn scaled (50 %–150 %, a quarter at rest) and moved, under
    // an opaque layer a third of the time: born in motion (written), then held (read).
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for case in 0..400 {
        let (w, h) = (1 + rng.next(600), 1 + rng.next(400));
        let at = (rng.next(DISPLAY.0 - w), rng.next(DISPLAY.1 - h), w, h);
        let blur_at = [8, 20, 30, 40][rng.next(4) as usize];
        let s = blur_scale(blur_radius(blur_at));
        let motion = |rng: &mut Rng| Command::LayerTransform {
            layer_id: 2,
            dx: rng.next(201) as i32 - 100,
            dy: rng.next(201) as i32 - 100,
            opacity: 255,
            scale: if rng.next(4) == 0 { 10_000 } else { 5_000 + rng.next(10_001) },
            origin_x: rng.next(DISPLAY.0) as i32,
            origin_y: rng.next(DISPLAY.1) as i32,
        };
        let mut cmds = vec![motion(&mut rng)];
        cmds.extend(scene(at, blur_at, rng.next(60)));
        let covered = rng.next(3) == 0;
        if covered {
            let (cx, cy) = (rng.next(DISPLAY.0 - 200), rng.next(DISPLAY.1 - 150));
            cmds.push(layer(|l| {
                (l.dst_x, l.dst_y, l.width, l.height) = (cx, cy, 200, 150);
                (l.opaque, l.layer_id, l.key) = (true, 3, 3 << 16);
            }));
        }
        let mut f = ready();
        let cut = (rng.next(DISPLAY.0), rng.next(DISPLAY.1), DISPLAY.0 - 1, DISPLAY.1 - 1);
        let p = present(&mut f, &cmds);
        let g = glass_of(&f, 1);
        assert_eq!(g.at, at, "case {case}: the rows are the rest rectangle's");
        let drawn = f.step(1).expect("the glass").layer;
        let area = check_back(&f, p, g, s, cut);
        if !covered {
            let want = drawn.width as f32 * drawn.height as f32;
            assert_eq!(area, want, "case {case}: the back covers the glass as drawn");
        }
        // A write blurs the rest rectangle wherever the glass is drawn.
        if let CacheStep::Write(row) = g.cache {
            let passes = chain(&f, p);
            let (vw, vh) = (w.div_ceil(s) + 2, h.div_ceil(s) + 2);
            let r0 = row % f.glass.plane_rows();
            assert_eq!(passes[2].area, (0, r0, vw - 1, r0 + vh - 1), "case {case}");
            let reach = blur_radius(blur_at) / s * s + s;
            assert_eq!(passes[0].area.0, at.0.saturating_sub(reach), "case {case}");
        }
        // Held at another size: read from the same rows.
        let p = present(
            &mut f,
            &[motion(&mut rng), Command::Damage { rect: TileRect::NONE, origin: 0 }],
        );
        let g = glass_of(&f, 1);
        check_back(&f, p, g, s, cut);
    }
}
