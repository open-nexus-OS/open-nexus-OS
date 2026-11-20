// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner and motion (TASK-0336, TASK-0338): why each glass is written (the cause line), a
//! glass of a window in motion keeps its blur and writes once at rest, a present carries the
//! window's transform with its layers, and a present owed for what moved alone (it names no
//! damage) redraws only that. The hold (TASK-0351 P6, `frame::hold`): rows of the rest
//! rectangle, what a held glass owes, the hold's two bounds.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, FrameLayer, Glass, Planned};

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
        source: 0,
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
        source: l.source,
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
    Command::LayerTransform {
        layer_id,
        dx,
        dy,
        opacity,
        scale: scale_pct * 100,
        origin_x: 0,
        origin_y: 0,
    }
}

#[test]
fn every_glass_write_names_its_cause() {
    use nexus_gfx::frame::Writes;
    let mut f = ready_frame(2048);
    let base = layer(0, 0, 1920, 1080);
    let window = layer(200, 200, 600, 400);
    // Keyed as windowd keys its layers: a glass is followed by its key, not its place in the set.
    let pane = with(glass(layer(300, 300, 400, 200), 40), |l| (l.layer_id, l.key) = (2, 0x2_0100));
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
    assert_eq!(p.writes, Writes { band: 1, band_base: 1, ..Default::default() });
    // The window below moved: the backdrop.
    f.layers.take_frame(&[
        base.clone(),
        layer(210, 200, 600, 400),
        pane.clone(),
        damage(0, 0, 1, 1),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { backdrop: 1, backdrop_band: 1, ..Default::default() });
    // A second glass below it, within its reach: new rows for the newcomer, and the pane's key
    // folds it (a glass below is a backdrop like any layer) — the pane keeps its rows.
    let other = with(glass(layer(250, 250, 100, 100), 40), |l| (l.layer_id, l.key) = (3, 0x3_0100));
    f.layers.take_frame(&[
        base.clone(),
        layer(210, 200, 600, 400),
        other.clone(),
        pane.clone(),
        damage(0, 0, 1, 1),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(p.writes, Writes { new: 1, backdrop: 1, backdrop_band: 1, ..Default::default() });
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
    const BODY: u32 = 0x2_0000;
    let body = with(layer(200, 200, 600, 400), |l| (l.layer_id, l.key) = (2, BODY));
    let chrome = with(glass(layer(200, 200, 600, 60), 40), |l| l.layer_id = 2);
    f.layers.take_frame(&[base.clone(), body.clone(), chrome.clone()]);
    f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 2), Some(CacheStep::Write(_))), "the first blur");
    // The window moves (a drag as its transform, carried by every present): its glass keeps
    // its rows.
    let moving = transform(2, 40, 10, 255, 100);
    f.layers.take_frame(&[
        moving.clone(),
        base.clone(),
        body.clone(),
        chrome.clone(),
        damage(0, 0, 1, 1),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)), "in motion: the last blur, kept");
    assert_eq!(p.writes.total(), 0);
    // Another glass, at rest over the moving window (its body lies within the reach), keeps
    // too — through a change the window names meanwhile.
    let dock = with(glass(layer(100, 620, 400, 100), 40), |l| l.layer_id = 3);
    f.layers.take_frame(&[
        moving.clone(),
        base.clone(),
        body.clone(),
        chrome.clone(),
        dock.clone(),
    ]);
    f.plan(DISPLAY, &every_blur);
    f.layers.take_frame(&[
        moving,
        base.clone(),
        body.clone(),
        chrome.clone(),
        dock.clone(),
        damage_from(BODY, 300, 580, 20, 10),
    ]);
    let p = f.plan(DISPLAY, &every_blur);
    assert!(matches!(cache_of(&f, 3), Some(CacheStep::Read(_))), "over a mover: kept");
    assert_eq!(p.writes.total(), 0);
    // At rest (the identity): one write for the changed backdrop.
    let rest = transform(2, 0, 0, 255, 100);
    f.layers.take_frame(&[rest, base, body, chrome, dock, damage(0, 0, 1, 1)]);
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
    f.layers.take_frame(&[transform(2, 0, 0, 255, 100), moved]);
    assert_eq!(f.layers.effective(0).map(|l| (l.dst_x, l.dst_y)), Some((240, 210)));
    assert!(!f.layers.in_motion(2), "the identity: at rest");
    // A transform for any window key is taken with the rest of the present (RFC-0114 §7: a key,
    // never a table index).
    let taken = f.layers.take_frame(&[
        transform(99, 5, 5, 255, 100),
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

// The hold (TASK-0351 P6, RFC-0109; `build/agents/arch-transition-every-frame-2026-11-19.md`,
// Decision 2: T6, T8, T9, T10): a glass's rows are of its REST rectangle, and one bounded rule
// keeps them through a motion.

const BASE: u32 = 0xFFFF_0000;
/// The plans a glass is held in a row (`frame::hold::HOLD_MAX`).
const HOLD_MAX: u32 = 64;

fn base() -> Command {
    with(layer(0, 0, 1920, 1080), |l| (l.opaque, l.key) = (true, BASE))
}

fn present(f: &mut Frame, cmds: &[Command]) -> Planned {
    f.layers.take_frame(cmds);
    f.plan(DISPLAY, &every_blur)
}

fn glass_of(f: &Frame, i: usize) -> Glass {
    f.step(i).and_then(|s| s.glass).expect("a glass step")
}

/// Window `id` scaled to `scale` ten-thousandths about `origin`.
fn scaled(id: u32, scale: u32, origin: (i32, i32)) -> Command {
    Command::LayerTransform {
        layer_id: id,
        dx: 0,
        dy: 0,
        opacity: 255,
        scale,
        origin_x: origin.0,
        origin_y: origin.1,
    }
}

fn at_rest(id: u32) -> Command {
    transform(id, 0, 0, 255, 100)
}

/// A panel scaling in from 96 % over seventeen plans, then at rest: `scene` before the panel's
/// glass (step `scene.len()`), `during` named on the ninth plan. The writes and reads of the
/// glass, and the frame as the rest plan left it.
fn scale_in(scene: &[Command], during: Option<Command>) -> (u32, u32, Frame, Planned) {
    let panel = with(glass(layer(1500, 40, 352, 566), 40), |l| (l.layer_id, l.key) = (9, 9 << 16));
    let (mut f, at, mut counts) = (ready_frame(4096), scene.len(), (0, 0));
    let mut tally = |f: &Frame, p: Planned| {
        counts.0 += p.writes.total();
        counts.1 += u32::from(matches!(glass_of(f, at).cache, CacheStep::Read(_)));
    };
    for n in 0..17 {
        let mut cmds = vec![scaled(9, 9_600 + 24 * n, (1676, 40))];
        if n == 0 {
            cmds.extend(scene.iter().cloned().chain([panel.clone()]));
        }
        cmds.push(during.clone().filter(|_| n == 8).unwrap_or_else(no_damage));
        let p = present(&mut f, &cmds);
        // Whatever size it is drawn at, its rows are the rest rectangle's.
        let (g, drawn) = (glass_of(&f, at), f.step(at).expect("the panel").layer);
        assert_eq!((g.at, g.rows), ((1500, 40, 352, 566), 144), "plan {n}");
        assert!(drawn.width < 352 && drawn.height < 566, "plan {n}: drawn scaled");
        assert_eq!(matches!(g.cache, CacheStep::Write(_)), n == 0, "plan {n}");
        tally(&f, p);
    }
    let p = present(&mut f, &[at_rest(9), no_damage()]);
    tally(&f, p);
    (counts.0, counts.1, f, p)
}

#[test]
fn a_glass_scaling_in_is_written_once_at_its_rest_rectangle() {
    // T6: nothing else changes — the first frame's rows are the rows it rests with.
    let (writes, reads, f, _) = scale_in(&[base()], None);
    assert_eq!((writes, reads), (1, 17));
    assert_eq!(f.step(1).map(|s| s.layer.width), Some(352), "at rest: drawn as it rests");
    // A change below it during the motion is owed and paid at rest: a band, not the glass.
    let change = Area::rect(1600, 300, 20, 10);
    let (writes, reads, f, p) = scale_in(&[base()], Some(damage_from(BASE, 1600, 300, 20, 10)));
    assert_eq!((writes, reads), (2, 16));
    assert_eq!((glass_of(&f, 1).band, p.writes.band, p.by.rest), (change, 1, 1));
}

#[test]
fn a_glass_over_a_layer_of_its_own_window_is_blurred_again_at_rest() {
    // The window's own body below its glass was blurred as the first frame drew it — scaled:
    // the rows are not the rest state's, and the one write at rest is where the body was drawn
    // through the hold (C-r) — its rest rectangle, the glass's own: all of the glass.
    let body = with(layer(1500, 40, 352, 566), |l| (l.layer_id, l.key) = (9, 0x9_0001));
    let (writes, reads, f, p) = scale_in(&[base(), body], None);
    assert_eq!((writes, reads), (2, 16));
    let all = Area::rect(1500, 40, 352, 566);
    assert_eq!((glass_of(&f, 2).band, p.writes.backdrop, p.by.rest), (all, 1, 1));
    assert_eq!((p.writes.backdrop_band, p.by.misses.band), (1, 0), "as wide as the glass");
}

#[test]
fn test_reject_losing_a_change_made_during_a_motion() {
    // T8: a window fades out and back; what changed below its glass meanwhile is in its rows
    // at rest, though the glass rests exactly as it was blurred.
    let pane = with(glass(layer(300, 300, 400, 200), 40), |l| l.layer_id = 2);
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), pane]);
    let p = present(&mut f, &[transform(2, 0, 0, 128, 100), damage_from(BASE, 350, 350, 20, 10)]);
    assert_eq!((p.writes.total(), cache_of(&f, 1)), (0, Some(CacheStep::Read(0))), "held");
    let p = present(&mut f, &[transform(2, 0, 0, 0, 100), damage_from(BASE, 600, 480, 30, 10)]);
    assert_eq!(p.writes.total(), 0);
    let p = present(&mut f, &[at_rest(2), no_damage()]);
    assert_eq!((p.writes.band, p.by.rest), (1, 1), "at rest: what it owes");
    let g = glass_of(&f, 1);
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), Area::rect(350, 350, 280, 140)));
    // Paid: the next plan reads.
    assert_eq!(present(&mut f, &[at_rest(2), no_damage()]).writes.total(), 0);
}

