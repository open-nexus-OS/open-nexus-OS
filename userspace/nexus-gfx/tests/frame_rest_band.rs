// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A key miss at rest is a band (TASK-0351 P19, C-r — C-c and C-r are one rule): while a glass
//! is held, every plan's change of the layers below it (`Moves::below`) is kept with its rows,
//! and the write that ends the hold blurs that band with what the rows owe — when the rows were
//! blurred from the same base and the band is a rectangle. A foreign change while the layers
//! are displaced (a Full owe) and a new wallpaper (another base) stay whole. The rows against a
//! full re-blur: `frame_reach_oracle.rs`.

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, Glass, Planned};
use support::*;

const BASE: u32 = 0xFFFF_0000;
const WINDOW: u32 = 0x1_0000;
/// The pane's rectangle; its reach is the rectangle grown by the blur's reach (66).
const PANE: (u32, u32, u32, u32) = (300, 300, 400, 200);

fn pane_rect() -> Area {
    Area::rect(PANE.0, PANE.1, PANE.2, PANE.3)
}

fn base() -> Command {
    with(layer(0, 0, 1920, 1080), |l| (l.opaque, l.key) = (true, BASE))
}

/// A window (transform id 1) at `x`, `y`: 150 × 100, beside the pane, half within its reach.
fn window(x: u32, y: u32) -> Command {
    with(layer(x, y, 150, 100), |l| (l.layer_id, l.key, l.src_row_abs) = (1, WINDOW, 9000))
}

/// Another window (transform id 3) that comes below the pane.
fn other() -> Command {
    with(layer(500, 450, 100, 60), |l| (l.layer_id, l.key, l.src_row_abs) = (3, 0x3_0000, 7000))
}

/// The glass of window 2.
fn pane() -> Command {
    with(glass(layer(PANE.0, PANE.1, PANE.2, PANE.3), 40), |l| (l.layer_id, l.key) = (2, 0x2_0000))
}

fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn transform(layer_id: u32, dx: i32, opacity: u32) -> Command {
    Command::LayerTransform {
        layer_id,
        dx,
        dy: 0,
        opacity,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    }
}

fn present(f: &mut Frame, cmds: &[Command]) -> Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &every_blur)
}

fn glass_of(f: &Frame, i: usize) -> Glass {
    f.step(i).and_then(|s| s.glass).expect("a glass step")
}

/// The pane written over the window at 100, then held while the window moves right by its
/// transform, one pixel a plan for `plans` plans — `during(f, dx)` done before each.
fn held_over_the_mover(plans: i32, during: fn(&mut Frame, i32) -> Command) -> Frame {
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(100, 320), pane()]);
    assert_eq!(glass_of(&f, 2).band, Area::Full, "new rows: the whole glass");
    for dx in 1..=plans {
        let named = during(&mut f, dx);
        let p = present(&mut f, &[transform(1, dx, 255), named]);
        assert_eq!((p.writes.total(), p.holds.held), (0, 1), "held at {dx}");
    }
    f
}

fn quiet(_: &mut Frame, _: i32) -> Command {
    no_damage()
}

#[test]
fn a_glass_held_over_a_mover_is_written_at_rest_as_the_mover_s_spans() {
    let mut f = held_over_the_mover(8, quiet);
    // The window rests where it went (108): the band is every place it was drawn meanwhile,
    // 100..258, within the pane's reach (from 234) — never the whole glass.
    let p = present(&mut f, &[transform(1, 0, 255), base(), window(108, 320), pane(), no_damage()]);
    let g = glass_of(&f, 2);
    let spans = Area::rect(100, 320, 158, 100).intersect(pane_rect().grown(66));
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), spans));
    assert_eq!(g.band, Area::rect(234, 320, 24, 100));
    assert_eq!((p.by.rest, p.writes.backdrop, p.writes.backdrop_band), (1, 1, 1));
    assert_eq!((p.by.misses.moved, p.by.misses.band), (1, 1));
    // What the write changes on the screen: the band grown by the reach, within the glass.
    let shows = g.band.grown(66).intersect(pane_rect());
    assert!(shows.inside(p.damage) && !pane_rect().inside(p.damage));
    // Nothing moves: the rows are read.
    let p = present(&mut f, &[base(), window(108, 320), pane(), no_damage()]);
    assert_eq!((p.writes.total(), cache_of(&f, 2)), (0, Some(CacheStep::Read(0))));
}

