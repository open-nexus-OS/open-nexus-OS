// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The scene below a written glass at the blur's resolution (TASK-0351 P19), against pixels —
//! the board's passes run on the host (`frame_exec.rs`): a scene texel is the mean of the four
//! pixels at the point its tap stands for; a tap past the display's edge reads the scene's own
//! edge texel and never one no pass drew; a band's scene holds every texel its horizontal pass
//! reads; the rows follow the full-resolution gaussian within the oracle's tolerance. One
//! bound is stated, not hidden: a layer's straight edge that falls exactly on a tap's point is
//! the pixels of the side the point's own pixel lies on, where the full-resolution scene mixed
//! both sides half and half.

#[path = "frame_exec.rs"]
mod exec;
#[path = "frame_support.rs"]
mod support;

use exec::{Gpu, HALF_ROWS, STALE, W};
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::oracle::{backdrop, tolerance, GLASS, READS};
use nexus_gfx::frame::{
    blur_radius, blur_scale, Area, Board, CacheStep, Frame, Gaussian, PassSpec, Target, Tex,
    BLUR_RADII,
};
use support::{cache_of, damage, every_blur, glass, layer, ready_frame, with, DISPLAY};

const LAYOUT_ROWS: u32 = 14_040;
const ATLAS: u32 = 5400;

/// The layout's content: a value in `[0, 1)` that differs from texel to texel.
fn tex(x: u32, row: u32) -> f32 {
    ((x.wrapping_mul(7919) ^ row.wrapping_mul(104_729)) % 997) as f32 / 997.0
}

fn planned(f: &mut Frame, cmds: &[Command]) -> nexus_gfx::frame::Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &every_blur)
}

/// An opaque window showing the atlas one to one.
fn window(x: u32, y: u32, w: u32, h: u32) -> Command {
    with(layer(x, y, w, h), |l| (l.opaque, l.layer_id, l.key) = (true, 1, 1 << 16))
}

fn pane(at: (u32, u32, u32, u32), blur: u32) -> Command {
    with(glass(layer(at.0, at.1, at.2, at.3), blur), |l| (l.layer_id, l.key) = (2, 2 << 16))
}

/// The three chain passes of the frame's one write.
fn chain(b: &Board<'_>) -> [PassSpec; 3] {
    assert_eq!(b.pass_count(), 4, "one write");
    [0, 1, 2].map(|i| b.pass(i).expect("a chain pass"))
}

/// The scene's rectangle of the scratch, from the horizontal pass's texture.
fn scene_rect(b: &Board<'_>, half: &PassSpec) -> (u32, u32, u32, u32) {
    match b.draw(half, 0).map(|d| d.tex) {
        Some(Tex::Scratch { x, row, w, h }) => (x, row, w, h),
        other => panic!("the horizontal pass reads the scene, not {other:?}"),
    }
}

/// The display's pixel under a glass: the opaque window `win` over the wallpaper.
fn display(win: (u32, u32, u32, u32), x: u32, y: u32) -> f32 {
    let (wx, wy, ww, wh) = win;
    if x >= wx && x < wx + ww && y >= wy && y < wy + wh {
        tex(x - wx, ATLAS + y - wy)
    } else {
        tex(x, y)
    }
}

