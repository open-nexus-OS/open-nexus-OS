// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The plan's own time over a desktop of twelve glass windows (TASK-0351 P6): the planner asks,
//! per glass, which of the frame's changes reach it — the number printed here is what that costs
//! a frame, for the ledger. The assertions hold the writes, never the time (host load moves it).

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Frame, FrameLayer};
use std::time::Instant;

const DISPLAY: (u32, u32) = (1920, 1080);

fn layer(x: u32, y: u32, w: u32, h: u32, f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer { src_row_abs: 5400, opacity: 255, ..FrameLayer::default() };
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

/// The base, twelve windows down a cascade — a glass chrome over an opaque body each, rounded
/// and shadowed — and the dock's glass on top: 26 layers, 13 glasses.
fn desktop() -> Vec<Command> {
    let mut cmds = vec![layer(0, 0, 1920, 1080, |l| (l.opaque, l.key) = (true, 0xFFFF_0000))];
    for i in 0..12u32 {
        let (x, y, id) = (60 + 48 * i, 40 + 36 * i, 1 + i);
        cmds.push(layer(x, y, 720, 480, |l| {
            (l.corner_radius, l.shadow_blur, l.shadow_offset_y, l.shadow_alpha) = (16, 24, 8, 102);
            (l.opaque, l.layer_id, l.key) = (true, id, id << 16);
        }));
        cmds.push(layer(x, y, 720, 48, |l| {
            (l.backdrop_blur, l.layer_id, l.key) = (40, id, (id << 16) | 1);
        }));
    }
    cmds.push(layer(560, 980, 800, 90, |l| (l.backdrop_blur, l.corner_radius) = (40, 24)));
    cmds
}

fn damage(origin: u32, x: u32, y: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: 40, height: 20 }, origin }
}

/// `reps` plans, each for one small change at a place that walks down the cascade: the writes
/// in all, and the nanoseconds a plan took.
fn run(origin: impl Fn(u32) -> u32, reps: u32) -> (u32, u128) {
    let mut f = Frame::new(4096);
    f.glass.ready = true;
    f.layers.take_frame(&desktop());
    f.plan(DISPLAY, &|_| true);
    let mut writes = 0;
    let t0 = Instant::now();
    for n in 0..reps {
        let i = n % 12;
        f.layers.take_frame(&[damage(origin(i), 80 + 48 * i, 50 + 36 * i)]);
        writes += f.plan(DISPLAY, &|_| true).writes.total();
    }
    (writes, t0.elapsed().as_nanos() / u128::from(reps))
}

#[test]
fn a_plan_over_twelve_glass_windows_is_timed_for_the_ledger() {
    let reps = 2400;
    // Origin 0 (unknown): every glass whose reach the change touches blurs again.
    let (unknown, ns) = run(|_| 0, reps);
    println!("twelve glass windows: plan {ns} ns (origin 0), {unknown} writes in {reps} plans");
    assert!(unknown >= reps, "a change of unknown origin under a chrome writes it: {unknown}");
    // Named by the window's chrome (its own content): the chromes of the windows above it
    // alone, each asked for the covers between.
    let (chrome, ns) = run(|i| ((1 + i) << 16) | 1, reps);
    println!("twelve glass windows: plan {ns} ns (the chrome's key), {chrome} writes");
    assert!(chrome > 0 && chrome < unknown, "a known origin blurs fewer: {chrome} of {unknown}");
    // Named by the window's body, under its own chrome: the window's own change, and the
    // chrome's drawn back covers it for every glass above.
    let (body, ns) = run(|i| (1 + i) << 16, reps);
    println!("twelve glass windows: plan {ns} ns (the body's key), {body} writes");
    assert_eq!(body, 0, "the change ends at the chrome's back");
}
