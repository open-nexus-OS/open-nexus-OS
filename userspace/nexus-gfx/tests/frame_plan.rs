// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The frame planner (TASK-0215 5c-3 P4): the retained set, its transforms, the glass cache and
//! the atlas upload as every executor sees them. What a frame changes on the screen is
//! `frame_damage.rs`.

#[path = "frame_support.rs"]
mod support;

use nexus_gfx::command::buffer::{Command, RgbaColor};
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::glass::{fold_layer, seed};
use nexus_gfx::frame::{
    Area, CacheStep, Frame, FrameLayer, GlassCache, Planned, RetainedLayers, Transform, MAX_LAYERS,
};
use support::*;

#[test]
fn a_present_without_layers_keeps_the_set() {
    let mut set = RetainedLayers::new();
    let taken = set.take_frame(&[layer(0, 0, 1920, 1080), layer(100, 100, 400, 300)]);
    assert!(taken.replaced);
    assert_eq!(set.len(), 2);
    // A pointer move brings no layer: the set stays, the frame composes it whole.
    let taken = set.take_frame(&[Command::BlendCursor { x: 5, y: 5, width: 32, height: 32 }]);
    assert!(!taken.replaced);
    assert_eq!(set.len(), 2);
}

#[test]
fn a_new_set_brings_the_scrolled_row_with_the_transforms_it_names() {
    let mut set = RetainedLayers::new();
    let body = |row| {
        with(layer(100, 100, 400, 300), |l| {
            l.layer_id = 1;
            l.src_row_abs = row;
        })
    };
    let fade = Command::LayerTransform {
        layer_id: 1,
        dx: 0,
        dy: 0,
        opacity: 128,
        scale: 10_000,
        origin_x: 0,
        origin_y: 0,
    };
    set.take_frame(&[fade.clone(), body(5400)]);
    // A scroll is a present (TASK-0338): the body comes with its new row and its window's
    // transform — the animation's state, named by every present that brings layers.
    set.take_frame(&[fade, body(7000)]);
    let l = set.effective(0).expect("a layer");
    assert_eq!(l.src_row_abs, 7000);
    assert_eq!(l.opacity, 128);
    // A present without layers keeps the set and its transforms.
    let taken = set.take_frame(&[]);
    assert!(!taken.replaced);
    assert_eq!(set.effective(0).map(|l| l.opacity), Some(128));
}

/// TASK-0354 P3: a transform scales every layer of its group about ONE origin, then moves them —
/// a window's body and the region inside it stay together while they grow (each scaled about
/// its own centre, the region drifted off the body).
#[test]
fn the_transform_scales_a_group_about_its_origin_then_moves_and_fades_it() {
    let mut set = RetainedLayers::new();
    set.take_frame(&[
        with(layer(100, 100, 400, 300), |l| l.layer_id = 2),
        with(layer(140, 120, 100, 50), |l| l.layer_id = 2),
    ]);
    let t = Transform { dx: 10, dy: -20, opacity: 51, scale: 5_000, origin: (300, 250) };
    set.set_transform(2, t);
    let body = set.effective(0).expect("the body");
    // Half the size about (300, 250) — the body's centre — then moved by (10, -20).
    assert_eq!((body.dst_x, body.dst_y, body.width, body.height), (210, 155, 200, 150));
    assert_eq!(body.opacity, 51);
    // The content band keeps its size: the executor spreads it over the scaled layer.
    assert_eq!((body.content_w, body.content_h), (400, 300));
    // The region keeps its place in the body: 40, 20 in at full size, 20, 10 at half.
    let region = set.effective(1).expect("the region");
    assert_eq!((region.dst_x, region.dst_y, region.width, region.height), (230, 165, 50, 25));
    // A fine step: 0.97 of 400 is 388 — no 1 % jumps.
    set.set_transform(2, Transform { scale: 9_700, ..t });
    assert_eq!(set.effective(0).map(|l| l.width), Some(388));
    // A move past the screen's edge stops at it.
    set.set_transform(2, Transform { dx: -500, ..Transform::IDENTITY });
    assert_eq!(set.effective(0).map(|l| l.dst_x), Some(0));
}