#[test]
fn a_layer_that_came_below_a_held_glass_is_its_rest_band() {
    // The pane's own window fades (the pane is held); meanwhile another window opens below it.
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), window(100, 320), pane()]);
    let p = present(&mut f, &[transform(2, 0, 128), no_damage()]);
    assert_eq!((p.writes.total(), p.holds.held), (0, 1));
    let came = [transform(2, 0, 160), base(), window(100, 320), other(), pane(), no_damage()];
    let p = present(&mut f, &came);
    assert_eq!((p.writes.total(), p.holds.held, p.holds.foreign), (0, 1, 0));
    let p = present(&mut f, &[transform(2, 0, 200), no_damage()]);
    assert_eq!((p.writes.total(), p.holds.held), (0, 1));
    // At rest: the band is where the window came, nothing else of the glass.
    let p = present(&mut f, &[transform(2, 0, 255), no_damage()]);
    let g = glass_of(&f, 3);
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), Area::rect(500, 450, 100, 60)));
    assert_eq!(
        (p.by.rest, p.by.misses.set, p.by.misses.band, p.writes.backdrop_band),
        (1, 1, 1, 1)
    );
}

#[test]
fn test_reject_a_rest_band_across_a_foreign_change() {
    // Held while the window is displaced, the window changes (another window than the pane's):
    // where that change lies once the layers are back nothing says — the rows owe the whole
    // glass, and the write after the hold is the whole glass.
    let mut f = held_over_the_mover(6, |_, dx| {
        let rect = TileRect { x: 260, y: 330, width: 10, height: 10 };
        if dx == 3 {
            Command::Damage { rect, origin: WINDOW }
        } else {
            no_damage()
        }
    });
    let p = present(&mut f, &[transform(1, 0, 255), base(), window(106, 320), pane(), no_damage()]);
    let g = glass_of(&f, 2);
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), Area::Full));
    assert_eq!(
        (p.by.rest, p.writes.backdrop, p.writes.backdrop_band, p.by.misses.band),
        (1, 1, 0, 0)
    );
    // The same for a change of the desktop's base below the pane.
    let mut f =
        held_over_the_mover(
            6,
            |_, dx| if dx == 2 { damage(400, 400, 20, 10) } else { no_damage() },
        );
    let p = present(&mut f, &[transform(1, 0, 255), base(), window(106, 320), pane(), no_damage()]);
    assert_eq!(
        (glass_of(&f, 2).band, p.writes.backdrop_band, p.by.misses.band),
        (Area::Full, 0, 0)
    );
}

#[test]
fn test_reject_a_rest_band_across_a_wallpaper_epoch() {
    // Held over the mover, the wallpaper is uploaded again: the rows were blurred from another
    // base, no layer explains the change — the whole glass.
    let mut f = held_over_the_mover(6, |f, dx| {
        if dx == 3 {
            f.glass.wallpaper_epoch += 1;
        }
        no_damage()
    });
    let p = present(&mut f, &[transform(1, 0, 255), base(), window(106, 320), pane(), no_damage()]);
    let g = glass_of(&f, 2);
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), Area::Full));
    assert_eq!((p.by.misses.epoch, p.by.misses.band, p.writes.backdrop_band), (1, 0, 0));
    // The plan after the whole write follows the new base: a move is a band again.
    let p = present(&mut f, &[base(), window(130, 320), pane(), no_damage()]);
    assert_eq!((p.writes.backdrop_band, glass_of(&f, 2).band), (1, Area::rect(234, 320, 46, 100)));
}
