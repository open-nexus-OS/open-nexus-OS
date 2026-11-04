// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A pass's draws walked once (TASK-0351 P10): the walk gives the indexed walk's draws in its
//! order for a plain window, a cascade of rounded, shadowed windows under a glass panel, and the
//! executor's twelve-window desktop — and over the desktop it costs a fraction of asking per
//! draw.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::{Board, DrawSpec, Frame, FrameLayer, PassSpec, Planned};
use std::time::Instant;

const DISPLAY: (u32, u32) = (1920, 1080);
const LAYOUT_ROWS: u32 = 14_040;
const HALF_ROWS: u32 = 600;
const ATLAS: u32 = 5400;

fn layer(x: u32, y: u32, w: u32, h: u32, f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer { src_row_abs: ATLAS, opacity: 255, ..FrameLayer::default() };
    (l.dst_x, l.dst_y, l.width, l.height) = (x, y, w, h);
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
        flags: if l.opaque { nexus_gfx::command::buffer::LAYER_OPAQUE } else { 0 },
        key: l.key,
        source: l.source,
    }
}

fn ready_frame(rows: u32) -> Frame {
    let mut f = Frame::new(rows);
    f.glass.ready = true;
    f
}

fn planned(frame: &mut Frame, cmds: &[Command]) -> Planned {
    frame.layers.take_frame(cmds);
    frame.plan(DISPLAY, &|_| true)
}

fn indexed(b: &Board<'_>, p: &PassSpec) -> Vec<DrawSpec> {
    (0..b.draws(p)).map(|i| b.draw(p, i).expect("a draw")).collect()
}

/// `n` windows down a cascade, each rounded and shadowed, every third opaque.
fn cascade(n: u32) -> Vec<Command> {
    (0..n)
        .map(|i| {
            layer(60 + 48 * i, 40 + 36 * i, 720, 480, |l| {
                (l.corner_radius, l.shadow_blur, l.shadow_offset_y, l.shadow_alpha) =
                    (16, 24, 8, 102);
                l.opaque = i % 3 == 0;
                l.layer_id = 1 + i;
            })
        })
        .collect()
}

fn every_pass_matches(cmds: &[Command]) -> (usize, usize) {
    let mut f = ready_frame(2048);
    let p = planned(&mut f, cmds);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let mut draws = 0;
    for pass in (0..b.pass_count()).map(|p| b.pass(p).expect("a pass")) {
        let walked: Vec<DrawSpec> = b.walk(&pass).collect();
        assert_eq!(walked, indexed(&b, &pass), "pass {pass:?}");
        draws += walked.len();
    }
    (b.pass_count(), draws)
}

#[test]
fn the_walk_is_the_indexed_walk_for_a_window() {
    assert_eq!(every_pass_matches(&[layer(100, 50, 400, 300, |_| {})]), (1, 2));
}

#[test]
fn the_walk_is_the_indexed_walk_under_a_glass_over_a_cascade() {
    let mut cmds = cascade(5);
    cmds.push(layer(300, 200, 500, 260, |l| (l.corner_radius, l.backdrop_blur) = (20, 40)));
    let (passes, draws) = every_pass_matches(&cmds);
    assert_eq!(passes, 4);
    assert!(draws > 40, "{draws} draws");
}

#[test]
fn the_walk_is_the_indexed_walk_over_twelve_windows() {
    let (passes, draws) = every_pass_matches(&cascade(12));
    assert_eq!(passes, 1);
    assert!(draws > 100, "{draws} draws");
}

/// The desktop of twelve windows: the walk asks the plan's layers once a frame; the indexed walk,
/// once a draw (and the executor asked twice). The ratio is printed for the ledger; the assertion
/// keeps only the direction, host load moves both alike.
#[test]
fn the_walk_costs_a_fraction_of_asking_per_draw() {
    let cmds = cascade(12);
    let mut f = ready_frame(2048);
    let p = planned(&mut f, &cmds);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let pass = b.pass(0).expect("the slot's pass");
    let n = b.draws(&pass);
    let reps = 50;
    let t0 = Instant::now();
    let mut walked = 0usize;
    for _ in 0..reps {
        walked += b.walk(&pass).count();
    }
    let walk = t0.elapsed();
    let t0 = Instant::now();
    let mut asked = 0usize;
    for _ in 0..reps {
        asked += (0..n).filter_map(|i| b.draw(&pass, i)).count();
    }
    let per_draw = t0.elapsed();
    assert_eq!((walked, asked), (reps * n, reps * n));
    let (w, a) = (walk.as_nanos() / (reps * n) as u128, per_draw.as_nanos() / (reps * n) as u128);
    println!("twelve windows: {n} draws, walk {w} ns a draw, asked per draw {a} ns a draw");
    assert!(w * 2 <= a, "walk {w} ns a draw, per draw {a} ns a draw");
}
