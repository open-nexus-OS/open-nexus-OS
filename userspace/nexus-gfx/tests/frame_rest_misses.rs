// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner's counters of package A (TASK-0351 P19, TASK-0343 P8 — instruments, no rule):
//! W5, a glass held through its window's motion whose key misses at rest is counted by the
//! first of its prints that differs from the ones its rows were written with — the wallpaper
//! epoch, the set of layers below, only its own window's layers, other windows' layers, or
//! something else the key folds; W4, a band write reached by an unknown origin is counted as
//! origin 0 (`zero`) or a key the set does not hold (`stray`), their sum `unknown`.

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{CacheStep, Frame, Planned, RestMisses, WritesBy};
use support::*;

const BASE: u32 = 0xFFFF_0000;

fn damage_from(origin: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin }
}

/// The damage a present names when it is owed for its layers alone.
fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn fade(layer_id: u32, opacity: u32) -> Command {
    Command::LayerTransform {
        layer_id,
        dx: 0,
        dy: 0,
        opacity,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    }
}

fn base() -> Command {
    with(layer(0, 0, 1920, 1080), |l| (l.opaque, l.key) = (true, BASE))
}

/// A layer of window `id` below the pane, within its reach.
fn window(id: u32, x: u32) -> Command {
    with(layer(x, 350, 200, 100), |l| (l.layer_id, l.key) = (id, id << 16))
}

/// The pane's own window's body below its glass.
fn body(x: u32) -> Command {
    with(layer(x, 300, 400, 200), |l| (l.layer_id, l.key) = (2, 0x2_0001))
}

/// The glass of window 2.
fn pane() -> Command {
    with(glass(layer(300, 300, 400, 200), 40), |l| (l.layer_id, l.key) = (2, 2 << 16))
}

fn present(f: &mut Frame, cmds: &[Command]) -> Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &every_blur)
}

/// The pane written over `scene`, held while its window fades with `during` presented (`None`:
/// the set kept) and `meanwhile` done to the frame, then at rest: the rest plan.
fn at_rest(scene: &[Command], during: Option<&[Command]>, meanwhile: fn(&mut Frame)) -> Planned {
    let mut f = ready_frame(4096);
    let mut cmds = scene.to_vec();
    cmds.push(pane());
    present(&mut f, &cmds);
    let at = scene.len();
    assert!(matches!(cache_of(&f, at), Some(CacheStep::Write(_))), "the first blur");
    let mut cmds = vec![fade(2, 128)];
    if let Some(layers) = during {
        cmds.extend(layers.iter().cloned().chain([pane()]));
    }
    cmds.push(no_damage());
    let p = present(&mut f, &cmds);
    let at = during.map_or(scene.len(), <[Command]>::len);
    assert!(matches!(cache_of(&f, at), Some(CacheStep::Read(_))), "in motion: held");
    assert_eq!(p.writes.total(), 0);
    meanwhile(&mut f);
    present(&mut f, &[fade(2, 255), no_damage()])
}

fn nothing(_: &mut Frame) {}

/// The rest plan's one write, a key miss at rest, counted as `class`.
fn one(p: &Planned, class: RestMisses) {
    assert_eq!((p.writes.backdrop, p.by.rest, p.by.misses), (1, 1, class));
}

#[test]
fn a_glass_at_rest_over_what_it_was_blurred_with_misses_nothing() {
    let p = at_rest(&[base(), window(3, 250)], None, nothing);
    assert_eq!((p.writes.total(), p.by), (0, WritesBy::default()));
}

#[test]
fn a_rest_miss_of_a_new_wallpaper_is_epoch() {
    let p = at_rest(&[base(), window(3, 250)], None, |f| f.glass.wallpaper_epoch += 1);
    one(&p, RestMisses { epoch: 1, ..Default::default() });
}

#[test]
fn a_rest_miss_of_a_layer_that_came_below_is_set() {
    let during = [base(), window(3, 250), window(4, 500)];
    let p = at_rest(&[base(), window(3, 250)], Some(&during), nothing);
    // C-r: written as the band of the window that came (500..700 of the pane's 300..700).
    one(&p, RestMisses { set: 1, band: 1, ..Default::default() });
}

#[test]
fn a_rest_miss_of_its_own_window_s_layer_moved_is_own() {
    let during = [base(), window(3, 250), body(320)];
    let p = at_rest(&[base(), window(3, 250), body(300)], Some(&during), nothing);
    // The body moved spans the whole pane: a band as wide as the glass is not counted `band`.
    one(&p, RestMisses { own: 1, ..Default::default() });
}

#[test]
fn a_rest_miss_of_another_window_s_layer_moved_is_moved() {
    let during = [base(), window(3, 260), body(320)];
    let p = at_rest(&[base(), window(3, 250), body(300)], Some(&during), nothing);
    // With the body moved too, the band spans the whole pane: not counted `band`.
    one(&p, RestMisses { moved: 1, ..Default::default() });
}

#[test]
fn a_rest_miss_of_a_layer_drawn_otherwise_in_place_is_drew() {
    let faded = with(window(3, 250), |l| l.opacity = 200);
    let p = at_rest(&[base(), window(3, 250)], Some(&[base(), faded]), nothing);
    // C-r: written as the band of the faded window alone.
    one(&p, RestMisses { drew: 1, band: 1, ..Default::default() });
}

#[test]
fn an_unknown_origin_is_zero_or_stray_and_their_sum_is_unknown() {
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(3, 250), pane()]);
    let zero = present(&mut f, &[damage(400, 400, 20, 10)]).by;
    let stray = present(&mut f, &[damage_from(0xDEAD_0000, 400, 400, 20, 10)]).by;
    let both = present(&mut f, &[damage(400, 400, 20, 10), damage_from(7, 450, 400, 4, 4)]);
    assert_eq!((zero.unknown, zero.zero, zero.stray), (1, 1, 0));
    assert_eq!((stray.unknown, stray.zero, stray.stray), (1, 0, 1));
    // Both reached it: one write, counted once — as the origin 0 windowd names.
    assert_eq!((both.writes.total(), both.by.unknown, both.by.zero, both.by.stray), (1, 1, 1, 0));
    // A change of a layer the set holds is neither.
    let below = present(&mut f, &[damage_from(3 << 16, 400, 400, 20, 10)]).by;
    assert_eq!((below.below, below.unknown, below.zero, below.stray), (1, 0, 0, 0));
}
