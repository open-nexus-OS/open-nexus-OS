// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan as the board GPU's passes (TASK-0215 5c-3 P4b): the slot's pass, a glass's
//! chain, a rounded layer's parts, the shadow, the reads kept inside the layout.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{
    blur_radius, Board, DrawSpec, Frame, FrameLayer, GlassCount, PassKind, PassSpec, Pipe, Planned,
    Target, Tex,
};

const DISPLAY: (u32, u32) = (1920, 1080);
/// The shared layout's rows (the atlas's end) and the half-resolution scratch's.
const LAYOUT_ROWS: u32 = 14_040;
const HALF_ROWS: u32 = 600;
const ATLAS: u32 = 5400;

fn layer(f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer { src_row_abs: ATLAS, opacity: 255, ..FrameLayer::default() };
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
    }
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> impl FnOnce(&mut FrameLayer) {
    move |l| {
        (l.dst_x, l.dst_y, l.width, l.height) = (x, y, w, h);
    }
}

fn planned(frame: &mut Frame, cmds: &[Command]) -> Planned {
    frame.layers.take_frame(cmds);
    frame.plan(DISPLAY, &|_| true)
}

fn ready(rows: u32) -> Frame {
    let mut f = Frame::new(rows);
    f.glass.ready = true;
    f
}

fn draws(b: &Board<'_>, p: &PassSpec) -> Vec<DrawSpec> {
    (0..b.draws(p)).map(|i| b.draw(p, i).expect("a draw")).collect()
}

fn slot(b: &Board<'_>) -> PassSpec {
    b.pass(b.pass_count() - 1).expect("the slot's pass")
}

/// The rounded rectangle's signed distance `layer.wgsl` computes at `uv` of `q`.
fn sdf(q: &Quad, uv: (f32, f32)) -> f32 {
    let p = (uv.0 * q.push[4] + q.push[6], uv.1 * q.push[5] + q.push[7]);
    let d = (p.0.abs() - q.push[8], p.1.abs() - q.push[9]);
    let outside = (d.0.max(0.0).powi(2) + d.1.max(0.0).powi(2)).sqrt();
    outside + d.0.max(d.1).min(0.0) - q.push[10]
}

#[test]
fn a_frame_without_glass_is_one_pass_the_wallpaper_then_each_layer() {
    let mut f = Frame::new(2048);
    let p = planned(
        &mut f,
        &[layer(|l| {
            rect(100, 50, 400, 300)(l);
            (l.src_x, l.opacity) = (64, 204);
        })],
    );
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(b.pass_count(), 1);
    let pass = slot(&b);
    assert_eq!((pass.target, pass.area), (Target::Slot, (0, 0, 1919, 1079)));
    let d = draws(&b, &pass);
    assert_eq!(d.len(), 2);
    assert_eq!(
        (d[0].pipe, d[0].tex),
        (Pipe::Opaque, Tex::Layout { x: 0, row: 0, w: 1920, h: 1080 })
    );
    assert_eq!(d[1].pipe, Pipe::Straight);
    assert_eq!(d[1].tex, Tex::Layout { x: 64, row: ATLAS, w: 400, h: 300 });
    assert_eq!((d[1].quad.x, d[1].quad.y, d[1].quad.w, d[1].quad.h), (100.0, 50.0, 400.0, 300.0));
    assert_eq!(d[1].quad.tint(), [1.0, 1.0, 1.0, 0.8]);
    assert_eq!(b.draw(&pass, 2), None);
}

#[test]
fn a_frozen_frame_is_the_base_every_scene_starts_from() {
    let mut f = ready(1024);
    let p = planned(
        &mut f,
        &[layer(|l| {
            rect(200, 100, 300, 200)(l);
            l.backdrop_blur = 20;
        })],
    );
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS).with_base(1080);
    let frozen = Tex::Layout { x: 0, row: 1080, w: 1920, h: 1080 };
    let scene = b.pass(0).expect("the scene below the glass");
    assert_eq!(scene.target, Target::Scratch);
    assert_eq!(b.draw(&scene, 0).map(|d| d.tex), Some(frozen));
    assert_eq!(b.draw(&slot(&b), 0).map(|d| d.tex), Some(frozen));
}

