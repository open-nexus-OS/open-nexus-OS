// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a frame changes on the screen (TASK-0215 5c-3 P5, RFC-0101 §8): the damage a present
//! names, a layer that moves or goes, a glass blurred again only when something within its reach
//! changed, a plane's age. Split out of `frame_plan.rs` (structure gate).

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, History};
use support::*;

// What a frame changes on the screen (TASK-0215 5c-3 P5, RFC-0101 §8).

fn damage_from(origin: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin }
}

#[test]
fn a_present_names_its_damage_and_one_with_layers_but_none_is_damage_everywhere() {
    let mut f = Frame::new(2048);
    f.layers.take_frame(&[layer(100, 100, 50, 50), damage(0, 0, 8, 8)]);
    // The first plan: the layer is new, drawn where it was not.
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Rect { x0: 0, y0: 0, x1: 150, y1: 150 });
    // A pointer move with its damage alone: that rectangle.
    f.layers.take_frame(&[damage(10, 20, 30, 40)]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Rect { x0: 10, y0: 20, x1: 40, y1: 60 });
    // Nothing named, nothing moved: nothing to redraw.
    f.layers.take_frame(&[]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::None);
    // Layers without damage: everywhere.
    f.layers.take_frame(&[layer(100, 100, 50, 50)]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Full);
    // Two damage rectangles: their bounds.
    f.layers.take_frame(&[damage(0, 0, 8, 8), damage(100, 100, 8, 8)]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Rect { x0: 0, y0: 0, x1: 108, y1: 108 });
}

#[test]
fn a_layer_that_moves_or_goes_damages_where_it_was_and_where_it_is() {
    let mut f = Frame::new(2048);
    let shadowed =
        |x, y| with(layer(x, y, 50, 50), |l| (l.shadow_blur, l.shadow_offset_y) = (8, 4));
    f.layers.take_frame(&[shadowed(100, 100), damage(0, 0, 1, 1)]);
    f.plan(DISPLAY, &every_blur);
    // Moved, with the present's own damage elsewhere: both halos and the rectangle.
    f.layers.take_frame(&[shadowed(300, 100), damage(0, 0, 1, 1)]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Rect { x0: 0, y0: 0, x1: 362, y1: 162 });
    // The same drawing with a new content generation: the present's damage alone.
    f.layers.take_frame(&[with(shadowed(300, 100), |l| l.content_epoch = 9), damage(5, 5, 1, 1)]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Rect { x0: 5, y0: 5, x1: 6, y1: 6 });
    // Gone (an empty set keeps the set — a set of one other layer replaces it).
    f.layers.take_frame(&[layer(0, 0, 10, 10), damage(0, 0, 1, 1)]);
    let d = f.plan(DISPLAY, &every_blur).damage;
    assert!(d.touches(Area::rect(300, 100, 50, 50)), "{d:?}");
}

