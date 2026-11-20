// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Which changes reach a glass (TASK-0351 P6, RFC-0101 §8): a damage rectangle's origin is the
//! key of the layer that draws it — it blurs the glasses above that layer again and no other
//! (R1), ends at an opaque layer that covers it (R2); a glass blurred again is a change for the
//! glasses above it (R3); layers that swap their order damage their overlap (R4); an unknown
//! origin is below everything. The write counts are the architect's (T1–T5, T7).

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, Planned, Writes, WritesBy};
use support::*;

fn damage_from(origin: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin }
}

/// The damage a present names when it is owed for its layers alone.
fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

/// A frame that drew `layers` once (everything blurred), ready for the next present.
fn settled(layers: &[Command]) -> Frame {
    let mut f = ready_frame(4096);
    f.layers.take_frame(layers);
    f.plan(DISPLAY, &every_blur);
    f
}

fn present(f: &mut Frame, cmds: &[Command]) -> Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &every_blur)
}

fn band_of(f: &Frame, i: usize) -> Option<Area> {
    f.step(i).and_then(|s| s.glass).map(|g| g.band)
}

const BASE: u32 = 0xFFFF_0000;
const PANEL: u32 = 0xFFFF_0300;

fn base() -> Command {
    with(layer(0, 0, 1920, 1080), |l| (l.opaque, l.key) = (true, BASE))
}

/// The base, three glass windows side by side and a shell panel across them, above.
fn three_windows_under_a_panel() -> Vec<Command> {
    let window = |n: u32| {
        with(glass(layer(100 + 500 * (n - 1), 200, 400, 300), 40), |l| {
            (l.layer_id, l.key) = (n, n << 16)
        })
    };
    let panel = with(layer(80, 150, 1500, 100), |l| l.key = PANEL);
    vec![base(), window(1), window(2), window(3), panel]
}

/// T1: a panel's content changes over three glass windows. Named by the panel's key it is
/// above all three: nothing blurs. With origin 0 the same change blurs all three.
#[test]
fn a_panel_s_own_change_blurs_no_window_under_it() {
    let mut f = settled(&three_windows_under_a_panel());
    let p = present(&mut f, &[damage_from(PANEL, 100, 160, 1400, 60)]);
    assert_eq!(p.writes.total(), 0, "the panel is above every glass");
    assert_eq!(p.damage, Area::rect(100, 160, 1400, 60), "the frame redraws the change alone");
    for i in 1..=3 {
        assert!(matches!(cache_of(&f, i), Some(CacheStep::Read(_))), "window {i} reads its rows");
    }
    let p = present(&mut f, &[damage(100, 160, 1400, 60)]);
    assert_eq!(p.writes, Writes { band: 3, band_base: 3, ..Default::default() });
    assert_eq!(p.by, WritesBy { unknown: 3, zero: 3, ..Default::default() });
}

/// T7: a key the set does not hold is as origin 0 — below every glass, never above one, and
/// never behind a cover (its layer may lie above it).
#[test]
fn test_reject_an_unknown_origin_as_above() {
    let (mut stray, mut zero) =
        (settled(&three_windows_under_a_panel()), settled(&three_windows_under_a_panel()));
    let p = present(&mut stray, &[damage_from(0xDEAD_0000, 100, 160, 1400, 60)]);
    let q = present(&mut zero, &[damage(100, 160, 1400, 60)]);
    assert_eq!(
        (p.writes.total(), p.by),
        (3, WritesBy { unknown: 3, stray: 3, ..Default::default() })
    );
    // The same writes, counted apart (W4): a stray key is not the origin 0 windowd queues.
    assert_eq!((p.writes.band, p.damage, p.by.unknown), (q.writes.band, q.damage, q.by.unknown));
    assert_eq!((p.by.stray, p.by.zero, q.by.stray, q.by.zero), (3, 0, 0, 3));
    for i in 1..=3 {
        assert!(matches!(cache_of(&stray, i), Some(CacheStep::Write(_))), "window {i}");
        assert_eq!(stray.step(i), zero.step(i), "window {i}: the step of origin 0");
    }
    // Under an opaque window that covers it, a change of unknown origin still reaches the
    // glass above: only a change whose layer is known to lie below the cover ends at it.
    let cover =
        with(layer(320, 320, 300, 200), |l| (l.opaque, l.layer_id, l.key) = (true, 4, 4 << 16));
    let pane = with(glass(layer(300, 300, 400, 260), 40), |l| (l.layer_id, l.key) = (5, 5 << 16));
    let mut f = settled(&[base(), cover, pane]);
    for unknown in [0, 0xDEAD_0000] {
        let p = present(&mut f, &[damage_from(unknown, 400, 400, 40, 20)]);
        assert_eq!(p.writes.total(), 1, "origin {unknown:#x}");
    }
    assert_eq!(present(&mut f, &[damage_from(BASE, 400, 400, 40, 20)]).writes.total(), 0);
}