/// The scene pass of a glass at (300, 200, 400, 200) over `win` at radius `blur`, checked
/// texel by texel: how many texels had a window edge on their point (`edge` says what such a
/// texel must hold; every other is the mean of the four pixels at its point).
fn scene_against_pixels(blur: u32, win: (u32, u32, u32, u32)) -> usize {
    let at = (300, 200, 400, 200);
    let (s, taps) =
        (blur_scale(blur_radius(blur)), blur_radius(blur) / blur_scale(blur_radius(blur)));
    let mut f = ready_frame(2048);
    let p = planned(&mut f, &[window(win.0, win.1, win.2, win.3), pane(at, blur)]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let passes = chain(&b);
    let (vw, hh) = (at.2.div_ceil(s) + 2, at.3.div_ceil(s) + 2 + 2 * taps);
    assert_eq!(scene_rect(&b, &passes[1]), (0, 0, vw + 2 * taps, hh), "a texel per tap");
    assert_eq!(passes[0].area, (0, 0, vw + 2 * taps - 1, hh - 1));
    let mut gpu = Gpu::new(1);
    gpu.begin();
    gpu.pass(&b, &passes[0], &tex);
    // Scene texel (u, t): the point the tap of half texel (u − taps, t) stands for.
    let first = (at.0 - s - s * taps, at.1 - s - s * taps);
    let mut on_an_edge = 0;
    for t in 0..hh {
        for u in 0..vw + 2 * taps {
            let (px, py) = (first.0 + u * s + s / 2, first.1 + t * s + s / 2);
            let d = |dx: u32, dy: u32| display(win, px - 1 + dx, py - 1 + dy);
            let got = gpu.scratch[t as usize * W + u as usize];
            let across = (px == win.0 || px == win.0 + win.2) && py > win.1 && py < win.1 + win.3;
            let want = if across {
                // The point lies on the window's left or right edge: the side of the point's
                // own pixel alone (the upper layer's at its left edge, what lies beside it at
                // its right) — the stated bound.
                on_an_edge += 1;
                (d(1, 0) + d(1, 1)) / 2.0
            } else {
                (d(0, 0) + d(1, 0) + d(0, 1) + d(1, 1)) / 4.0
            };
            assert!(
                (got - want).abs() < 1e-5,
                "r{blur} texel ({u}, {t}) at ({px}, {py}): {got} against {want}"
            );
        }
    }
    on_an_edge
}

#[test]
fn a_reduced_scene_holds_the_taps_the_full_one_fetched() {
    // Quarter resolution: the points lie at x ≡ 2, y ≡ 2 (mod 4) from the glass at (300, 200);
    // half resolution: at odd x and y. A window whose edges lie between the points: every
    // texel — over the wallpaper, over the window, across its edges, around the pieces the
    // wallpaper is cut into — is the mean of the four pixels at its point.
    assert_eq!(scene_against_pixels(40, (411, 233, 300, 200)), 0);
    assert_eq!(scene_against_pixels(20, (411, 233, 300, 200)), 0);
    assert_eq!(scene_against_pixels(8, (410, 232, 300, 200)), 0);
}

#[test]
fn a_layer_s_edge_on_a_tap_s_point_is_one_side_s_pixels_and_bounded() {
    // The window's left and right edges ON the points (x = 410 and 710 at quarter resolution,
    // 411 and 711 at half): those texels hold the pixels of one side, where the mean of the
    // four mixed both — at most half a texel of the edge's contrast. Every other texel is
    // exact (checked inside). Quarter: a column of texels an edge, the window's 200 rows four
    // a texel. Half: the scene ends at x = 710 and y = 410, before the window's right edge
    // and its last rows — the left edge's column alone, the points at odd y from 233 to 409.
    assert_eq!(scene_against_pixels(40, (410, 233, 300, 200)), 2 * 50);
    assert_eq!(scene_against_pixels(8, (411, 232, 300, 200)), 89);
    // What that is in the blurred rows at most: half the centre tap's weight of the contrast.
    for (r, bound) in [(8, 0.11), (20, 0.09), (30, 0.06), (40, 0.05)] {
        let half_a_texel = Gaussian::library(r).weights().at(0) / 2.0;
        assert!(half_a_texel < bound, "radius {r}: {half_a_texel}");
    }
}

#[test]
fn a_tap_past_the_display_reads_the_scene_s_edge_texel_never_a_stale_one() {
    // The top bar: radius 20 at quarter resolution, five taps. Of its 492 x 22 taps those
    // above and beside the display have no pixel; the scene is the 480 x 16 inside it.
    let bar = (0, 0, 1920, 40);
    let mut f = ready_frame(2048);
    let p = planned(&mut f, &[window(100, 0, 800, 600), pane(bar, 20)]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let passes = chain(&b);
    assert_eq!(scene_rect(&b, &passes[1]), (0, 0, 480, 16));
    assert_eq!(passes[0].area, (0, 0, 479, 15));
    let mut gpu = Gpu::new(64);
    gpu.begin();
    for pass in &passes {
        gpu.pass(&b, pass, &tex);
    }
    // Every texel of the scene was drawn; the texels around it were not — only the clamp at
    // the scene's rectangle keeps a tap off them.
    for t in 0..16 {
        assert!(gpu.scratch[t * W..t * W + 480].iter().all(|v| *v < 2.0), "scene row {t}");
        assert_eq!(gpu.scratch[t * W + 480], STALE);
    }
    assert_eq!(gpu.scratch[16 * W], STALE);
    // Nothing stale reached the half rows or the bar's rows.
    let (vw, vh, hh) = (482, 12, 22);
    for h in 0..hh {
        assert!(gpu.half[h * W..h * W + vw].iter().all(|v| *v < 2.0), "half row {h}");
    }
    for v in 0..vh {
        assert!(gpu.cache[v * W..v * W + vw].iter().all(|x| *x < 2.0), "the bar's row {v}");
    }
    // The six half rows above the display are the blur of the scene's first row, as the
    // seventh is: a tap past the edge is the scene's edge texel.
    for h in 0..6 {
        assert_eq!(gpu.half[h * W..h * W + vw], gpu.half[6 * W..6 * W + vw], "half row {h}");
    }
    // And across: the bar's first half texels read columns left of the display as the
    // scene's first column — a flat scene row gives a flat half row there.
    let flat = |_: u32, row: u32| (row % ATLAS) as f32 / 2048.0;
    gpu.begin();
    for pass in &passes {
        gpu.pass(&b, pass, &flat);
    }
    let row = &gpu.half[10 * W..10 * W + vw];
    assert!(row.iter().all(|v| (v - row[0]).abs() < 1e-5), "a flat row blurs flat");
}

#[test]
fn test_reject_a_band_scene_missing_a_tap_column() {
    // A whole write, then a band's: the band's scene holds every texel its horizontal pass
    // reads — one column or one row less on any side and a stale texel reaches the rows.
    for blur in [8, 40] {
        let at = (300, 200, 400, 200);
        let cmds = [window(100, 100, 800, 600), pane(at, blur)];
        let mut f = ready_frame(2048);
        let whole = planned(&mut f, &cmds);
        let mut gpu = Gpu::new(256);
        {
            let b = Board::new(&f, whole, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
            gpu.begin();
            for pass in &chain(&b) {
                gpu.pass(&b, pass, &tex);
            }
        }
        let band = planned(&mut f, &[cmds[0].clone(), cmds[1].clone(), damage(500, 300, 20, 10)]);
        let band_of = f.step(1).and_then(|s| s.glass).map(|g| g.band);
        assert!(matches!(cache_of(&f, 1), Some(CacheStep::Write(_))), "r{blur}: a write");
        assert!(matches!(band_of, Some(Area::Rect { .. })), "r{blur}: a band, {band_of:?}");
        let b = Board::new(&f, band, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
        let passes = chain(&b);
        let (x0, y0, x1, y1) = passes[0].area;
        let stale_rows = |gpu: &Gpu| gpu.cache.iter().filter(|v| **v >= 2.0 && **v < STALE).count();
        let run = |scene_area| {
            let mut gpu = Gpu { scratch: vec![], half: vec![], cache: gpu.cache.clone() };
            gpu.scratch = vec![STALE; W * DISPLAY.1 as usize];
            gpu.half = vec![STALE; W * HALF_ROWS as usize];
            let scene = PassSpec { area: scene_area, ..passes[0] };
            for pass in [&scene, &passes[1], &passes[2]] {
                gpu.pass(&b, pass, &tex);
            }
            stale_rows(&gpu)
        };
        assert_eq!(run((x0, y0, x1, y1)), 0, "r{blur}: the band's scene holds its taps");
        for narrowed in
            [(x0 + 1, y0, x1, y1), (x0, y0, x1 - 1, y1), (x0, y0 + 1, x1, y1), (x0, y0, x1, y1 - 1)]
        {
            assert!(run(narrowed) > 0, "r{blur}: {narrowed:?} misses a texel a tap reads");
        }
    }
}

#[test]
fn the_rows_follow_the_full_resolution_gaussian_within_the_oracle_s_tolerance() {
    // The oracle's backdrop under the oracle's glass, a channel at a time: the rows the frame's
    // chain leaves, read back as the glass's back is (bilinear in its rows), against the kernel
    // table's full-resolution gaussian — inside `oracle::tolerance`, unchanged.
    let (gx, gy, gw, gh) = GLASS;
    let mut worst = [0.0f32; 4];
    for (k, radius) in BLUR_RADII.into_iter().enumerate() {
        let (s, weights) = (blur_scale(radius), Gaussian::full(radius).weights());
        let mut f = ready_frame(2048);
        let p = planned(&mut f, &[pane(GLASS, radius)]);
        let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
        let passes = chain(&b);
        assert_eq!(passes[2].target, Target::Cache(0));
        let rows = Tex::Cache { x: 0, row: 0, w: gw.div_ceil(s) + 2, h: gh.div_ceil(s) + 2 };
        for channel in 0..3 {
            let layout = |x: u32, row: u32| f32::from(backdrop(x, row)[channel]) / 255.0;
            let mut gpu = Gpu::new(256);
            gpu.begin();
            for pass in &passes {
                gpu.pass(&b, pass, &layout);
            }
            let grid = (gy + 4..gy + gh)
                .step_by(12)
                .flat_map(|y| (gx + 4..gx + gw).step_by(12).map(move |x| (x, y)));
            for (x, y) in READS[..4].iter().copied().chain(grid) {
                let mut want = 0.0f32;
                for (dy, wy) in weights.iter() {
                    for (dx, wx) in weights.iter() {
                        want += wx * wy * layout((x as i32 + dx) as u32, (y as i32 + dy) as u32);
                    }
                }
                let at = |p: u32, o: u32| (f64::from(p) + 0.5 - f64::from(o - s)) / f64::from(s);
                let got = gpu.fetch(rows, (at(x, gx), at(y, gy)), &layout);
                worst[k] = worst[k].max((got - want).abs() * 255.0);
            }
        }
        assert!(
            worst[k] <= f32::from(tolerance(radius)),
            "radius {radius}: {} levels off the full gaussian (tolerance {})",
            worst[k],
            tolerance(radius)
        );
    }
    println!("levels off the full-resolution gaussian by radius {BLUR_RADII:?}: {worst:?}");
}

#[test]
fn a_scaled_quad_is_its_shape_at_the_smaller_size() {
    // `layer.wgsl`'s distance at `uv` of a quad: the rounded rectangle's, in pixels.
    let sdf = |q: &Quad, uv: (f32, f32)| {
        let p = (uv.0 * q.push[4] + q.push[6], uv.1 * q.push[5] + q.push[7]);
        let d = (p.0.abs() - q.push[8], p.1.abs() - q.push[9]);
        (d.0.max(0.0).powi(2) + d.1.max(0.0).powi(2)).sqrt() + d.0.max(d.1).min(0.0) - q.push[10]
    };
    let whole = Quad::layer(300.0, 200.0, 400.0, 200.0, [1.0, 0.5, 0.25, 0.75], 20.0);
    let corner =
        Quad::part((300.0, 200.0, 400.0, 200.0), (300.0, 200.0, 20.0, 20.0), [1.0; 4], 20.0);
    for q in [whole, corner, whole.part_of((500.0, 250.0, 64.0, 32.0))] {
        let r = q.scaled(0.25, (256.0, 156.0));
        assert_eq!(
            (r.x, r.y, r.w, r.h),
            ((q.x - 256.0) / 4.0, (q.y - 156.0) / 4.0, q.w / 4.0, q.h / 4.0)
        );
        assert_eq!((r.tint(), &r.push[12..]), (q.tint(), &q.push[12..]), "tint and texture kept");
        for uv in [(0.0, 0.0), (0.5, 0.5), (0.03, 0.9), (1.0, 1.0)] {
            assert_eq!(
                sdf(&r, uv),
                sdf(&q, uv) / 4.0,
                "the distance in the smaller plane's pixels"
            );
        }
    }
    // A shadow: one over its blur and its offset are pixels too.
    let shadow =
        Quad::shadow(100.0, 100.0, 300.0, 200.0, 12.0, 24.0, [0.0, 8.0], [0.0, 0.0, 0.0, 0.5]);
    let r = shadow.scaled_shadow(0.5, (0.0, 0.0));
    assert_eq!(
        (r.x, r.y, r.w, r.h),
        (shadow.x / 2.0, shadow.y / 2.0, shadow.w / 2.0, shadow.h / 2.0)
    );
    assert_eq!((r.push[10], r.push[11], r.push[13]), (6.0, 1.0 / 12.0, 4.0));
    assert_eq!(r.tint(), shadow.tint());
}
