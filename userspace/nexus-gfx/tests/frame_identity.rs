// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner and identity (RFC-0109, TASK-0341): a layer is matched to its last frame's by its
//! key, never by its place in the list — a layer that appears or goes leaves every other layer
//! clean — and a glass is keyed by what lies below it within its reach, so a scroll layer's slice
//! moving elsewhere blurs no glass again.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Area, CacheStep, Frame, FrameLayer};

const DISPLAY: (u32, u32) = (1920, 1080);

fn layer(key: u32, x: u32, y: u32, w: u32, h: u32) -> FrameLayer {
    FrameLayer {
        src_row_abs: 5400,
        width: w,
        height: h,
        dst_x: x,
        dst_y: y,
        opacity: 255,
        key,
        ..FrameLayer::default()
    }
}

fn glass(mut l: FrameLayer, blur: u32) -> FrameLayer {
    l.backdrop_blur = blur;
    l
}

fn cmd(l: FrameLayer) -> Command {
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
        flags: 0,
        key: l.key,
    }
}

/// The damage a present names when it is owed for what moved alone (TASK-0338).
fn no_damage() -> Command {
    Command::Damage { rect: TileRect::NONE, origin: 0 }
}

fn ready_frame() -> Frame {
    let mut f = Frame::new(2048);
    f.glass.ready = true;
    f
}

fn every_blur(_: u32) -> bool {
    true
}

fn present(f: &mut Frame, layers: &[FrameLayer], extra: &[Command]) -> nexus_gfx::frame::Planned {
    let mut cmds: Vec<Command> = layers.iter().copied().map(cmd).collect();
    cmds.extend_from_slice(extra);
    f.layers.take_frame(&cmds);
    f.plan(DISPLAY, &every_blur)
}

fn cache_of(f: &Frame, i: usize) -> Option<CacheStep> {
    f.step(i).and_then(|s| s.glass).map(|g| g.cache)
}

/// A wash appearing before a window in the list (a hover) changes where it is drawn and nothing
/// else: the window's glass keeps its rows, nothing below it is redrawn.
#[test]
fn a_layer_inserted_mid_list_leaves_every_later_layer_clean() {
    let mut f = ready_frame();
    let base = layer(1, 0, 0, 1920, 1080);
    let window = glass(layer(2, 200, 200, 600, 400), 40);
    let wash = layer(9, 1500, 900, 100, 40);
    present(&mut f, &[base, window], &[]);
    let p = present(&mut f, &[base, wash, window], &[no_damage()]);
    assert_eq!(p.damage, Area::rect(1500, 900, 100, 40), "where the wash is, alone");
    assert_eq!(p.writes.total(), 0, "the window's glass keeps its blur");
    assert!(matches!(cache_of(&f, 2), Some(CacheStep::Read(_))));
    // …and going again damages where it was, alone.
    let p = present(&mut f, &[base, window], &[no_damage()]);
    assert_eq!(p.damage, Area::rect(1500, 900, 100, 40));
    assert_eq!(p.writes.total(), 0);
}

/// A scroll layer's slice moving in one window re-blurs no glass of another window above it:
/// the glass's backdrop is what lies under it.
#[test]
fn a_slice_moving_elsewhere_blurs_no_glass_again() {
    let mut f = ready_frame();
    let base = layer(1, 0, 0, 1920, 1080);
    let window_a = layer(2, 100, 100, 500, 400);
    let slice = |row| FrameLayer { src_row_abs: row, ..layer(3, 120, 150, 460, 300) };
    let window_b = glass(layer(4, 1000, 100, 600, 400), 40);
    present(&mut f, &[base, window_a, slice(7000), window_b], &[]);
    let p = present(&mut f, &[base, window_a, slice(7080), window_b], &[no_damage()]);
    assert_eq!(p.damage, Area::rect(120, 150, 460, 300), "the slice alone");
    assert_eq!(p.writes.total(), 0, "window B's glass is not under the slice");
    assert!(matches!(cache_of(&f, 3), Some(CacheStep::Read(_))));
}

/// …while a slice moving under a glass does change what it blurs.
#[test]
fn a_slice_moving_under_a_glass_blurs_it_again() {
    let mut f = ready_frame();
    let base = layer(1, 0, 0, 1920, 1080);
    let slice = |row| FrameLayer { src_row_abs: row, ..layer(3, 120, 150, 460, 300) };
    let menu = glass(layer(4, 300, 200, 200, 120), 40);
    present(&mut f, &[base, slice(7000), menu], &[]);
    let p = present(&mut f, &[base, slice(7080), menu], &[no_damage()]);
    assert_eq!(p.writes.backdrop, 1, "the menu's backdrop moved");
}

/// A layer that goes is damaged where it was; the layers after it, now one place earlier in the
/// list, are the same layers and stay clean.
#[test]
fn a_layer_that_goes_damages_where_it_was_and_nothing_else() {
    let mut f = ready_frame();
    let base = layer(1, 0, 0, 1920, 1080);
    let a = layer(2, 100, 100, 50, 50);
    let b = layer(3, 300, 300, 50, 50);
    present(&mut f, &[base, a, b], &[]);
    let p = present(&mut f, &[base, b], &[no_damage()]);
    assert_eq!(p.damage, Area::rect(100, 100, 50, 50));
}

/// Unnamed layers (key 0) still match by order: the identity is the key and its occurrence.
#[test]
fn unnamed_layers_match_by_their_order() {
    let mut f = ready_frame();
    let a = layer(0, 100, 100, 50, 50);
    let b = layer(0, 300, 300, 50, 50);
    present(&mut f, &[a, b], &[]);
    let p = present(&mut f, &[a, b], &[no_damage()]);
    assert_eq!(p.damage, Area::None, "the same two layers in the same order");
    let p = present(&mut f, &[b, a], &[no_damage()]);
    assert_eq!(p.damage, Area::rect(100, 100, 250, 250), "swapped: both changed");
}