/// RFC-0114 §7: a transform id is a window's key — any of `1..=0xFFFF`, never a table index;
/// id 0 names no window, and the set holds one transform per window it can draw.
#[test]
fn test_reject_transforms_for_id_zero_or_past_the_windows_a_frame_draws() {
    let mut set = RetainedLayers::new();
    let t = Transform { dx: 1, dy: 1, ..Transform::IDENTITY };
    assert!(!set.set_transform(0, t));
    assert!(set.set_transform(9, t), "a key past the old eight slots");
    assert!(set.set_transform(0xFFFF, t), "the desktop's key");
    for id in 1..=MAX_LAYERS as u32 {
        let _ = set.set_transform(0x100 + id, t);
    }
    assert!(!set.set_transform(0x7777, t), "one window more than a frame draws");
    assert!(set.set_transform(9, Transform::IDENTITY), "a held key updates in place");
}

/// A present with layers names every window's transform: a closed window's goes with it, a
/// window it names none of is at rest — a new window never inherits a faded-out one.
#[test]
fn a_present_with_layers_replaces_the_transforms() {
    let mut set = RetainedLayers::new();
    set.take_frame(&[with(layer(100, 100, 400, 300), |l| l.layer_id = 7)]);
    set.set_transform(7, Transform { opacity: 0, ..Transform::IDENTITY });
    assert!(set.in_motion(7));
    set.take_frame(&[with(layer(100, 100, 400, 300), |l| l.layer_id = 7)]);
    assert!(!set.in_motion(7), "named no transform: at rest");
    assert_eq!(set.effective(0).map(|l| l.opacity), Some(255));
}

#[test]
fn layers_past_the_capacity_are_counted_not_kept() {
    let mut set = RetainedLayers::new();
    let cmds: Vec<Command> = (0..MAX_LAYERS as u32 + 3).map(|i| layer(i, 0, 10, 10)).collect();
    let taken = set.take_frame(&cmds);
    assert_eq!(taken.dropped, 3);
    assert_eq!(set.len(), MAX_LAYERS);
    assert_eq!(set.effective(MAX_LAYERS), None);
}

#[test]
fn the_plan_covers_the_layers_the_glass_recipe_and_the_pointer_and_nothing_else() {
    let blit =
        Command::BlitSurface { src_x: 0, src_y: 1080, dst_x: 0, dst_y: 0, width: 8, height: 8 };
    let pointer = Command::BlendCursor { x: 4, y: 4, width: 16, height: 16 };
    assert!(RetainedLayers::covers(&[blit.clone(), layer(0, 0, 10, 10), pointer.clone()]));
    assert!(RetainedLayers::covers(&[]), "no layers: the plan draws the set it keeps");
    // A glass layer as the layer encoder emits it: the backdrop restored, blurred, cached, then
    // the layer with its blur radius (the plan's own glass).
    let rect = TileRect { x: 0, y: 0, width: 8, height: 8 };
    let glass = [
        blit.clone(),
        Command::BlurBackdrop { rect, radius: 30, saturation_percent: 140 },
        Command::BlitAbsolute {
            src_x: 0,
            src_y_abs: 2160,
            dst_x: 0,
            dst_y_abs: 5400,
            width: 8,
            height: 8,
        },
        with(layer(0, 0, 8, 8), |l| l.backdrop_blur = 30),
        pointer.clone(),
    ];
    assert!(RetainedLayers::covers(&glass), "the glass recipe beside its layer");
    let fill = Command::FillSdfRoundedRect { rect, radius: 4, color: RgbaColor(0xff00_00ff) };
    assert!(!RetainedLayers::covers(&[blit, fill, pointer]), "a fill the plan would lose");
    let full: Vec<Command> = (0..MAX_LAYERS as u32).map(|i| layer(i, 0, 10, 10)).collect();
    assert!(RetainedLayers::covers(&full));
    let past: Vec<Command> = (0..MAX_LAYERS as u32 + 1).map(|i| layer(i, 0, 10, 10)).collect();
    assert!(!RetainedLayers::covers(&past), "a layer the set cannot hold");
}

