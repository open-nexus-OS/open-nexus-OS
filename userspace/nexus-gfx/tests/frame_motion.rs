// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner and motion (TASK-0336, TASK-0338): why each glass is written (the cause line), a
//! glass of a window in motion keeps its blur and writes once at rest, a present carries the
//! window's transform with its layers, and a present owed for what moved alone (it names no
//! damage) redraws only that.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, FrameLayer, Transform};

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
        content_w: 0,
        content_h: 0,
        scroll_band_top_abs: 0,
        scroll_band_h: 0,
        layer_id: 0,
        content_epoch: 0,
        flags: 0,
        key: 0,
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
        content_w: l.content_w,
        content_h: l.content_h,
        scroll_band_top_abs: l.scroll_band_top_abs,
        scroll_band_h: l.scroll_band_h,
        layer_id: l.layer_id,
        content_epoch: l.content_epoch,
        flags: (if l.opaque { nexus_gfx::command::buffer::LAYER_OPAQUE } else { 0 })
            | (if l.solid { nexus_gfx::command::buffer::LAYER_SOLID } else { 0 }),
        key: l.key,
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

fn damage(x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin: 0 }
}

fn damage_from(origin: u32, x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin }
}

/// The damage a present names when it is owed for what moved alone (TASK-0338).
fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn transform(layer_id: u32, dx: i32, dy: i32, opacity: u32, scale_pct: u32) -> Command {
    Command::LayerTransform { layer_id, dx, dy, opacity, scale_pct }
}

#[test]
fn every_glass_write_names_its_cause() {
    use nexus_gfx::frame::Writes;
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let window = layer(200, 200, 600, 400);
    let pane = with(glass(layer(300, 300, 400, 200), 40), |l| l.layer_id = 2);
    // The first frame is a present without named damage: damage everywhere, named full.
    f.layers.take_frame(&[base.clone(), window.clone(), pane.clone()]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { full: 1, ..Default::default() });
    // A change below within its reach, the same backdrop in the same rows: the band.
    f.layers.take_frame(&[
        base.clone(),
        window.clone(),
        pane.clone(),
        damage_from(0, 350, 350, 20, 10),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { band: 1, ..Default::default() });
    // The window below moved: the backdrop.
    f.layers.take_frame(&[
        base.clone(),
        layer(210, 200, 600, 400),
        pane.clone(),
        damage(0, 0, 1, 1),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { backdrop: 1, ..Default::default() });
    // A second glass packed before it: new rows for the newcomer, and the pane's key folds it
    // (a glass below is a backdrop like any layer).
    let other = with(glass(layer(100, 100, 100, 100), 40), |l| l.layer_id = 3);
    f.layers.take_frame(&[
        base.clone(),
        layer(210, 200, 600, 400),
        other.clone(),
        pane.clone(),
        damage(0, 0, 1, 1),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { new: 1, backdrop: 1, ..Default::default() });
    // A full present: everything, named full.
    f.layers.take_frame(&[base, layer(210, 200, 600, 400), other, pane]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { full: 2, ..Default::default() });
    assert_eq!(p.writes.total(), 2);
}

#[test]
fn a_glass_of_a_window_in_motion_keeps_its_blur_and_writes_once_at_rest() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let body = with(layer(200, 200, 600, 400), |l| l.layer_id = 2);
    let chrome = with(glass(layer(200, 200, 600, 60), 40), |l| l.layer_id = 2);
    f.layers.take_frame(&[base.clone(), body.clone(), chrome.clone()]);
    f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 2), Some(CacheStep::Write(_))), "the first blur");
    // The window moves (a drag as a transform override): its glass keeps its rows.
    assert!(f.layers.set_transform(2, Transform { dx: 40, dy: 10, opacity: 255, scale_pct: 100 }));
    f.layers.take_frame(&[base.clone(), body.clone(), chrome.clone(), damage(0, 0, 1, 1)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)), "in motion: the last blur, kept");
    assert_eq!(p.writes.total(), 0);
    // Another glass elsewhere, touched only by the moving window's damage, keeps too.
    let dock = with(glass(layer(100, 900, 400, 100), 40), |l| l.layer_id = 3);
    f.layers.take_frame(&[base.clone(), body.clone(), chrome.clone(), dock.clone()]);
    f.plan(DISPLAY, &every_blur);
    f.layers.take_frame(&[
        base.clone(),
        body.clone(),
        chrome.clone(),
        dock.clone(),
        damage_from(2, 120, 920, 20, 10),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 3), Some(CacheStep::Read(_))), "touched by motion only: kept");
    assert_eq!(p.writes.total(), 0);
    // At rest (the identity override): one write for the changed backdrop.
    assert!(f.layers.set_transform(2, Transform { dx: 0, dy: 0, opacity: 255, scale_pct: 100 }));
    f.layers.take_frame(&[base, body, chrome, dock, damage(0, 0, 1, 1)]);
    let p = f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 2), Some(CacheStep::Write(_))), "at rest: blurred once");
    assert!(p.writes.total() >= 1);
}

