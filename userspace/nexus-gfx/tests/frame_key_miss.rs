// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A key miss is a band (TASK-0351 P6, C-c): a glass whose rows were current one plan ago and
//! whose key changed only through the layers below it — one drawn otherwise, one that came or
//! went, layers that swapped their order — blurs where those layers were and are within its
//! reach, never the whole glass. The first write after a hold, a new wallpaper and a change of
//! the glass itself stay whole. The rows against a full re-blur: `frame_reach_oracle.rs`.

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, Glass, Planned, Writes};
use support::*;

const BASE: u32 = 0xFFFF_0000;
const WINDOW: u32 = 0x1_0000;
/// The pane's rectangle and its reach (the rectangle grown by the blur's reach, 66).
const PANE: (u32, u32, u32, u32) = (300, 300, 400, 200);

fn reach() -> Area {
    Area::rect(PANE.0, PANE.1, PANE.2, PANE.3).grown(66)
}

fn base() -> Command {
    with(layer(0, 0, 1920, 1080), |l| (l.opaque, l.key) = (true, BASE))
}

/// A window (transform id 1) at `x`, `y`: 150 × 100, beside the pane, half within its reach.
fn window(x: u32, y: u32) -> Command {
    with(layer(x, y, 150, 100), |l| (l.layer_id, l.key, l.src_row_abs) = (1, WINDOW, 9000))
}

fn pane() -> Command {
    with(glass(layer(PANE.0, PANE.1, PANE.2, PANE.3), 40), |l| (l.layer_id, l.key) = (2, 0x2_0000))
}

fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn present(f: &mut Frame, cmds: &[Command]) -> Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &every_blur)
}

fn glass_of(f: &Frame, i: usize) -> Glass {
    f.step(i).and_then(|s| s.glass).expect("a glass step")
}

fn transform(layer_id: u32, dx: i32) -> Command {
    Command::LayerTransform {
        layer_id,
        dx,
        dy: 0,
        opacity: 255,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    }
}

/// A key miss written as a band: once, counted with its cause.
const BAND_MISS: Writes =
    Writes { band: 0, backdrop: 1, new: 0, full: 0, band_base: 0, backdrop_band: 1, hidden: 0 };

#[test]
fn a_key_miss_from_a_moved_layer_writes_its_old_and_new_extents() {
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(100, 320), pane()]);
    assert_eq!(glass_of(&f, 2).band, Area::Full, "new rows: the whole glass");
    // The window moves 30 right and 40 down: where it was and where it is, within the reach.
    let p = present(&mut f, &[base(), window(130, 360), pane(), no_damage()]);
    let g = glass_of(&f, 2);
    let both = Area::rect(100, 320, 150, 100).union(Area::rect(130, 360, 150, 100));
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), both.intersect(reach())));
    assert_eq!(g.band, Area::rect(234, 320, 46, 140));
    assert_eq!(p.writes, BAND_MISS);
    // What the write changes on the screen: the band grown by the reach, within the glass.
    let shows = g.band.grown(66).intersect(Area::rect(PANE.0, PANE.1, PANE.2, PANE.3));
    assert!(shows.inside(p.damage) && shows != Area::rect(PANE.0, PANE.1, PANE.2, PANE.3));
    // Nothing moves: the rows are read.
    let p = present(&mut f, &[base(), window(130, 360), pane(), no_damage()]);
    assert_eq!((p.writes.total(), cache_of(&f, 2)), (0, Some(CacheStep::Read(0))));
    // A move and a change of the base in the same plan: the band holds both.
    let p = present(&mut f, &[base(), window(100, 320), pane(), damage(600, 450, 20, 10)]);
    let moved = both.intersect(reach()).union(Area::rect(600, 450, 20, 10));
    assert_eq!((p.writes.backdrop_band, glass_of(&f, 2).band), (1, moved));
}