#[test]
fn a_glass_reads_its_cache_until_something_below_it_changes() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let window = layer(200, 200, 600, 400);
    let pane = glass(layer(300, 300, 400, 200), 20);
    let above = layer(900, 100, 200, 200);
    f.layers.take_frame(&[base.clone(), window.clone(), pane.clone(), above.clone()]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Write(0)));
    // The same frame again (a pointer move): one draw from the cache.
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)));
    // A layer above the glass changes (the present names where): its backdrop does not.
    f.layers.take_frame(&[
        base.clone(),
        window.clone(),
        pane.clone(),
        layer(950, 100, 200, 200),
        damage(900, 100, 250, 200),
    ]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)));
    // The window below moves: the glass is blurred again.
    f.layers.take_frame(&[
        base.clone(),
        layer(210, 200, 600, 400),
        pane.clone(),
        above.clone(),
        damage(0, 0, 1, 1),
    ]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Write(0)));
    // A transform override below counts as a change too.
    f.layers.take_frame(&[base, with(window, |l| l.layer_id = 1), pane, above]);
    f.plan(DISPLAY, &every_blur);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)));
    // …once it shows (R6/T9, `build/agents/arch-transition-every-frame-2026-11-19.md`): a
    // layer moving below holds the glass while it travelled less than the blur hides…
    f.layers.set_transform(1, Transform { dx: 5, ..Transform::IDENTITY });
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)));
    // …and the glass is blurred again once it moved further than that (radius 20).
    f.layers.set_transform(1, Transform { dx: 40, ..Transform::IDENTITY });
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Write(0)));
}

#[test]
fn a_glass_the_executor_cannot_blur_takes_no_slot() {
    let mut f = ready_frame(2048);
    f.layers.take_frame(&[glass(layer(0, 0, 100, 100), 8), glass(layer(0, 200, 100, 100), 20)]);
    f.plan(DISPLAY, &|r| r != 8);
    assert_eq!(f.step(0).and_then(|s| s.glass), None);
    // The second glass packs where the first would have gone.
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(0)));
}

#[test]
fn a_glass_without_rows_is_off_and_counted_short_until_the_cache_has_a_plane_for_it() {
    // Two glass layers of 100 rows (27 quarter-resolution rows each at radius 20) in a cache of
    // one plane of 40 rows.
    let mut f = ready_frame(40);
    let a = glass(layer(0, 0, 100, 100), 20);
    let b = glass(layer(0, 200, 100, 100), 20);
    f.layers.take_frame(&[a.clone(), b]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 0), Some(CacheStep::Write(0)));
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Off));
    assert_eq!(f.step(1).and_then(|s| s.glass).map(|g| g.rows), Some(27));
    assert_eq!(f.glass.short(), 27, "what a further plane is asked for");
    // The executor adds a plane: the second glass takes its first rows in the same frame, the
    // first keeps its own.
    assert_eq!(f.place_short(), 1);
    assert_eq!(cache_of(&f, 0), Some(CacheStep::Write(0)));
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Write(40)), "a glass's rows lie in one plane");
    assert_eq!((f.glass.short(), f.glass.planes()), (0, 2));
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 0), Some(CacheStep::Read(0)));
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Read(40)));
    // Without the executor's cache no glass is planned at all.
    let mut cold = Frame::new(2048);
    cold.layers.take_frame(&[a]);
    cold.plan(DISPLAY, &every_blur);
    assert_eq!(cold.step(0).and_then(|s| s.glass), None);
}

