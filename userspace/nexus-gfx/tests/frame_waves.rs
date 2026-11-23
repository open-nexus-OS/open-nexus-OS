// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's glass writes in waves (TASK-0351 P19), against pixels — the board's passes run
//! on the host (`frame_exec.rs`, where a pass clears what it does not draw in its area and
//! the scratches are stale at a frame's start): the rows a frame's waves leave are the rows
//! its writes leave one by one, whole writes and bands; a write whose scene shows a glass
//! written in the same frame runs a wave later; no place of a wave lies past a scratch.

#[path = "frame_exec.rs"]
mod exec;
#[path = "frame_support.rs"]
mod support;

use exec::{Gpu, HALF_ROWS, STALE, W};
use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::{Board, CacheStep, Frame, PassKind, PassSpec, Target, MERGE_MAX};
use support::{cache_of, damage, every_blur, glass, layer, ready_frame, with, DISPLAY};

const LAYOUT_ROWS: u32 = 14_040;
const CACHE_ROWS: u32 = 1024;

/// The layout's content: a value in `[0, 1)` that differs from texel to texel.
fn tex(x: u32, row: u32) -> f32 {
    ((x.wrapping_mul(7919) ^ row.wrapping_mul(104_729)) % 997) as f32 / 997.0
}

fn window(id: u32, at: (u32, u32, u32, u32)) -> Command {
    with(layer(at.0, at.1, at.2, at.3), |l| (l.opaque, l.layer_id, l.key) = (true, id, id << 16))
}

fn pane(id: u32, at: (u32, u32, u32, u32), blur: u32) -> Command {
    with(glass(layer(at.0, at.1, at.2, at.3), blur), |l| (l.layer_id, l.key) = (id, id << 16))
}

fn board<'a>(f: &'a mut Frame, cmds: &[Command], half_rows: u32) -> Board<'a> {
    f.layers.take_frame(cmds);
    let p = f.plan(DISPLAY, &every_blur);
    Board::new(f, p, DISPLAY, LAYOUT_ROWS, half_rows)
}

/// The frame's passes before the slot's, run in order from stale scratches.
fn run(gpu: &mut Gpu, b: &Board<'_>) {
    gpu.begin();
    for p in 0..b.pass_count() - 1 {
        gpu.pass(b, &b.pass(p).expect("a pass"), &tex);
    }
}

/// Texels of the cache computed from a texel no pass drew.
fn spoiled(cache: &[f32]) -> usize {
    cache.iter().filter(|v| **v >= 2.0 && **v < STALE).count()
}

/// The most two caches differ by, and the texels either holds.
fn apart(a: &[f32], b: &[f32]) -> (f32, usize) {
    let held = a.iter().zip(b).filter(|(x, y)| **x < STALE || **y < STALE);
    held.fold((0.0, 0), |(most, n), (x, y)| ((x - y).abs().max(most), n + 1))
}

/// A window, three glasses apart at three radii (each 45 half rows), a pill over the first.
fn scene() -> [Command; 5] {
    [
        window(1, (100, 100, 1500, 800)),
        pane(2, (200, 150, 400, 90), 40),
        pane(3, (800, 300, 300, 130), 20),
        pane(4, (1300, 600, 250, 70), 8),
        pane(5, (260, 170, 100, 40), 20),
    ]
}