#[test]
fn a_glass_is_blurred_again_only_when_something_within_its_reach_changed() {
    let mut f = ready_frame(2048);
    // A window's glass (id 2): a change of the base under it is its backdrop's.
    let glass = with(layer(500, 500, 200, 100), |l| (l.backdrop_blur, l.layer_id) = (20, 2));
    f.layers.take_frame(&[layer(0, 0, 1920, 1080), glass.clone(), damage(0, 0, 1920, 1080)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(f.step(1).and_then(|s| s.glass).map(|g| g.cache), Some(CacheStep::Write(0)));
    assert_eq!(p.damage.on(DISPLAY), Area::Full.on(DISPLAY));
    // Damage far from the glass: read, and the frame's damage is that rectangle alone.
    f.layers.take_frame(&[damage(0, 0, 10, 10)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(f.step(1).and_then(|s| s.glass).map(|g| g.cache), Some(CacheStep::Read(0)));
    assert_eq!(p.damage, Area::Rect { x0: 0, y0: 0, x1: 10, y1: 10 });
    // Damage within the blur's reach of the glass: written again — in its band alone, so the
    // frame's damage is the rectangle and the part of the glass the band's reach touches
    // (TASK-0335 P1), not the whole glass.
    f.layers.take_frame(&[damage(450, 450, 10, 10)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(f.step(1).and_then(|s| s.glass).map(|g| g.cache), Some(CacheStep::Write(0)));
    assert_eq!(p.damage, Area::Rect { x0: 450, y0: 450, x1: 526, y1: 526 });
    // A content change below it, reported as damage under the glass: written again.
    f.layers.take_frame(&[
        with(layer(0, 0, 1920, 1080), |l| l.content_epoch = 5),
        glass,
        damage(600, 550, 1, 1),
    ]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(f.step(1).and_then(|s| s.glass).map(|g| g.cache), Some(CacheStep::Write(0)));
}

#[test]
fn a_plane_shown_frames_ago_lacks_the_frames_since_and_an_old_or_unknown_one_everything() {
    let mut h = History::new();
    assert_eq!(h.since(0), Area::Full, "never drawn");
    let f1 = h.record(Area::rect(0, 0, 10, 10));
    let f2 = h.record(Area::rect(100, 100, 10, 10));
    let f3 = h.record(Area::None);
    assert_eq!((f1, f2, f3), (1, 2, 3));
    assert_eq!(h.since(3), Area::None, "drawn at the newest frame: up to date");
    assert_eq!(h.since(2), Area::None);
    assert_eq!(h.since(1), Area::Rect { x0: 100, y0: 100, x1: 110, y1: 110 });
    assert_eq!(h.since(0), Area::Full);
    assert_eq!(h.since(9), Area::Full, "newer than the newest: unknown");
    for _ in 0..nexus_gfx::frame::HISTORY {
        h.record(Area::rect(5, 5, 1, 1));
    }
    assert_eq!(h.since(2), Area::Full, "older than the history holds");
    assert_eq!(h.since(3), Area::Rect { x0: 5, y0: 5, x1: 6, y1: 6 }, "the oldest it holds");
    assert_eq!(h.since(h.newest() - 1), Area::Rect { x0: 5, y0: 5, x1: 6, y1: 6 });
}

#[test]
fn test_reject_an_area_off_the_display_and_clamp_one_past_it() {
    assert_eq!(Area::rect(5, 5, 0, 3), Area::None);
    assert_eq!(Area::rect(u32::MAX, 0, 4, 4), Area::None, "wraps: nothing");
    assert_eq!(Area::rect(2000, 10, 4, 4).on(DISPLAY), None);
    assert_eq!(Area::rect(1900, 1070, 100, 100).on(DISPLAY), Some((1900, 1070, 1919, 1079)));
    assert_eq!(Area::Full.on(DISPLAY), Some((0, 0, 1919, 1079)));
    assert_eq!(Area::Full.pixels(DISPLAY), 1920 * 1080);
    assert_eq!(Area::None.union(Area::Full), Area::Full);
    assert!(!Area::rect(0, 0, 10, 10).touches(Area::rect(10, 0, 10, 10)), "edge to edge");
}

#[test]
fn a_glass_window_typed_into_keeps_its_blur_and_a_change_below_it_renews_it() {
    use nexus_gfx::frame::layers::ORIGIN_ABOVE;
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    // Window 1 (id 1) is glass; window 2 (id 2) lies above it, to its right.
    let win1 = with(layer(400, 300, 600, 400), |l| (l.backdrop_blur, l.layer_id) = (20, 1));
    let win2 = with(layer(900, 300, 400, 300), |l| l.layer_id = 2);
    f.layers.take_frame(&[base.clone(), win1.clone(), win2.clone(), damage(0, 0, 1920, 1080)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
    // Typing in window 1: damage inside it, from it. Its backdrop is unchanged.
    f.layers.take_frame(&[damage_from(1, 450, 350, 200, 40)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Read(0)));
    assert_eq!(p.damage, Area::Rect { x0: 450, y0: 350, x1: 650, y1: 390 }, "the frame redraws it");
    // Window 2 above it changes, overlapping window 1's reach: not below — unchanged backdrop.
    f.layers.take_frame(&[damage_from(2, 900, 300, 100, 100)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Read(0)));
    // The pointer over the glass: never below.
    f.layers.take_frame(&[damage_from(ORIGIN_ABOVE, 500, 400, 32, 32)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Read(0)));
    // The base under the glass changes: blurred again.
    f.layers.take_frame(&[damage_from(0, 500, 400, 10, 10)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
    // An origin the set does not know: taken as below.
    f.layers.take_frame(&[damage_from(7, 500, 400, 10, 10)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
    // More rectangles than the set keeps: everything.
    let many: Vec<Command> = (0..9).map(|i| damage_from(2, 10 * i, 0, 4, 4)).collect();
    f.layers.take_frame(&many);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.damage, Area::Full);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
}

#[test]
fn a_desktop_glass_region_keeps_its_blur_when_its_own_pixels_change() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    // A dock button: the desktop's own glass region (id 0) at the bottom.
    let button = with(layer(1740, 1020, 44, 44), |l| l.backdrop_blur = 40);
    f.layers.take_frame(&[base.clone(), button.clone(), damage(0, 0, 1920, 1080)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
    // The button's own pixels change (the desktop surface within its rectangle): read.
    f.layers.take_frame(&[damage(1744, 1024, 36, 36)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Read(0)));
    assert_eq!(p.damage, Area::Rect { x0: 1744, y0: 1024, x1: 1780, y1: 1060 });
    // The desktop beside it changes, within its reach: blurred again.
    f.layers.take_frame(&[damage(1700, 1024, 20, 20)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
    // A change straddling the region's edge is not its own.
    f.layers.take_frame(&[damage(1730, 1024, 20, 20)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
}

#[test]
fn a_window_of_two_glass_slices_keeps_both_blurs_when_typed_into() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let head = with(layer(400, 300, 600, 40), |l| (l.backdrop_blur, l.layer_id) = (20, 1));
    let body = with(layer(400, 340, 600, 400), |l| (l.backdrop_blur, l.layer_id) = (20, 1));
    f.layers.take_frame(&[base.clone(), head.clone(), body.clone(), damage(0, 0, 1920, 1080)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
    assert_eq!(cache_of(&f, 2).map(|c| matches!(c, CacheStep::Write(_))), Some(true));
    // Typing into the body: the window's own change, below neither of its slices.
    f.layers.take_frame(&[damage_from(1, 450, 400, 200, 40)]);
    f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 1), Some(CacheStep::Read(_))), "the head's glass kept");
    assert!(matches!(cache_of(&f, 2), Some(CacheStep::Read(_))), "the body's glass kept");
}

fn band_of(f: &Frame, i: usize) -> Option<Area> {
    f.step(i).and_then(|s| s.glass).map(|g| g.band)
}

#[test]
fn a_change_under_a_cached_glass_names_the_band_it_blurs_again() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let window = layer(200, 200, 600, 400);
    let pane = with(glass(layer(300, 300, 400, 200), 40), |l| l.layer_id = 2);
    f.layers.take_frame(&[base.clone(), window.clone(), pane.clone()]);
    f.plan(DISPLAY, &every_blur);
    // The first blur is the whole glass (new rows).
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Write(0)));
    assert_eq!(band_of(&f, 2), Some(Area::Full));
    // A small change in the window below, within the glass's reach: the band alone.
    f.layers.take_frame(&[
        base.clone(),
        window.clone(),
        pane.clone(),
        damage_from(0, 350, 350, 20, 10),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Write(0)));
    assert_eq!(band_of(&f, 2), Some(Area::rect(350, 350, 20, 10)));
    // The frame's damage: the band grown by the blur's reach, within the glass — not the glass.
    assert_eq!(
        p.damage,
        Area::rect(300, 300, 400, 200).intersect(Area::rect(350, 350, 20, 10).grown(66))
    );
    // A change beyond its reach: a read, no band.
    f.layers.take_frame(&[
        base.clone(),
        window.clone(),
        pane.clone(),
        damage_from(0, 1500, 900, 20, 10),
    ]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)));
    // The window below moves (the key changes): the whole glass again.
    f.layers.take_frame(&[base, layer(210, 200, 600, 400), pane, damage(0, 0, 1, 1)]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Write(0)));
    assert_eq!(band_of(&f, 2), Some(Area::Full));
}