/// Twelve glass windows and the shell's glass, then a window opened under them and one closed
/// (TASK-0351, the board at twelve windows: rows for five, the rest without a blur and their
/// whole rectangle damaged every frame; rows packed in walk order, so a window opened or
/// closed below blurred every glass behind it again).
#[test]
fn a_glass_keeps_its_rows_while_the_set_around_it_changes() {
    let window = |k: u32| with(glass(layer(40 * k, 30 * k, 1440, 853), 40), |l| l.key = 0x0100 + k);
    let mut f = ready_frame(1024);
    let mut set: Vec<_> = (0..12).map(window).collect();
    // The cache grows as the executor does: a plane while the plan is short of rows — in the
    // frame that found it short, so no glass is ever shown without its backdrop.
    f.layers.take_frame(&set);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!((p.writes.full, f.glass.short()), (4, 8 * 216), "rows for four of twelve");
    let mut placed = 0;
    while f.glass.short() != 0 {
        placed += f.place_short();
    }
    assert_eq!((placed, f.glass.planes()), (8, 3), "216 rows a window: four a plane");
    let rows: Vec<_> = (0..12).map(|i| cache_of(&f, i)).collect();
    assert!(rows.iter().all(|c| matches!(c, Some(CacheStep::Read(_) | CacheStep::Write(_)))));
    f.plan(DISPLAY, &every_blur);
    let settled: Vec<_> = (0..12).map(|i| cache_of(&f, i)).collect();
    assert!(settled.iter().all(|c| matches!(c, Some(CacheStep::Read(_)))), "{settled:?}");
    // The third window closes: nothing behind it moves, nothing is written for its rows.
    // (A named damage: a present without one blurs everything again, named full.)
    let named = |set: &[Command]| [set, &[damage(0, 0, 1, 1)]].concat();
    let gone = set.remove(2);
    f.layers.take_frame(&named(&set));
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!((p.writes.new, p.writes.full), (0, 0), "no glass took new rows");
    for (i, was) in settled.iter().enumerate().filter(|(i, _)| *i != 2) {
        let now = cache_of(&f, if i > 2 { i - 1 } else { i });
        let (Some(CacheStep::Read(a)), Some(CacheStep::Read(b) | CacheStep::Write(b))) =
            (*was, now)
        else {
            panic!("window {i}: {was:?} then {now:?}")
        };
        assert_eq!(a, b, "window {i} kept its rows");
    }
    // It opens again, on top: it takes the rows it left (the lowest free ones).
    set.push(gone);
    f.layers.take_frame(&named(&set));
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!((p.writes.new, f.glass.short()), (1, 0));
    assert_eq!(cache_of(&f, 11), settled[2].map(|_| CacheStep::Write(2 * 216)));
}

#[test]
fn the_atlas_uploads_once_per_generation() {
    let mut f = Frame::new(2048);
    let stamped = |epoch| with(layer(0, 0, 100, 100), |l| l.content_epoch = epoch);
    f.layers.take_frame(&[stamped(3)]);
    assert_eq!(
        f.plan(DISPLAY, &every_blur),
        Planned {
            layers: 1,
            writes: Default::default(),
            by: Default::default(),
            upload: true,
            damage: Area::Full,
            holds: Default::default()
        }
    );
    // A pointer move: nothing new to upload.
    assert!(!f.plan(DISPLAY, &every_blur).upload);
    // A new set of the same generation (a window dragged): still nothing.
    f.layers.take_frame(&[stamped(3)]);
    assert!(!f.plan(DISPLAY, &every_blur).upload);
    // New content.
    f.layers.take_frame(&[stamped(4)]);
    assert!(f.plan(DISPLAY, &every_blur).upload);
    // An abandoned batch lost the upload: the next frame uploads again.
    f.layers.invalidate_upload();
    assert!(f.plan(DISPLAY, &every_blur).upload);
    // An unstamped set uploads whenever it is replaced.
    f.layers.take_frame(&[layer(0, 0, 100, 100)]);
    assert!(f.plan(DISPLAY, &every_blur).upload);
    assert!(!f.plan(DISPLAY, &every_blur).upload);
}