/// T2: the base changes under a glass, an opaque window between them covering the change: the
/// glass's backdrop is the window there, nothing blurs and nothing is owed. Without the window
/// the same change blurs the glass.
#[test]
fn a_change_under_an_opaque_layer_ends_there() {
    let cover =
        with(layer(320, 320, 300, 200), |l| (l.opaque, l.layer_id, l.key) = (true, 4, 4 << 16));
    let pane = with(glass(layer(300, 300, 400, 260), 40), |l| (l.layer_id, l.key) = (5, 5 << 16));
    let change = damage_from(BASE, 400, 400, 40, 20);
    let mut f = settled(&[base(), cover.clone(), pane.clone()]);
    let p = present(&mut f, &[change.clone()]);
    assert_eq!(p.writes, Writes::default(), "covered: no write, nothing put off");
    assert_eq!(p.damage, Area::rect(400, 400, 40, 20));
    assert_eq!(present(&mut f, &[no_damage()]).writes.total(), 0, "and nothing owed");
    // A change the window covers only in part reaches the glass; so does the window's own.
    assert_eq!(present(&mut f, &[damage_from(BASE, 300, 400, 40, 20)]).writes.total(), 1);
    let p = present(&mut f, &[damage_from(4 << 16, 400, 400, 40, 20)]);
    assert_eq!((p.writes.total(), p.by.below), (1, 1));
    // The cover removed: the base's change is the glass's backdrop.
    let mut f = settled(&[base(), pane]);
    let p = present(&mut f, &[change]);
    assert_eq!(p.writes, Writes { band: 1, ..Default::default() });
    assert_eq!(p.by, WritesBy { below: 1, ..Default::default() });
}

/// T3: two glasses, the upper within reach of the lower. A change below the lower blurs it
/// again, and what the lower then shows is a change for the upper — though the named rectangle
/// lies out of the upper's reach. A change out of the lower's reach blurs neither.
#[test]
fn a_glass_blurred_again_is_a_change_for_the_glass_above_it() {
    let lower = with(glass(layer(300, 300, 400, 200), 40), |l| (l.layer_id, l.key) = (1, 1 << 16));
    let upper = with(glass(layer(420, 320, 100, 60), 20), |l| (l.layer_id, l.key) = (2, 2 << 16));
    let mut f = settled(&[base(), lower, upper]);
    let p = present(&mut f, &[damage(320, 350, 20, 10)]);
    assert_eq!(p.writes.total(), 2);
    assert_eq!(p.by, WritesBy { unknown: 1, zero: 1, glass: 1, ..Default::default() });
    // The lower shows its band grown by the blur's reach, within itself; the upper blurs that
    // again where it lies in its own reach.
    let shown = Area::rect(320, 350, 20, 10).grown(66).intersect(Area::rect(300, 300, 400, 200));
    assert_eq!(band_of(&f, 1), Some(Area::rect(320, 350, 20, 10)));
    assert_eq!(band_of(&f, 2), Some(shown.intersect(Area::rect(420, 320, 100, 60).grown(66))));
    assert!(shown.inside(p.damage), "what the lower shows is redrawn");
    let p = present(&mut f, &[damage(1500, 900, 20, 10)]);
    assert_eq!(p.writes.total(), 0, "out of reach: both read their rows");
}