#[test]
fn a_rounded_layer_is_its_bands_and_its_corner_squares() {
    let mut f = Frame::new(2048);
    let p = planned(
        &mut f,
        &[layer(|l| {
            rect(100, 100, 400, 300)(l);
            l.corner_radius = 16;
        })],
    );
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    assert_eq!(d.len(), 1 + 7);
    let rects: Vec<_> =
        d[1..].iter().map(|d| (d.pipe, d.quad.x, d.quad.y, d.quad.w, d.quad.h)).collect();
    assert_eq!(
        rects,
        vec![
            (Pipe::Straight, 116.0, 100.0, 368.0, 300.0),
            (Pipe::Straight, 100.0, 116.0, 16.0, 268.0),
            (Pipe::Straight, 484.0, 116.0, 16.0, 268.0),
            (Pipe::Rounded, 100.0, 100.0, 16.0, 16.0),
            (Pipe::Rounded, 484.0, 100.0, 16.0, 16.0),
            (Pipe::Rounded, 100.0, 384.0, 16.0, 16.0),
            (Pipe::Rounded, 484.0, 384.0, 16.0, 16.0),
        ]
    );
    // Each part samples its own rectangle of the surface, one to one.
    assert_eq!(d[2].tex, Tex::Layout { x: 0, row: ATLAS + 16, w: 16, h: 268 });
    assert_eq!(d[7].tex, Tex::Layout { x: 384, row: ATLAS + 284, w: 16, h: 16 });
    // A corner square cuts the whole layer's corner: its outer corner outside, its inner inside.
    let top_left = &d[4].quad;
    assert!(sdf(top_left, (0.0, 0.0)) > 6.0);
    assert!((sdf(top_left, (1.0, 1.0)) + 16.0).abs() < 1e-3);
    let bottom_right = &d[7].quad;
    assert!(sdf(bottom_right, (1.0, 1.0)) > 6.0);
    assert!(sdf(bottom_right, (0.0, 0.0)) < 0.0);
}

#[test]
fn a_band_spread_over_a_larger_layer_is_one_draw() {
    let mut f = Frame::new(2048);
    let p = planned(
        &mut f,
        &[layer(|l| {
            rect(0, 0, 600, 400)(l);
            (l.content_w, l.content_h, l.corner_radius) = (500, 300, 12);
        })],
    );
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    assert_eq!(d.len(), 2);
    assert_eq!(
        (d[1].pipe, d[1].tex),
        (Pipe::Rounded, Tex::Layout { x: 0, row: ATLAS, w: 500, h: 300 })
    );
    assert_eq!((d[1].quad.w, d[1].quad.h), (600.0, 400.0));
}

#[test]
fn a_shadow_draws_before_its_layer() {
    let mut f = Frame::new(2048);
    let p = planned(
        &mut f,
        &[layer(|l| {
            rect(200, 200, 300, 200)(l);
            (l.shadow_blur, l.shadow_offset_y, l.shadow_alpha) = (24, 8, 102);
        })],
    );
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    assert_eq!(
        d.iter().map(|d| d.pipe).collect::<Vec<_>>(),
        [Pipe::Opaque, Pipe::Shadow, Pipe::Straight]
    );
    let s = d[1].quad;
    assert_eq!(d[1].tex, Tex::None);
    assert_eq!(
        s,
        Quad::shadow(200.0, 200.0, 300.0, 200.0, 0.0, 24.0, [0.0, 8.0], [0.0, 0.0, 0.0, 0.4])
    );
}

