// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Cost follows what is seen (TASK-0351 P6): a glass whose backdrop changed where the layers
//! above hide it is not blurred again — its rows owe the change, and the frame that uncovers
//! it pays. Twenty cascaded windows blurred three or four glasses a frame on the board for a
//! change one of them showed (`board--2026-11-18T13-29-53`).

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::{Area, CacheStep, Writes};
use support::*;

/// A window's glass body: a glass of its own id and key, at full opacity.
fn body(id: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    with(glass(layer(x, y, w, h), 40), |l| (l.layer_id, l.key) = (id, id << 16 | 0x100))
}

/// `n` windows of 1240×780 opened one over the other, each 32 right and 24 down of the last.
fn cascade(n: u32) -> Vec<Command> {
    let mut scene = vec![layer(0, 0, 1920, 1080)];
    scene.extend((0..n).map(|w| body(w + 1, 100 + 32 * w, 60 + 24 * w, 1240, 780)));
    scene
}

fn with_damage(mut scene: Vec<Command>, d: Command) -> Vec<Command> {
    scene.push(d);
    scene
}

#[test]
fn a_change_under_the_window_above_is_not_blurred_again_until_it_shows() {
    let mut f = ready_frame(2048);
    let (below, over) = (body(1, 100, 100, 800, 600), body(2, 300, 200, 800, 600));
    let scene = vec![layer(0, 0, 1920, 1080), below.clone(), over];
    f.layers.take_frame(&scene);
    assert_eq!(f.plan(DISPLAY, &every_blur).writes.full, 2);
    // The base changes deep under both: only the window on top shows it.
    f.layers.take_frame(&with_damage(scene.clone(), damage(600, 500, 40, 40)));
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { band: 1, band_base: 1, hidden: 1, ..Default::default() });
    assert!(matches!(cache_of(&f, 1), Some(CacheStep::Read(_))), "the glass below keeps its rows");
    assert!(matches!(cache_of(&f, 2), Some(CacheStep::Write(_))));
    // Nothing changes: nothing is written, the rows below still owe their band (counted every
    // frame they do).
    f.layers.take_frame(&with_damage(scene.clone(), damage(0, 0, 1, 1)));
    assert_eq!(f.plan(DISPLAY, &every_blur).writes, Writes { hidden: 1, ..Default::default() });
    // The window on top goes: the band is seen, and the frame that shows it blurs it.
    f.layers.take_frame(&[layer(0, 0, 1920, 1080), below.clone(), damage(0, 0, 1, 1)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { band: 1, ..Default::default() });
    assert_eq!(
        f.step(1).and_then(|s| s.glass).map(|g| g.band),
        Some(Area::Rect { x0: 600, y0: 500, x1: 640, y1: 540 }),
        "the band it owed"
    );
    assert!(p.damage.touches(Area::rect(600, 500, 40, 40)), "and the screen shows it");
    // Paid: the next frame reads.
    f.layers.take_frame(&[layer(0, 0, 1920, 1080), below, damage(0, 0, 1, 1)]);
    assert_eq!(f.plan(DISPLAY, &every_blur).writes, Writes::default());
}

#[test]
fn a_change_that_reaches_what_is_seen_of_a_covered_glass_is_blurred() {
    let mut f = ready_frame(2048);
    let scene =
        vec![layer(0, 0, 1920, 1080), body(1, 100, 100, 800, 600), body(2, 300, 200, 800, 600)];
    f.layers.take_frame(&scene);
    f.plan(DISPLAY, &every_blur);
    // Under the strip of the lower window that shows left of the upper one.
    f.layers.take_frame(&with_damage(scene.clone(), damage(150, 400, 40, 40)));
    let p = f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 1), Some(CacheStep::Write(_))), "its strip shows the change");
    assert_eq!(p.writes.hidden, 0);
}

#[test]
fn a_glass_under_a_faded_or_rowless_layer_is_blurred() {
    // A window fading in hides nothing: the glass under it is written as before.
    let mut f = ready_frame(2048);
    let faded = with(body(2, 300, 200, 800, 600), |l| l.opacity = 128);
    let scene = vec![layer(0, 0, 1920, 1080), body(1, 100, 100, 800, 600), faded];
    f.layers.take_frame(&scene);
    f.plan(DISPLAY, &every_blur);
    f.layers.take_frame(&with_damage(scene, damage(600, 500, 40, 40)));
    assert_eq!(f.plan(DISPLAY, &every_blur).writes.hidden, 0);
    // A glass above that holds no rows yet is drawn without its back: it hides nothing either.
    let mut f = ready_frame(2048);
    let scene = vec![layer(0, 0, 1920, 1080), body(1, 100, 100, 800, 600)];
    f.layers.take_frame(&scene);
    f.plan(DISPLAY, &every_blur);
    let opened = with_damage(
        vec![scene[0].clone(), scene[1].clone(), body(2, 300, 200, 800, 600)],
        damage(600, 500, 40, 40),
    );
    f.layers.take_frame(&opened);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!((p.writes.hidden, p.writes.new), (0, 1), "the newcomer has no rows to hide with");
}

#[test]
fn a_new_backdrop_under_a_wholly_covered_glass_waits_whole() {
    let mut f = ready_frame(2048);
    // A small glass wholly under an opaque pane.
    let small = body(1, 400, 300, 200, 150);
    let pane = with(layer(300, 200, 800, 600), |l| (l.opaque, l.layer_id) = (true, 2));
    let scene = |x: u32| vec![layer(0, 0, 1920, 1080), layer(x, 250, 50, 50), small.clone()];
    let covered = |x: u32| {
        let mut s = scene(x);
        s.push(pane.clone());
        s.push(damage(0, 0, 1, 1));
        s
    };
    f.layers.take_frame(&covered(380));
    f.plan(DISPLAY, &every_blur);
    // A layer below it moves within its reach: a new backdrop — nobody sees it.
    f.layers.take_frame(&covered(390));
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { hidden: 1, ..Default::default() });
    // Uncovered: written whole.
    let mut shown = scene(390);
    shown.push(damage(0, 0, 1, 1));
    f.layers.take_frame(&shown);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes.total(), 1);
    assert_eq!(f.step(2).and_then(|s| s.glass).map(|g| g.band), Some(Area::Full));
}

#[test]
fn twenty_cascaded_windows_blur_one_glass_for_a_change_under_them() {
    let mut f = ready_frame(1024);
    let scene = cascade(20);
    f.layers.take_frame(&scene);
    f.plan(DISPLAY, &every_blur);
    // The cache grows with the glasses, as the executor grows it.
    for _ in 0..16 {
        if f.glass.short() == 0 {
            break;
        }
        f.place_short();
    }
    assert_eq!(f.glass.short(), 0);
    // A dock button under the cascade's middle changes — the desktop's own surface.
    f.layers.take_frame(&with_damage(scene.clone(), damage(900, 700, 64, 64)));
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes.total(), 1, "{:?}", p.writes);
    assert!(p.writes.hidden >= 15, "{:?}", p.writes);
    // The same change again: every hidden glass still owes one band, none is written twice.
    f.layers.take_frame(&with_damage(scene, damage(900, 700, 64, 64)));
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes.total(), 1, "{:?}", p.writes);
}