/// T4: two overlapping windows swap their order and the present names no damage: neither is
/// drawn otherwise, but where they overlap the other one shows now.
#[test]
fn layers_that_swap_their_order_damage_their_overlap() {
    let one = with(layer(200, 200, 400, 300), |l| (l.layer_id, l.key) = (1, 1 << 16));
    let two = with(layer(500, 400, 400, 300), |l| (l.layer_id, l.key) = (2, 2 << 16));
    let mut f = settled(&[base(), one.clone(), two.clone()]);
    let p = present(&mut f, &[base(), two.clone(), one.clone(), no_damage()]);
    assert_eq!(p.damage, Area::rect(500, 400, 100, 100), "their overlap, and nothing else");
    assert_eq!(
        present(&mut f, &[base(), two.clone(), one.clone(), no_damage()]).damage,
        Area::None
    );
    // A glass over the overlap blurs again — its backdrop's order changed (the class says so) —
    // where they overlap alone (C-c: the reorder explains the key).
    let pane = with(glass(layer(450, 350, 200, 200), 40), |l| (l.layer_id, l.key) = (3, 3 << 16));
    let mut f = settled(&[base(), one.clone(), two.clone(), pane.clone()]);
    let p = present(&mut f, &[base(), two, one, pane, no_damage()]);
    assert_eq!(p.writes, Writes { backdrop: 1, backdrop_band: 1, ..Default::default() });
    assert_eq!(band_of(&f, 3), Some(Area::rect(500, 400, 100, 100)));
    assert_eq!(p.by, WritesBy { order: 1, ..Default::default() });
    assert!(Area::rect(500, 400, 100, 100).inside(p.damage));
}

/// T5: a window opens, and closes, in presents that name no damage: the frame redraws where
/// the window is, or was — its rectangle and its shadow's halo.
#[test]
fn a_window_that_opens_or_closes_damages_its_extent() {
    let window = with(layer(200, 200, 400, 300), |l| {
        (l.layer_id, l.key) = (1, 1 << 16);
        (l.shadow_blur, l.shadow_offset_y, l.shadow_alpha) = (24, 8, 102);
    });
    let extent = Area::rect(200, 200, 400, 300).grown(32);
    let mut f = settled(&[base()]);
    assert_eq!(present(&mut f, &[base(), window, no_damage()]).damage, extent, "opened");
    assert_eq!(present(&mut f, &[base(), no_damage()]).damage, extent, "closed");
    assert_eq!(present(&mut f, &[base(), no_damage()]).damage, Area::None);
}

/// The classes of a write: a named layer below, a layer below that moved, and the one write
/// after a motion.
#[test]
fn a_write_names_what_reached_the_glass() {
    let window = |x: u32| with(layer(x, 200, 600, 400), |l| (l.layer_id, l.key) = (1, 1 << 16));
    let pane = with(glass(layer(300, 300, 400, 200), 40), |l| (l.layer_id, l.key) = (2, 2 << 16));
    let mut f = settled(&[base(), window(200), pane.clone()]);
    let p = present(&mut f, &[damage_from(1 << 16, 350, 350, 20, 10)]);
    assert_eq!(p.by, WritesBy { below: 1, ..Default::default() });
    let p = present(&mut f, &[base(), window(210), pane.clone(), no_damage()]);
    assert_eq!(p.by, WritesBy { moved: 1, ..Default::default() });
    // The pane's window moves: its rows are kept; at rest they are written once.
    let moving = Command::LayerTransform {
        layer_id: 2,
        dx: 40,
        dy: 10,
        opacity: 255,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    };
    let rest = Command::LayerTransform {
        layer_id: 2,
        dx: 0,
        dy: 0,
        opacity: 255,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    };
    let p = present(&mut f, &[moving, base(), window(210), pane.clone(), no_damage()]);
    assert_eq!((p.writes.total(), p.by), (0, WritesBy::default()));
    let p = present(&mut f, &[rest, base(), window(210), pane, damage(350, 350, 20, 10)]);
    assert_eq!((p.writes.total(), p.by), (1, WritesBy { rest: 1, ..Default::default() }));
}