#[test]
fn a_key_miss_from_a_layer_that_came_or_went_writes_its_extent() {
    let its = Area::rect(100, 320, 150, 100).intersect(reach());
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), pane()]);
    // A window opens below the pane: where it is.
    let p = present(&mut f, &[base(), window(100, 320), pane(), no_damage()]);
    assert_eq!((p.writes, glass_of(&f, 2).band), (BAND_MISS, its));
    // It closes: where it was.
    let p = present(&mut f, &[base(), pane(), no_damage()]);
    assert_eq!((p.writes, glass_of(&f, 1).band), (BAND_MISS, its));
    // A window above the pane goes below it (the pane's backdrop gains it), and back.
    let p = present(&mut f, &[base(), pane(), window(100, 320), no_damage()]);
    assert_eq!(p.writes.total(), 0, "above the pane: none of its backdrop");
    let p = present(&mut f, &[base(), window(100, 320), pane(), no_damage()]);
    assert_eq!((p.writes, glass_of(&f, 2).band), (BAND_MISS, its), "came from above");
    let p = present(&mut f, &[base(), pane(), window(100, 320), no_damage()]);
    assert_eq!((p.writes, glass_of(&f, 1).band), (BAND_MISS, its), "went above");
}

#[test]
fn test_reject_a_band_for_the_first_write_after_a_hold() {
    // The window below moves by its transform (the pane is held), then rests where it went:
    // the write after the hold is the whole glass — the rows missed what the hold kept.
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(100, 320), pane()]);
    for dx in 1..=8 {
        let p = present(&mut f, &[transform(1, dx), no_damage()]);
        assert_eq!((p.writes.total(), p.holds.held), (0, 1), "held at {dx}");
    }
    let p = present(&mut f, &[transform(1, 0), base(), window(108, 320), pane(), no_damage()]);
    let g = glass_of(&f, 2);
    assert_eq!((g.cache, g.band, p.by.rest), (CacheStep::Write(0), Area::Full, 1));
    assert_eq!((p.writes.backdrop, p.writes.backdrop_band), (1, 0));
}

#[test]
fn a_held_glass_made_to_owe_the_whole_glass_by_a_displaced_change_is_counted() {
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(100, 320), pane()]);
    // Held while the window is displaced, the window changes: the glass owes all of itself.
    let change = Command::Damage {
        rect: TileRect { x: 260, y: 330, width: 10, height: 10 },
        origin: WINDOW,
    };
    let p = present(&mut f, &[transform(1, 30), change]);
    assert_eq!((p.holds.held, p.holds.foreign), (1, 1));
    // A displaced plan that changes nothing owes nothing more: not counted.
    let p = present(&mut f, &[transform(1, 31), no_damage()]);
    assert_eq!((p.holds.held, p.holds.foreign), (1, 0));
}

#[test]
fn test_reject_a_band_for_a_key_change_no_layer_explains() {
    // The window moves and the wallpaper is uploaded again in the same plan: the backdrop's
    // seed changed, no layer explains it — the whole glass.
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(100, 320), pane()]);
    f.glass.wallpaper_epoch += 1;
    let p = present(&mut f, &[base(), window(130, 360), pane(), no_damage()]);
    assert_eq!((glass_of(&f, 2).cache, glass_of(&f, 2).band), (CacheStep::Write(0), Area::Full));
    assert_eq!((p.writes.backdrop, p.writes.backdrop_band), (1, 0));
    // The pane's own corner radius changes (its rows stay): the glass itself, not a layer.
    let rounded = with(pane(), |l| l.corner_radius = 12);
    let p = present(&mut f, &[base(), window(130, 360), rounded.clone(), no_damage()]);
    assert_eq!((glass_of(&f, 2).cache, glass_of(&f, 2).band), (CacheStep::Write(0), Area::Full));
    assert_eq!((p.writes.backdrop, p.writes.backdrop_band), (1, 0));
    // The plan after a whole write is current again: a move is a band.
    let p = present(&mut f, &[base(), window(100, 320), rounded, no_damage()]);
    assert_eq!(p.writes, BAND_MISS);
}
