// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Direct scanout (RFC-0101 §8 amended 2026-11-04, TASK-0351 P3): a frame whose topmost layer is
//! an opaque, untransformed, plain window body from a surface of its own, covering the display
//! whole, needs no compose — the planner names that layer; anything less composes.

use nexus_gfx::command::buffer::{Command, LAYER_OPAQUE, LAYER_SOLID};
use nexus_gfx::frame::{direct, FrameLayer, RetainedLayers, Transform};

const DISPLAY: (u32, u32) = (1920, 1080);

/// A change to a body.
type Tweak = Box<dyn Fn(&mut FrameLayer)>;

/// A body `w` × `h` at `(x, y)` from the atlas, changed by `f`, on the wire.
fn layer(x: u32, y: u32, w: u32, h: u32, f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer {
        src_row_abs: 5400,
        width: w,
        height: h,
        dst_x: x,
        dst_y: y,
        opacity: 255,
        ..FrameLayer::default()
    };
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
        flags: (if l.opaque { LAYER_OPAQUE } else { 0 }) | (if l.solid { LAYER_SOLID } else { 0 }),
        key: l.key,
        source: l.source,
    }
}

/// A fullscreen opaque body from surface 3, window transform 7, changed by `f`.
fn fullscreen(f: impl FnOnce(&mut FrameLayer)) -> Command {
    layer(0, 0, DISPLAY.0, DISPLAY.1, |l| {
        l.opaque = true;
        l.source = 3;
        l.slot = 1;
        l.layer_id = 7;
        f(l);
    })
}

fn set(cmds: &[Command]) -> RetainedLayers {
    let mut layers = RetainedLayers::default();
    layers.take_frame(cmds);
    layers
}

#[test]
fn a_fullscreen_opaque_surface_on_top_is_scanned_out_directly() {
    let wallpaper = layer(0, 0, DISPLAY.0, DISPLAY.1, |_| {});
    let below = layer(100, 100, 400, 300, |l| l.corner_radius = 16);
    assert_eq!(direct(&set(&[wallpaper, below, fullscreen(|_| {})]), DISPLAY), Some(2));
    assert_eq!(direct(&set(&[fullscreen(|_| {})]), DISPLAY), Some(0));
}

#[test]
fn test_reject_anything_above_or_a_frame_without_layers() {
    let badge = layer(1800, 20, 80, 80, |l| l.opacity = 200);
    assert_eq!(direct(&set(&[fullscreen(|_| {}), badge]), DISPLAY), None);
    assert_eq!(direct(&set(&[]), DISPLAY), None);
}

#[test]
fn test_reject_a_body_that_is_not_plain_or_not_whole() {
    let cases: [(&str, Tweak); 9] = [
        ("translucent", Box::new(|l| l.opacity = 254)),
        ("not opaque", Box::new(|l| l.opaque = false)),
        ("rounded", Box::new(|l| l.corner_radius = 1)),
        ("shadowed", Box::new(|l| (l.shadow_blur, l.shadow_alpha) = (8, 90))),
        ("glass", Box::new(|l| l.backdrop_blur = 20)),
        ("a scroll slice", Box::new(|l| l.scroll_band_h = 64)),
        ("from the layout", Box::new(|l| l.source = 0)),
        ("short of the display", Box::new(|l| l.height = DISPLAY.1 - 1)),
        ("offset", Box::new(|l| l.dst_x = 1)),
    ];
    for (why, f) in cases {
        assert_eq!(direct(&set(&[fullscreen(f)]), DISPLAY), None, "{why} must compose");
    }
}

#[test]
fn test_reject_a_body_in_motion_and_accept_it_at_rest() {
    let mut layers = set(&[fullscreen(|_| {})]);
    assert!(layers.set_transform(7, Transform { dx: 4, ..Transform::IDENTITY }));
    assert_eq!(direct(&layers, DISPLAY), None, "a moving window composes");
    assert!(layers.set_transform(7, Transform { scale_pct: 98, ..Transform::IDENTITY }));
    assert_eq!(direct(&layers, DISPLAY), None, "a scaling window composes");
    assert!(layers.set_transform(7, Transform::IDENTITY));
    assert_eq!(direct(&layers, DISPLAY), Some(0), "at rest it is scanned out");
}