const WINDOW: u32 = 0x1_0000;

/// The base, a window (transform id 1) at `x` and a glass pane of another window over it.
fn pane_over_a_window(x: u32) -> [Command; 3] {
    let window = with(layer(x, 200, 600, 400), |l| (l.layer_id, l.key) = (1, WINDOW));
    [base(), window, with(glass(layer(300, 300, 400, 200), 40), |l| l.layer_id = 2)]
}

#[test]
fn a_glass_at_rest_over_a_moving_layer_is_blurred_when_the_layer_rests() {
    // T9: the window below moves 16 pixels by its transform — less than the blur hides.
    let mut f = ready_frame(4096);
    present(&mut f, &pane_over_a_window(200));
    for dx in 1..=16 {
        let p = present(&mut f, &[transform(1, dx, 0, 255, 100), no_damage()]);
        assert_eq!((p.writes.total(), cache_of(&f, 2)), (0, Some(CacheStep::Read(0))), "{dx}");
    }
    // It rests where it went: one write.
    let mut dropped = vec![at_rest(1)];
    dropped.extend(pane_over_a_window(216));
    dropped.push(no_damage());
    let p = present(&mut f, &dropped);
    assert_eq!((p.writes.total(), p.writes.backdrop, p.by.rest), (1, 1, 1));
    // A drag of 400 pixels re-blurs it every blur radius of travel: not every frame, not
    // never.
    let mut writes = 0;
    for step in 1..=100 {
        writes +=
            present(&mut f, &[transform(1, 4 * step, 0, 255, 100), no_damage()]).writes.total();
    }
    let mut dropped = vec![at_rest(1)];
    dropped.extend(pane_over_a_window(616));
    dropped.push(no_damage());
    writes += present(&mut f, &dropped).writes.total();
    assert!((9..=11).contains(&writes), "{writes} writes over a 400 px drag at radius 40");
}

