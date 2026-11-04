// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A draw cut to its pass's area (TASK-0351 P5): the part shades the same rounded-rectangle
//! distance and samples the same texels as the whole at every pixel it keeps; a shadow keeps
//! its falloff; a spread band and a blur are kept whole; a draw outside is none.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::frame::{clip_draw, DrawSpec, Pipe, Tex};

/// The rounded rectangle's signed distance `layer.wgsl` computes at `uv` of `q`.
fn sdf(q: &Quad, uv: (f32, f32)) -> f32 {
    let p = (uv.0 * q.push[4] + q.push[6], uv.1 * q.push[5] + q.push[7]);
    let d = (p.0.abs() - q.push[8], p.1.abs() - q.push[9]);
    let outside = (d.0.max(0.0).powi(2) + d.1.max(0.0).powi(2)).sqrt();
    outside + d.0.max(d.1).min(0.0) - q.push[10]
}

/// The `uv` of the display pixel centre `(x, y)` in `q`.
fn uv(q: &Quad, (x, y): (u32, u32)) -> (f32, f32) {
    ((x as f32 + 0.5 - q.x) / q.w, (y as f32 + 0.5 - q.y) / q.h)
}

/// The layout texel the pixel `(x, y)` of `d` samples (a one-to-one draw).
fn texel(d: &DrawSpec, (x, y): (u32, u32)) -> (u32, u32) {
    let Tex::Layout { x: tx, row, w, h } = d.tex else { panic!("a layout texture") };
    let (u, v) = uv(&d.quad, (x, y));
    (tx + (u * w as f32) as u32, row + (v * h as f32) as u32)
}

fn straight(rect: (f32, f32, f32, f32), tex: Tex) -> DrawSpec {
    let (x, y, w, h) = rect;
    DrawSpec { pipe: Pipe::Straight, quad: Quad::layer(x, y, w, h, [1.0; 4], 0.0), tex }
}

#[test]
fn a_draw_inside_the_area_is_itself_and_one_outside_is_none() {
    let d = straight((100.0, 100.0, 50.0, 40.0), Tex::Layout { x: 7, row: 9, w: 50, h: 40 });
    assert_eq!(clip_draw(&d, (0, 0, 1919, 1079)), Some(d));
    assert_eq!(clip_draw(&d, (100, 100, 149, 139)), Some(d));
    assert_eq!(clip_draw(&d, (150, 100, 200, 200)), None);
    assert_eq!(clip_draw(&d, (0, 0, 99, 99)), None);
}

#[test]
fn a_cut_draw_samples_the_same_texels_at_every_pixel_it_keeps() {
    let whole =
        straight((100.0, 100.0, 200.0, 200.0), Tex::Layout { x: 10, row: 20, w: 200, h: 200 });
    let part = clip_draw(&whole, (150, 160, 169, 179)).expect("the part");
    assert_eq!((part.quad.x, part.quad.y, part.quad.w, part.quad.h), (150.0, 160.0, 20.0, 20.0));
    assert_eq!(part.tex, Tex::Layout { x: 60, row: 80, w: 20, h: 20 });
    assert_eq!(part.pipe, Pipe::Straight);
    for p in [(150, 160), (169, 179), (160, 170)] {
        assert_eq!(texel(&part, p), texel(&whole, p), "pixel {p:?}");
    }
    // A buffer's band moves the same way; the band's source and slot stay.
    let buffer =
        DrawSpec { tex: Tex::Buffer { source: 3, slot: 1, x: 0, row: 0, w: 200, h: 200 }, ..whole };
    let cut = clip_draw(&buffer, (150, 160, 169, 179)).expect("the part");
    assert_eq!(cut.tex, Tex::Buffer { source: 3, slot: 1, x: 50, row: 60, w: 20, h: 20 });
}

#[test]
fn a_cut_corner_square_keeps_the_whole_rounded_rectangle() {
    // The top-left corner square of a layer at (100, 100) 300x200 with radius 24, cut to the
    // pass area's twelve pixels inside it: the distance at a kept pixel is the whole's.
    let whole = DrawSpec {
        pipe: Pipe::Rounded,
        quad: Quad::part((100.0, 100.0, 300.0, 200.0), (100.0, 100.0, 24.0, 24.0), [1.0; 4], 24.0),
        tex: Tex::Layout { x: 0, row: 0, w: 24, h: 24 },
    };
    let part = clip_draw(&whole, (108, 112, 119, 123)).expect("the part");
    assert_eq!((part.quad.x, part.quad.y, part.quad.w, part.quad.h), (108.0, 112.0, 12.0, 12.0));
    assert_eq!(part.tex, Tex::Layout { x: 8, row: 12, w: 12, h: 12 });
    for p in [(108, 112), (119, 123), (113, 117)] {
        let (a, b) = (sdf(&part.quad, uv(&part.quad, p)), sdf(&whole.quad, uv(&whole.quad, p)));
        assert!((a - b).abs() < 1e-3, "pixel {p:?}: {a} against {b}");
    }
    // The corner's arc is where the whole put it: outside at the square's own corner, inside
    // at the layer's.
    assert!(sdf(&whole.quad, uv(&whole.quad, (100, 100))) > 0.0);
    assert!(sdf(&whole.quad, uv(&whole.quad, (123, 123))) < 0.0);
}

#[test]
fn a_cut_shadow_keeps_its_falloff_and_its_offset() {
    let whole = DrawSpec {
        pipe: Pipe::Shadow,
        quad: Quad::shadow(
            200.0,
            200.0,
            400.0,
            300.0,
            16.0,
            24.0,
            [0.0, 8.0],
            [0.0, 0.0, 0.0, 0.5],
        ),
        tex: Tex::None,
    };
    let part = clip_draw(&whole, (0, 0, 219, 1079)).expect("the part");
    assert_eq!((part.quad.x, part.quad.y), (176.0, 184.0));
    assert_eq!((part.quad.w, part.quad.h), (44.0, whole.quad.h));
    assert_eq!(part.quad.push[8..12], whole.quad.push[8..12], "the shape and one over the blur");
    assert_eq!(part.quad.push[12..14], whole.quad.push[12..14], "the offset");
    for p in [(176, 184), (219, 500), (200, 300)] {
        let (a, b) = (sdf(&part.quad, uv(&part.quad, p)), sdf(&whole.quad, uv(&whole.quad, p)));
        assert!((a - b).abs() < 1e-3, "pixel {p:?}: {a} against {b}");
    }
}

#[test]
fn test_reject_cutting_a_spread_band_a_blur_or_a_quad_off_the_grid() {
    // A 100x100 band spread over a 200x200 layer: its texels are not the quad's pixels.
    let spread =
        straight((100.0, 100.0, 200.0, 200.0), Tex::Layout { x: 0, row: 0, w: 100, h: 100 });
    assert_eq!(clip_draw(&spread, (150, 150, 169, 169)), Some(spread));
    // A blur draw was cut by its chain; its source window is not a texel rectangle.
    let blur = DrawSpec {
        pipe: Pipe::BlurH(20),
        quad: Quad::blur(0.0, 0.0, 1920.0, 540.0, [1.0, 0.5, 0.0, 0.0], [0.001, 0.0], 1.0),
        tex: Tex::Scratch,
    };
    assert_eq!(clip_draw(&blur, (10, 10, 19, 19)), Some(blur));
    // A quad between pixels keeps its whole band.
    let between =
        straight((100.5, 100.0, 200.0, 200.0), Tex::Layout { x: 0, row: 0, w: 200, h: 200 });
    assert_eq!(clip_draw(&between, (150, 150, 169, 169)), Some(between));
}