#[test]
fn a_present_carries_the_window_transform_with_its_layers() {
    let mut f = ready_frame(2048);
    let window = with(layer(200, 200, 600, 400), |l| l.layer_id = 2);
    // A drag step is a present naming no damage: the window's frame stays, its transform moves
    // it (TASK-0338) ...
    f.layers.take_frame(&[window.clone()]);
    f.plan(DISPLAY, &every_blur);
    f.layers.take_frame(&[transform(2, 40, 10, 255, 100), window.clone(), no_damage()]);
    assert_eq!(f.layers.effective(0).map(|l| (l.dst_x, l.dst_y)), Some((240, 210)));
    // ... and the release's present brings the window at its new place WITH the identity, so
    // no plan ever sees the new frame moved again or the old one snapped back.
    let moved = with(layer(240, 210, 600, 400), |l| l.layer_id = 2);
    f.layers.take_frame(&[
        Command::LayerTransform { layer_id: 2, dx: 0, dy: 0, opacity: 255, scale_pct: 100 },
        moved,
    ]);
    assert_eq!(f.layers.effective(0).map(|l| (l.dst_x, l.dst_y)), Some((240, 210)));
    assert!(!f.layers.in_motion(2), "the identity: at rest");
    // A transform for a window beyond the table is ignored, the rest of the present taken.
    let taken = f.layers.take_frame(&[
        Command::LayerTransform { layer_id: 99, dx: 5, dy: 5, opacity: 255, scale_pct: 100 },
        with(layer(240, 210, 600, 400), |l| l.layer_id = 2),
    ]);
    assert!(taken.replaced);
}

/// TASK-0338: a frame owed for a move alone — the present carries the layers as they are, the
/// window's transform, and names no damage: the plan redraws where the window was and where it
/// is, never the screen; owed for nothing (the frame clock), it redraws nothing.
#[test]
fn a_present_that_names_no_damage_redraws_only_what_moved() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let window = with(layer(200, 200, 600, 400), |l| l.layer_id = 2);
    f.layers.take_frame(&[base.clone(), window.clone()]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::Full, "layers and no damage: all");
    f.layers.take_frame(&[
        transform(2, 40, 10, 255, 100),
        base.clone(),
        window.clone(),
        no_damage(),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.damage, Area::rect(200, 200, 640, 410), "where it was and where it is");
    f.layers.take_frame(&[transform(2, 40, 10, 255, 100), base, window, no_damage()]);
    assert_eq!(f.plan(DISPLAY, &every_blur).damage, Area::None, "nothing moved: nothing drawn");
}

/// TASK-0338: a scroll is a present — the body comes with its new source row and the present
/// names no damage: the plan redraws the body alone, and the window's glass below it keeps its
/// blur (the body is the window's own content, above the glass).
#[test]
fn a_scrolled_body_is_redrawn_alone_and_its_glass_keeps_its_blur() {
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let pane = with(glass(layer(200, 200, 600, 400), 40), |l| l.layer_id = 2);
    let body = |row| {
        with(layer(200, 260, 600, 340), |l| {
            l.layer_id = 2;
            l.src_row_abs = row;
            l.scroll_band_top_abs = 5000;
            l.scroll_band_h = 2000;
        })
    };
    f.layers.take_frame(&[base.clone(), pane.clone(), body(5400)]);
    f.plan(DISPLAY, &every_blur);
    f.layers.take_frame(&[base, pane, body(5480), no_damage()]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.damage, Area::rect(200, 260, 600, 340));
    assert_eq!(p.writes.total(), 0, "no glass blurred again");
    assert!(matches!(cache_of(&f, 1), Some(CacheStep::Read(_))));
}