#[test]
fn a_change_made_while_the_layers_below_are_displaced_owes_the_whole_glass() {
    // The window below moves away and comes back to where it was: nothing to blur again.
    let mut f = ready_frame(4096);
    present(&mut f, &pane_over_a_window(200));
    present(&mut f, &[transform(1, 30, 0, 255, 100), no_damage()]);
    let p = present(&mut f, &[at_rest(1), no_damage()]);
    assert_eq!((p.writes.total(), cache_of(&f, 2)), (0, Some(CacheStep::Read(0))));
    // It changes its content while displaced: the change is named where the window is drawn
    // then, not where it shows at rest — no band can hold it, the glass owes all of itself.
    present(&mut f, &[transform(1, 30, 0, 255, 100), damage_from(WINDOW, 380, 350, 20, 10)]);
    assert_eq!(cache_of(&f, 2), Some(CacheStep::Read(0)), "held");
    let p = present(&mut f, &[at_rest(1), no_damage()]);
    assert_eq!((p.writes.total(), p.by.rest), (1, 1));
    assert_eq!(glass_of(&f, 2).band, Area::Full);
}

#[test]
fn test_reject_holding_a_glass_past_the_bound() {
    // T10: a client parks its window's transform. A change below its glass is owed while the
    // glass is held, and paid after `HOLD_MAX` plans without a write — not at a rest that
    // never comes.
    let pane = with(glass(layer(300, 300, 400, 200), 40), |l| l.layer_id = 2);
    let mut f = ready_frame(4096);
    present(&mut f, &[base(), pane]);
    let parked = transform(2, 40, 10, 255, 100);
    for n in 1..=HOLD_MAX {
        let change = if n == 1 { damage_from(BASE, 350, 350, 20, 10) } else { no_damage() };
        let p = present(&mut f, &[parked.clone(), change]);
        assert_eq!((p.writes.total(), cache_of(&f, 1)), (0, Some(CacheStep::Read(0))), "{n}");
    }
    let p = present(&mut f, &[parked.clone(), no_damage()]);
    let (g, drawn) = (glass_of(&f, 1), f.step(1).expect("the pane").layer);
    assert_eq!((g.cache, g.band), (CacheStep::Write(0), Area::rect(350, 350, 20, 10)));
    // Paid at its rest rectangle, shown where it is drawn.
    assert_eq!((g.at, (drawn.dst_x, drawn.dst_y)), ((300, 300, 400, 200), (340, 310)));
    assert_eq!((p.writes.band, p.damage), (1, Area::rect(340, 310, 400, 200)));
    // The hold starts again.
    assert_eq!(present(&mut f, &[parked, no_damage()]).writes.total(), 0);
}

#[test]
fn without_a_transform_a_glass_rests_where_it_is_drawn() {
    let mut f = ready_frame(4096);
    present(&mut f, &pane_over_a_window(200));
    let (g, drawn) = (glass_of(&f, 2), f.step(2).expect("the pane").layer);
    assert_eq!(g.at, (drawn.dst_x, drawn.dst_y, drawn.width, drawn.height));
}