// The cache's own rules, moved with it from the QEMU path's backend.

fn ready_cache() -> GlassCache {
    let mut c = GlassCache::new(2048);
    c.ready = true;
    c
}

#[test]
fn the_same_key_reads_on_the_second_frame() {
    assert_eq!(GlassCache::new(2048).wallpaper_epoch, 0);
    let mut c = ready_cache();
    assert_eq!(c.step(0, 42, 500, false), CacheStep::Write(0));
    assert_eq!(c.step(0, 42, 500, false), CacheStep::Read(0));
}

#[test]
fn a_changed_backdrop_writes_again() {
    let mut c = ready_cache();
    assert_eq!(c.step(0, 42, 500, false), CacheStep::Write(0));
    assert_eq!(c.step(0, 43, 500, false), CacheStep::Write(0));
    assert_eq!(c.step(0, 43, 500, false), CacheStep::Read(0));
}

#[test]
fn a_slot_keeps_its_rows_and_a_new_height_takes_the_lowest_free_ones() {
    let mut c = ready_cache();
    assert_eq!(c.step(0, 1, 300, false), CacheStep::Write(0));
    assert_eq!(c.step(1, 7, 300, false), CacheStep::Write(300));
    // The glass below vanished (its slot is carried by nobody): the other keeps its rows.
    c.carry(&[Some(1)]);
    assert_eq!(c.step(0, 7, 300, false), CacheStep::Read(300));
    // A glass of another height: new rows, the lowest that touch no held ones.
    assert_eq!(c.step(0, 7, 400, false), CacheStep::Write(0));
    assert_eq!(c.step(1, 9, 200, false), CacheStep::Write(400));
}

#[test]
fn test_reject_rows_across_a_plane_edge_and_a_glass_taller_than_a_plane() {
    let mut c = ready_cache();
    c.add_plane();
    assert_eq!(c.step(0, 1, 1500, false), CacheStep::Write(0));
    // 548 rows are free in the first plane: 600 start at the second, never across the edge.
    assert_eq!(c.step(1, 2, 600, false), CacheStep::Write(2048));
    assert_eq!(c.step(2, 3, 500, false), CacheStep::Write(1500));
    // Taller than a plane: off whatever the planes, and never asked for.
    assert_eq!(c.step(3, 4, 2049, false), CacheStep::Off);
    assert_eq!(c.short(), 0);
}

#[test]
fn an_overflow_or_no_cache_is_off_and_keeps_no_stale_slot() {
    let mut c = ready_cache();
    assert_eq!(c.step(0, 1, 1600, false), CacheStep::Write(0));
    assert_eq!(c.step(1, 2, 500, false), CacheStep::Off);
    assert_eq!(c.short(), 500);
    assert_eq!(c.keep(1, 500), None, "nothing held");
    let mut cold = GlassCache::new(2048);
    assert_eq!(cold.step(0, 1, 100, false), CacheStep::Off);
}

#[test]
fn the_fold_separates_field_order() {
    let at = |src_row_abs, src_x| FrameLayer {
        src_row_abs,
        src_x,
        width: 3,
        height: 4,
        dst_x: 5,
        dst_y: 6,
        opacity: 255,
        ..FrameLayer::default()
    };
    // (row 1, x 2) against (row 2, x 1) must not collide (a plain XOR would).
    assert_ne!(
        fold_layer(seed(1280, 800, 0), &at(1, 2)),
        fold_layer(seed(1280, 800, 0), &at(2, 1))
    );
    // A wallpaper uploaded again changes every key.
    assert_ne!(seed(1280, 800, 0), seed(1280, 800, 1));
}
