// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Occlusion and the cheap programs on the board (TASK-0335 P2, TASK-0336, TASK-0337): an
//! opaque layer cuts what lies below it into the pieces around it — the wallpaper, the layers,
//! their shadows, a glass's cached backdrop on its grid — a solid patch spread over a rounded
//! window is its parts, and opaque straight parts run the opaque program.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::{Board, DrawSpec, Frame, FrameLayer, PassSpec, Pipe, Planned, Tex};

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
        key: l.key,
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

/// The pixels a list of draws covers (their quads' areas), and that none of them overlaps.
fn disjoint_area(d: &[DrawSpec]) -> f32 {
    for (i, a) in d.iter().enumerate() {
        for b in d.iter().skip(i + 1) {
            let apart = a.quad.x + a.quad.w <= b.quad.x
                || b.quad.x + b.quad.w <= a.quad.x
                || a.quad.y + a.quad.h <= b.quad.y
                || b.quad.y + b.quad.h <= a.quad.y;
            assert!(apart, "pieces overlap: {:?} {:?}", a.quad, b.quad);
        }
    }
    d.iter().map(|d| d.quad.w * d.quad.h).sum()
}

#[test]
fn an_opaque_pane_cuts_the_wallpaper_and_the_layers_below_it_and_spares_what_it_covers() {
    let mut f = Frame::new(2048);
    let window = layer(|l| rect(100, 100, 800, 600)(l));
    // A layer wholly under the pane: not drawn at all.
    let hidden = layer(|l| {
        rect(320, 270, 100, 100)(l);
        l.src_x = 900;
    });
    let pane = layer(|l| {
        rect(300, 250, 400, 300)(l);
        (l.src_x, l.opaque) = (1000, true);
    });
    let p = planned(&mut f, &[window, hidden, pane]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    // The wallpaper's four bands, the window's four, nothing of the hidden one, the pane.
    assert_eq!(d.len(), 9);
    let wall = &d[..4];
    assert!(wall.iter().all(|w| w.pipe == Pipe::Opaque));
    assert_eq!(disjoint_area(wall), (1920 * 1080 - 400 * 300) as f32);
    // Every wallpaper piece samples the plane where it lies (one to one).
    for w in wall {
        let Tex::Layout { x, row, w: tw, h: th } = w.tex else { panic!("the plane") };
        assert_eq!(
            (x as f32, row as f32, tw as f32, th as f32),
            (w.quad.x, w.quad.y, w.quad.w, w.quad.h)
        );
    }
    let win = &d[4..8];
    assert_eq!(disjoint_area(win), (800 * 600 - 400 * 300) as f32);
    for w in win {
        let Tex::Layout { x, row, .. } = w.tex else { panic!("the atlas") };
        // The piece's texels are the part of the band its pixels showed.
        assert_eq!((x as f32, row as f32), (w.quad.x - 100.0, ATLAS as f32 + w.quad.y - 100.0));
    }
    assert_eq!(d[8].tex, Tex::Layout { x: 1000, row: ATLAS, w: 400, h: 300 });
    // The textured work of the slot: 1920×1080 + 800×600 + 400×300, less twice the pane.
    assert_eq!(b.work_px(&slot(&b)), 1920 * 1080 + 800 * 600 + 400 * 300 - 2 * 400 * 300);
}

#[test]
fn test_reject_a_pane_without_the_flag_or_at_less_than_full_opacity_as_an_occluder() {
    let mut f = Frame::new(2048);
    let window = layer(|l| rect(100, 100, 800, 600)(l));
    let plain = layer(|l| {
        rect(300, 250, 400, 300)(l);
        l.src_x = 1000;
    });
    let p = planned(&mut f, &[window.clone(), plain]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(draws(&b, &slot(&b)).len(), 3, "no flag: nothing is cut");
    let faded = layer(|l| {
        rect(300, 250, 400, 300)(l);
        (l.src_x, l.opaque, l.opacity) = (1000, true, 200);
    });
    let p = planned(&mut f, &[window, faded]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    assert_eq!(draws(&b, &slot(&b)).len(), 3, "faded: what is below shows through");
}

#[test]
fn a_rounded_pane_claims_its_rectangle_inside_the_corners_and_cuts_a_shadow_too() {
    let mut f = Frame::new(2048);
    let window = layer(|l| {
        rect(100, 100, 800, 600)(l);
        (l.shadow_blur, l.shadow_offset_y, l.shadow_alpha) = (24, 8, 120);
    });
    let pane = layer(|l| {
        rect(300, 250, 400, 300)(l);
        (l.src_x, l.opaque, l.corner_radius) = (1000, true, 16);
    });
    let p = planned(&mut f, &[window, pane]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    // The occluder is the pane inset by its radius: (316, 266, 368, 268).
    let wall = &d[..4];
    assert_eq!(disjoint_area(wall), (1920 * 1080 - 368 * 268) as f32);
    // The window's shadow: its quad (the shape grown by the blur, moved down) in four pieces,
    // each a part of the whole shadow's quad with the whole shape's push constants.
    let shadow: Vec<_> = d.iter().filter(|x| x.pipe == Pipe::Shadow).collect();
    assert_eq!(shadow.len(), 4);
    let whole = Quad::shadow(
        100.0,
        100.0,
        800.0,
        600.0,
        0.0,
        24.0,
        [0.0, 8.0],
        [0.0, 0.0, 0.0, 120.0 / 255.0],
    );
    for s in &shadow {
        assert_eq!(s.quad.push[8..12], whole.push[8..12]);
        assert_eq!(s.quad.push[6], s.quad.x - (whole.x + whole.w * 0.5));
        assert_eq!(s.quad.push[7], s.quad.y - (whole.y + whole.h * 0.5));
    }
    assert_eq!(
        shadow.iter().map(|s| s.quad.w * s.quad.h).sum::<f32>(),
        whole.w * whole.h - 368.0 * 268.0
    );
    // The pane itself: its rounded parts, drawn whole.
    assert_eq!(
        d.iter().filter(|x| x.tex == Tex::Layout { x: 1000, row: ATLAS, w: 400, h: 300 }).count(),
        0
    );
    assert!(d.iter().any(|x| matches!(x.tex, Tex::Layout { x: 1000, .. })));
}

#[test]
fn a_glass_back_under_a_pane_is_cut_on_its_cache_grid() {
    let mut f = ready(2048);
    let window = layer(|l| {
        rect(100, 100, 800, 600)(l);
        (l.backdrop_blur, l.layer_id) = (40, 1);
    });
    let pane = layer(|l| {
        rect(303, 251, 400, 300)(l);
        (l.src_x, l.opaque, l.layer_id) = (1000, true, 1);
    });
    let p = planned(&mut f, &[window, pane]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    let s = nexus_gfx::frame::blur_scale(nexus_gfx::frame::blur_radius(40));
    let back: Vec<_> = d.iter().filter(|x| matches!(x.tex, Tex::Cache { .. })).collect();
    assert_eq!(back.len(), 4, "the glass back around the pane");
    for x in &back {
        // Each piece starts and ends on the scale's grid from the layer's origin, and its
        // cache texels are where the whole back's were.
        assert_eq!((x.quad.x as u32 - 100) % s, 0);
        assert_eq!((x.quad.y as u32 - 100) % s, 0);
        let Tex::Cache { x: tx, row, .. } = x.tex else { unreachable!() };
        assert_eq!(tx, 1 + (x.quad.x as u32 - 100) / s);
        assert_eq!(row, 1 + (x.quad.y as u32 - 100) / s);
    }
    // Shrunk inward to the grid, the occluder leaves a little more of the back drawn — never
    // less than the whole minus the pane.
    let drawn = back.iter().map(|x| x.quad.w * x.quad.h).sum::<f32>();
    assert!(drawn >= (800 * 600 - 400 * 300) as f32 && drawn < (800 * 600) as f32);
}

#[test]
fn a_solid_patch_spread_over_a_rounded_layer_is_its_parts_each_sampling_the_patch() {
    let mut f = Frame::new(2048);
    // A window's starting surface: a 14×14 patch of one colour over a 600×400 frame.
    let starting = layer(|l| {
        rect(200, 150, 600, 400)(l);
        (l.content_w, l.content_h, l.corner_radius) = (14, 14, 12);
        (l.src_x, l.solid, l.opaque) = (40, true, true);
    });
    let p = planned(&mut f, &[starting]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let d = draws(&b, &slot(&b));
    // The wallpaper around it (four bands: the layer is opaque inside its corners), then the
    // layer's seven parts — every one sampling the patch.
    let parts: Vec<_> = d.iter().filter(|x| matches!(x.tex, Tex::Layout { x: 40, .. })).collect();
    assert_eq!(parts.len(), 7);
    for x in &parts {
        assert_eq!(x.tex, Tex::Layout { x: 40, row: ATLAS, w: 14, h: 14 });
    }
    // The straight parts run the opaque program, the corners the rounded one.
    assert_eq!(parts.iter().filter(|x| x.pipe == Pipe::Opaque).count(), 3);
    assert_eq!(parts.iter().filter(|x| x.pipe == Pipe::Rounded).count(), 4);
    let covered: f32 = parts.iter().map(|x| x.quad.w * x.quad.h).sum();
    assert_eq!(covered, 600.0 * 400.0);
}

#[test]
fn test_reject_the_opaque_program_for_a_faded_or_unflagged_layer() {
    let mut f = Frame::new(2048);
    let pipe_of = |f: &mut Frame, opaque: bool, opacity: u32| {
        let l = layer(|l| {
            rect(100, 100, 300, 200)(l);
            (l.src_x, l.opaque, l.opacity) = (500, opaque, opacity);
        });
        let p = planned(f, &[l]);
        let b = Board::new(f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
        let d = draws(&b, &slot(&b));
        d.iter().find(|x| matches!(x.tex, Tex::Layout { x: 500, .. })).map(|x| x.pipe)
    };
    assert_eq!(pipe_of(&mut f, true, 255), Some(Pipe::Opaque));
    assert_eq!(pipe_of(&mut f, true, 200), Some(Pipe::Straight), "faded: it blends");
    assert_eq!(pipe_of(&mut f, false, 255), Some(Pipe::Straight), "unflagged: it blends");
}
