// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The frame planner (TASK-0215 5c-3 P4): the retained set, its overrides, the glass cache and
//! the atlas upload as every executor sees them.

use nexus_gfx::command::buffer::{Command, RgbaColor};
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::glass::{fold_layer, seed};
use nexus_gfx::frame::{
    Area, CacheStep, Frame, FrameLayer, GlassCache, History, Planned, RetainedLayers, Transform,
    MAX_LAYERS,
};

const DISPLAY: (u32, u32) = (1920, 1080);

fn layer(x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::CompositeLayer {
        src_row_abs: 5400,
        src_x: 0,
        width: w,
        height: h,
        dst_x: x,
        dst_y: y,
        opacity: 255,
        corner_radius: 0,
        shadow_blur: 0,
        shadow_offset_y: 0,
        shadow_alpha: 0,
        backdrop_blur: 0,
        scroll_id: 0,
        content_w: 0,
        content_h: 0,
        scroll_band_top_abs: 0,
        scroll_band_h: 0,
        layer_id: 0,
        content_epoch: 0,
    }
}

fn with(cmd: Command, f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer::from_command(&cmd).expect("a layer");
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
        scroll_id: l.scroll_id,
        content_w: l.content_w,
        content_h: l.content_h,
        scroll_band_top_abs: l.scroll_band_top_abs,
        scroll_band_h: l.scroll_band_h,
        layer_id: l.layer_id,
        content_epoch: l.content_epoch,
    }
}

fn glass(cmd: Command, blur: u32) -> Command {
    with(cmd, |l| l.backdrop_blur = blur)
}

fn ready_frame(rows: u32) -> Frame {
    let mut f = Frame::new(rows);
    f.glass.ready = true;
    f
}

fn every_blur(_: u32) -> bool {
    true
}

fn cache_of(f: &Frame, i: usize) -> Option<CacheStep> {
    f.step(i).and_then(|s| s.glass).map(|g| g.cache)
}

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
fn a_new_set_drops_the_scroll_overrides_and_keeps_the_transforms() {
    let mut set = RetainedLayers::new();
    let body = with(layer(100, 100, 400, 300), |l| {
        l.scroll_id = 1;
        l.layer_id = 1;
    });
    set.take_frame(&[body.clone()]);
    assert!(set.set_scroll(1, 7000));
    let fade = Transform { dx: 0, dy: 0, opacity: 128, scale_pct: 100 };
    assert!(set.set_transform(1, fade));
    assert_eq!(set.effective(0).map(|l| l.src_row_abs), Some(7000));
    // A new set carries the scroll offset itself; the transform is the animation's state.
    set.take_frame(&[body]);
    let l = set.effective(0).expect("a layer");
    assert_eq!(l.src_row_abs, 5400);
    assert_eq!(l.opacity, 128);
}

#[test]
fn the_transform_moves_fades_and_scales_about_the_centre() {
    let mut set = RetainedLayers::new();
    set.take_frame(&[with(layer(100, 100, 400, 300), |l| l.layer_id = 2)]);
    set.set_transform(2, Transform { dx: 10, dy: -20, opacity: 51, scale_pct: 50 });
    let l = set.effective(0).expect("a layer");
    // Moved by (10, -20), then half the size about the centre: 200x150 at (210, 155).
    assert_eq!((l.dst_x, l.dst_y, l.width, l.height), (210, 155, 200, 150));
    assert_eq!(l.opacity, 51);
    // The content band keeps its size: the executor spreads it over the scaled layer.
    assert_eq!((l.content_w, l.content_h), (400, 300));
    // A move past the screen's edge stops at it.
    set.set_transform(2, Transform { dx: -500, dy: 0, opacity: 255, scale_pct: 100 });
    assert_eq!(set.effective(0).map(|l| l.dst_x), Some(0));
}

#[test]
fn test_reject_overrides_for_id_zero_or_beyond_the_table() {
    let mut set = RetainedLayers::new();
    let t = Transform { dx: 1, dy: 1, opacity: 255, scale_pct: 100 };
    assert!(!set.set_scroll(0, 1));
    assert!(!set.set_scroll(9, 1));
    assert!(!set.set_transform(0, t));
    assert!(!set.set_transform(9, t));
    assert!(set.set_scroll(8, 1) && set.set_transform(8, t));
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
    f.layers.set_transform(1, Transform { dx: 5, dy: 0, opacity: 255, scale_pct: 100 });
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
fn rows_pack_in_walk_order_and_an_overflow_blurs_live() {
    // Two glass layers of 100 rows (27 quarter-resolution rows each at radius 20) in a cache of
    // 40 rows.
    let mut f = ready_frame(40);
    let a = glass(layer(0, 0, 100, 100), 20);
    let b = glass(layer(0, 200, 100, 100), 20);
    f.layers.take_frame(&[a.clone(), b]);
    f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 0), Some(CacheStep::Write(0)));
    assert_eq!(cache_of(&f, 1), Some(CacheStep::Off));
    assert_eq!(f.step(1).and_then(|s| s.glass).map(|g| g.rows), Some(27));
    // Without the executor's cache no glass is planned at all (nor blurred live, as before).
    let mut cold = Frame::new(2048);
    cold.layers.take_frame(&[a]);
    cold.plan(DISPLAY, &every_blur);
    assert_eq!(cold.step(0).and_then(|s| s.glass), None);
}

#[test]
fn the_atlas_uploads_once_per_generation() {
    let mut f = Frame::new(2048);
    let stamped = |epoch| with(layer(0, 0, 100, 100), |l| l.content_epoch = epoch);
    f.layers.take_frame(&[stamped(3)]);
    assert_eq!(
        f.plan(DISPLAY, &every_blur),
        Planned { layers: 1, upload: true, damage: Area::Full }
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
    assert_eq!(c.step(0, 42, 0, 500, false), CacheStep::Write(0));
    assert_eq!(c.step(0, 42, 0, 500, false), CacheStep::Read(0));
}

#[test]
fn a_changed_backdrop_writes_again() {
    let mut c = ready_cache();
    assert_eq!(c.step(0, 42, 0, 500, false), CacheStep::Write(0));
    assert_eq!(c.step(0, 43, 0, 500, false), CacheStep::Write(0));
    assert_eq!(c.step(0, 43, 0, 500, false), CacheStep::Read(0));
}

#[test]
fn test_reject_reading_rows_at_a_moved_packing_offset() {
    let mut c = ready_cache();
    assert_eq!(c.step(1, 7, 100, 300, false), CacheStep::Write(100));
    // A glass below vanished: the same key at another offset writes.
    assert_eq!(c.step(1, 7, 0, 300, false), CacheStep::Write(0));
}

#[test]
fn an_overflow_or_no_cache_blurs_live_and_keeps_no_stale_slot() {
    let mut c = ready_cache();
    assert_eq!(c.step(0, 1, 2048 - 10, 500, false), CacheStep::Off);
    assert_eq!(c.step(0, 1, 0, 500, false), CacheStep::Write(0));
    let mut cold = GlassCache::new(2048);
    assert_eq!(cold.step(0, 1, 0, 100, false), CacheStep::Off);
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

// What a frame changes on the screen (TASK-0215 5c-3 P5, RFC-0101 §8).

fn damage(x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin: 0 }
}

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