#[test]
fn a_wave_s_writes_leave_what_they_leave_alone() {
    // The same presents down two boards: one whose half scratch holds the frame's writes side
    // by side, one whose half scratch holds one write at a time — one by one.
    let (mut waves, mut alone) = (ready_frame(CACHE_ROWS), ready_frame(CACHE_ROWS));
    let (mut a, mut b) = (Gpu::new(CACHE_ROWS as usize), Gpu::new(CACHE_ROWS as usize));
    let whole = scene();
    // Then the window's content changes under two of the glasses and in the pill's reach:
    // band writes, the pill's a wave after the glass it lies on.
    let bands = [&whole[..], &[damage(380, 180, 600, 200)]].concat();
    for (cmds, writes, in_waves) in [(&whole[..], 4, 2), (&bands[..], 3, 2)] {
        let shared = board(&mut waves, cmds, HALF_ROWS);
        let single = board(&mut alone, cmds, 66);
        assert_eq!(shared.glass.writes, writes);
        assert_eq!((shared.wave_count(), single.wave_count()), (in_waves, writes as usize));
        assert_eq!(shared.pass_count(), 2 * writes as usize + in_waves + 1);
        assert_eq!(single.pass_count(), 3 * writes as usize + 1);
        run(&mut a, &shared);
        run(&mut b, &single);
        assert_eq!((spoiled(&a.cache), spoiled(&b.cache)), (0, 0));
        // The same rows to the last digit a push constant's 24 bits carry: the two boards
        // name the half rows as fractions of scratches of different heights.
        let (most, held) = apart(&a.cache, &b.cache);
        assert!(held > 10_000 && most < 1e-4, "{held} texels, {most} apart");
    }
    // The band presents wrote less than the rows: a band's pass leaves the rest of its glass.
    assert!(matches!(cache_of(&waves, 1), Some(CacheStep::Write(_))));
}

#[test]
fn a_band_s_vertical_pass_keeps_the_rows_beside_it() {
    // Two glasses whose rows lie one under the other in the cache, each written for a band:
    // a pass down around both bands would clear what lies between them — the rest of each
    // glass. A pass a write clears nothing it does not draw.
    let mut f = ready_frame(CACHE_ROWS);
    let mut gpu = Gpu::new(CACHE_ROWS as usize);
    let whole = scene();
    run(&mut gpu, &board(&mut f, &whole, HALF_ROWS));
    let before = gpu.cache.clone();
    let bands = [&whole[..], &[damage(380, 180, 600, 200)]].concat();
    let b = board(&mut f, &bands, HALF_ROWS);
    let downs: Vec<PassSpec> = (0..b.pass_count())
        .filter_map(|p| b.pass(p))
        .filter(|p| matches!(p.target, Target::Cache(_)))
        .collect();
    assert_eq!(downs.len(), 3, "a vertical pass a write");
    run(&mut gpu, &b);
    let inside = |k: usize| {
        let (x, y) = ((k % W) as u32, (k / W) as u32);
        downs.iter().any(|d| x >= d.area.0 && x <= d.area.2 && y >= d.area.1 && y <= d.area.3)
    };
    let (mut kept, mut written) = (0, 0);
    for (k, (old, new)) in before.iter().zip(&gpu.cache).enumerate() {
        if inside(k) {
            written += 1;
            assert!(*new < 2.0, "texel {k} of a band holds {new}");
        } else {
            kept += usize::from(*old < STALE);
            assert_eq!(old, new, "texel {k} beside the bands");
        }
    }
    assert!(written > 0 && kept > written, "{written} written, {kept} kept");
    // The rectangle around both bands holds texels of neither: what one pass would clear.
    let around = downs.iter().fold((u32::MAX, u32::MAX, 0, 0), |u, d| {
        (u.0.min(d.area.0), u.1.min(d.area.1), u.2.max(d.area.2), u.3.max(d.area.3))
    });
    let between = (around.1..=around.3)
        .flat_map(|y| (around.0..=around.2).map(move |x| y as usize * W + x as usize))
        .filter(|k| !inside(*k) && before[*k] < STALE)
        .count();
    assert!(between > 0, "nothing between the bands: the case names no hazard");
}