#[test]
fn a_glass_to_write_draws_its_scene_below_then_blurs_it_into_its_rows() {
    let mut f = ready(2048);
    let window = layer(rect(100, 100, 800, 600));
    let panel = layer(|l| {
        rect(300, 200, 400, 200)(l);
        (l.backdrop_blur, l.corner_radius) = (40, 20);
    });
    let p = planned(&mut f, &[window, panel]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(b.glass, GlassCount { writes: 1, reads: 0, off: 0 });
    assert_eq!(b.pass_count(), 4);
    // The scene below the glass, over the reach of the horizontal taps (radius 40 at quarter
    // resolution: 10 taps four pixels apart).
    let scene = b.pass(0).unwrap();
    assert_eq!(scene.target, Target::Scratch);
    assert_eq!(scene.kind, PassKind::Scene { upto: 1 });
    let (vw, vh, taps) = (102, 52, 10);
    let reach = 4 * taps + 4;
    assert_eq!(
        scene.area,
        (300 - reach, 200 - reach, 300 + 4 * vw + 4 * taps - 5, 200 + 4 * vh + 4 * taps - 5)
    );
    assert_eq!(
        draws(&b, &scene).iter().map(|d| d.pipe).collect::<Vec<_>>(),
        [Pipe::Opaque, Pipe::Straight]
    );
    // The horizontal pass from the scratch into the reduced scratch (the QEMU geometry).
    let half = b.pass(1).unwrap();
    let hh = vh + 2 * taps;
    assert_eq!((half.target, half.area), (Target::Half, (0, 0, vw - 1, hh - 1)));
    let (fw, fh) = (1920.0, 1080.0);
    let across = Quad::blur(
        0.0,
        0.0,
        vw as f32,
        hh as f32,
        [4.0 * vw as f32 / fw, 4.0 * hh as f32 / fh, 296.0 / fw, (196.0 - 40.0) / fh],
        [4.0 / fw, 0.0],
        1.0,
    );
    assert_eq!(
        half.kind,
        PassKind::One(DrawSpec { pipe: Pipe::BlurH(40), quad: across, tex: Tex::Scratch })
    );
    // The vertical pass into the glass's cache rows.
    let rows = b.pass(2).unwrap();
    assert_eq!((rows.target, rows.area), (Target::Cache, (0, 0, vw - 1, vh - 1)));
    let hr = HALF_ROWS as f32;
    let down = Quad::blur(
        0.0,
        0.0,
        vw as f32,
        vh as f32,
        [vw as f32 / fw, vh as f32 / hr, 0.0, taps as f32 / hr],
        [0.0, 1.0 / hr],
        1.0,
    );
    let half_tex = Tex::Half { rows: HALF_ROWS };
    assert_eq!(
        rows.kind,
        PassKind::One(DrawSpec { pipe: Pipe::BlurV(40), quad: down, tex: half_tex })
    );
    // The slot: the wallpaper, the window, the glass drawn back from its rows, the panel.
    let d = draws(&b, &slot(&b));
    assert_eq!(d[2].pipe, Pipe::Rounded);
    assert_eq!(d[2].tex, Tex::Cache { x: 1, row: 1, w: 100, h: 50 });
    assert_eq!(d.len(), 2 + 1 + 7);
}

#[test]
fn a_glass_read_from_its_cache_adds_no_pass_and_one_without_rows_is_counted() {
    let mut f = ready(2048);
    let pane = layer(|l| {
        rect(0, 0, 300, 200)(l);
        l.backdrop_blur = 20;
    });
    planned(&mut f, &[pane.clone()]);
    let p = f.plan(DISPLAY, &|_| true);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!((b.glass, b.pass_count()), (GlassCount { writes: 0, reads: 1, off: 0 }, 1));
    assert_eq!(draws(&b, &slot(&b))[1].pipe, Pipe::Opaque);
    // No cache rows left: the glass is drawn without its blurred backdrop, and counted.
    let mut tight = ready(10);
    let p = planned(&mut tight, &[pane]);
    let b = Board::new(&tight, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!((b.glass, b.pass_count()), (GlassCount { writes: 0, reads: 0, off: 1 }, 1));
    assert_eq!(
        draws(&b, &slot(&b)).iter().map(|d| d.pipe).collect::<Vec<_>>(),
        [Pipe::Opaque, Pipe::Straight]
    );
}

#[test]
fn test_reject_reading_past_the_layout() {
    let mut f = Frame::new(2048);
    let p = planned(
        &mut f,
        &[
            // Its last 40 rows lie past the layout: the draw shrinks to what the layout holds.
            layer(|l| {
                rect(0, 0, 100, 100)(l);
                l.src_row_abs = LAYOUT_ROWS - 60;
            }),
            // Wholly past it: nothing is drawn.
            layer(|l| {
                rect(0, 0, 100, 100)(l);
                l.src_row_abs = LAYOUT_ROWS + 5;
            }),
        ],
    );
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    assert_eq!(d.len(), 2);
    assert_eq!(d[1].tex, Tex::Layout { x: 0, row: LAYOUT_ROWS - 60, w: 100, h: 60 });
    assert_eq!(d[1].quad.h, 60.0);
}

#[test]
fn the_blur_radius_is_the_nearest_the_library_holds() {
    let picks: Vec<_> =
        [0, 8, 12, 14, 20, 25, 30, 35, 40, 100].iter().map(|&r| blur_radius(r)).collect();
    assert_eq!(picks, [8, 8, 8, 8, 20, 20, 30, 30, 40, 40]);
}

#[test]
fn the_slot_pass_is_limited_to_the_area_the_executor_names_inside_the_display() {
    let mut f = Frame::new(2048);
    let p = planned(&mut f, &[layer(rect(100, 50, 400, 300))]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS).with_area((10, 20, 3000, 4000));
    assert_eq!(slot(&b).area, (10, 20, 1919, 1079));
    assert_eq!(b.draws(&slot(&b)), 2, "the whole scene is drawn into the area");
}

#[test]
fn the_work_of_a_pass_is_its_draws_clipped_to_its_area() {
    let mut f = Frame::new(2048);
    let p = planned(&mut f, &[layer(rect(100, 50, 400, 300))]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    // The whole display: the wallpaper's 1920×1080 and the layer's 400×300.
    assert_eq!(b.work_px(&slot(&b)), 1920 * 1080 + 400 * 300);
    // A slot that lacks only a 200×100 corner of the layer: both draws clipped to it.
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS).with_area((400, 250, 599, 349));
    assert_eq!(b.work_px(&slot(&b)), 200 * 100 + 100 * 100);
}

#[test]
fn a_glass_written_for_a_band_blurs_the_band_alone() {
    let mut f = ready(2048);
    let window = layer(|l| {
        rect(100, 100, 800, 600)(l);
        l.layer_id = 1;
    });
    let panel = layer(|l| {
        rect(300, 200, 400, 200)(l);
        (l.backdrop_blur, l.corner_radius, l.layer_id) = (40, 20, 2);
    });
    planned(&mut f, &[window.clone(), panel.clone()]);
    // A 20×10 change under the panel at (500, 300): origin 0 (the base) is below the glass.
    let damage =
        Command::Damage { rect: TileRect { x: 500, y: 300, width: 20, height: 10 }, origin: 0 };
    let p = planned(&mut f, &[window, panel, damage]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(b.glass, GlassCount { writes: 1, reads: 0, off: 0 });
    let (taps, s) = (10, 4);
    let reach = s * taps + s;
    // The scene below over the band's reach.
    let scene = b.pass(0).unwrap();
    assert_eq!(scene.area, (500 - reach, 300 - reach, 520 + reach - 1, 310 + reach - 1));
    // The horizontal pass: the band's half-resolution columns (origin x − 4), one of margin;
    // its rows plus the vertical taps (origin y − 4 − halo).
    let half = b.pass(1).unwrap();
    let (c0, c1) = ((500 - 296) / s - 1, (519 - 296) / s + 1);
    let (h0, h1) = ((300 + 40 - 196) / s - taps - 1, (309 + 40 - 196) / s + taps + 1);
    assert_eq!(half.area, (c0, h0, c1, h1));
    // The vertical pass: the band's cache rows, one of margin.
    let rows = b.pass(2).unwrap();
    assert_eq!(rows.area, (c0, (300 - 196) / s - 1, c1, (309 - 196) / s + 1));
    // Far less work than the whole glass's chain.
    assert!(b.work_px(&scene) < 100 * 1000 / 2);
}

#[test]
fn two_glasses_written_apart_share_one_scene_and_two_blur_passes() {
    let mut f = ready(2048);
    let window = layer(|l| {
        rect(100, 100, 800, 600)(l);
        l.layer_id = 1;
    });
    let bar = layer(|l| {
        rect(0, 0, 1920, 40)(l);
        (l.backdrop_blur, l.layer_id) = (40, 2);
    });
    let dock = layer(|l| {
        rect(560, 980, 800, 90)(l);
        (l.backdrop_blur, l.layer_id) = (40, 3);
    });
    // The first frame writes both (new rows): the chains share passes — four in all.
    let p = planned(&mut f, &[window.clone(), bar.clone(), dock.clone()]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(b.glass, GlassCount { writes: 2, reads: 0, off: 0 });
    assert_eq!(b.pass_count(), 4);
    let scene = b.pass(0).unwrap();
    assert_eq!((scene.target, scene.kind), (Target::Scratch, PassKind::Scene { upto: 1 }));
    assert_eq!(scene.area, (0, 0, 1919, 1079), "both reaches: the bar's top, the dock's bottom");
    let half = b.pass(1).unwrap();
    assert_eq!((half.target, half.kind), (Target::Half, PassKind::Blurs { vertical: false }));
    assert_eq!(b.draws(&half), 2);
    let (d0, d1) = (b.draw(&half, 0).unwrap(), b.draw(&half, 1).unwrap());
    assert_eq!((d0.pipe, d1.pipe), (Pipe::BlurH(40), Pipe::BlurH(40)));
    // The dock's horizontal result lies below the bar's in the half scratch: its rows follow.
    assert!(d1.quad.y >= d0.quad.y + d0.quad.h, "{} {} {}", d0.quad.y, d0.quad.h, d1.quad.y);
    let rows = b.pass(2).unwrap();
    assert_eq!((rows.target, rows.kind), (Target::Cache, PassKind::Blurs { vertical: true }));
    assert_eq!(b.draws(&rows), 2);
    assert_eq!(b.pass(3).unwrap().target, Target::Slot);
    assert_eq!(b.pass(4), None);
}

#[test]
fn a_glass_over_another_written_glass_keeps_its_own_chain() {
    let mut f = ready(2048);
    let dock = layer(|l| {
        rect(560, 980, 800, 90)(l);
        (l.backdrop_blur, l.layer_id) = (40, 2);
    });
    let pill = layer(|l| {
        rect(600, 1000, 100, 40)(l);
        (l.backdrop_blur, l.layer_id) = (20, 3);
    });
    let p = planned(&mut f, &[dock, pill]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(b.glass.writes, 2);
    assert_eq!(b.pass_count(), 7, "the pill's backdrop is the dock's fresh blur: one by one");
    assert_eq!(b.pass(0).unwrap().kind, PassKind::Scene { upto: 0 });
    assert_eq!(b.pass(3).unwrap().kind, PassKind::Scene { upto: 1 });
}