#[test]
fn test_reject_a_write_sharing_a_wave_with_the_glass_it_blurs() {
    // The pill lies over the first glass: its scene shows that glass's back. In the board's
    // order the glass's rows are written before the pill's scene reads them.
    let mut f = ready_frame(CACHE_ROWS);
    let cmds = [
        window(1, (100, 100, 1500, 800)),
        pane(2, (200, 150, 400, 90), 40),
        pane(5, (260, 170, 100, 40), 20),
    ];
    let b = board(&mut f, &cmds, HALF_ROWS);
    assert_eq!((b.glass.writes, b.wave_count(), b.pass_count()), (2, 2, 7));
    let passes: Vec<PassSpec> = (0..6).map(|p| b.pass(p).expect("a pass")).collect();
    assert_eq!(passes[2].target, Target::Cache(0), "the glass's rows");
    assert_eq!(passes[3].kind, PassKind::Scene { upto: 2 }, "then the pill's scene");
    let mut gpu = Gpu::new(CACHE_ROWS as usize);
    run(&mut gpu, &b);
    assert_eq!(spoiled(&gpu.cache), 0);
    // Shared — the pill's scene before the glass's vertical pass — the pill's rows are
    // computed from rows no pass has written yet.
    let mut gpu = Gpu::new(CACHE_ROWS as usize);
    gpu.begin();
    for p in [0, 1, 3, 4, 2, 5] {
        gpu.pass(&b, &passes[p], &tex);
    }
    assert!(spoiled(&gpu.cache) > 0, "a scene that read rows written after it");
    // Apart, the two share a wave.
    let mut f = ready_frame(CACHE_ROWS);
    let apart = [cmds[0].clone(), cmds[1].clone(), pane(5, (900, 500, 100, 40), 20)];
    let b = board(&mut f, &apart, HALF_ROWS);
    assert_eq!((b.wave_count(), b.pass_count()), (1, 6));
}

#[test]
fn a_window_between_two_glasses_keeps_them_in_one_wave() {
    // A panel above the first glass and below the second, in the second's reach: each write
    // has its own scene pass — the first's without the panel, the second's with it.
    let mut f = ready_frame(CACHE_ROWS);
    let cmds = [
        window(1, (200, 200, 700, 500)),
        pane(2, (100, 100, 200, 100), 20),
        window(3, (650, 300, 150, 120)),
        pane(4, (600, 400, 300, 200), 20),
    ];
    let b = board(&mut f, &cmds, HALF_ROWS);
    assert_eq!((b.glass.writes, b.wave_count()), (2, 1));
    let scenes: Vec<_> = (0..2).map(|p| b.pass(p).map(|s| (s.kind, b.draws(&s)))).collect();
    let (Some((PassKind::Scene { upto: 1 }, first)), Some((PassKind::Scene { upto: 3 }, second))) =
        (scenes[0], scenes[1])
    else {
        panic!("two scene passes, not {scenes:?}");
    };
    assert!(second > first, "the panel is in the second scene alone");
    let mut gpu = Gpu::new(CACHE_ROWS as usize);
    run(&mut gpu, &b);
    assert_eq!(spoiled(&gpu.cache), 0);
}

#[test]
fn test_reject_a_wave_past_the_scratch() {
    // Seven tall glasses side by side at the smallest radius: 160 half rows each, three to a
    // half scratch of 600. A write that finds no room starts a wave at the top; every pass
    // lies inside its target.
    let mut f = ready_frame(2048);
    let mut cmds = vec![window(1, (0, 0, 1920, 1080))];
    cmds.extend((0..7).map(|k| pane(2 + k, (40 + 260 * k, 300, 200, 300), 8)));
    let plane_rows = f.glass.plane_rows();
    let b = board(&mut f, &cmds, HALF_ROWS);
    assert_eq!((b.glass.writes, b.wave_count(), b.pass_count()), (7, 3, 2 * 7 + 3 + 1));
    let mut shared = 0;
    for p in 0..b.pass_count() - 1 {
        let spec = b.pass(p).expect("a pass");
        let rows = match spec.target {
            Target::Scratch => DISPLAY.1,
            Target::Half => HALF_ROWS,
            Target::Cache(_) => plane_rows,
            Target::Slot => panic!("the slot's pass is the last"),
            Target::Run(_) => panic!("no run is taken here"),
        };
        assert!(spec.area.2 < DISPLAY.0 && spec.area.3 < rows, "pass {p}: {:?}", spec.area);
        if let PassKind::Blurs { count, .. } = spec.kind {
            assert!(count <= 3 && count <= MERGE_MAX, "{count} writes in a wave");
            shared += count;
        }
    }
    assert_eq!(shared, 6, "two waves of three; the seventh write runs alone");
    let mut gpu = Gpu::new(2048);
    run(&mut gpu, &b);
    assert_eq!(spoiled(&gpu.cache), 0);
}
