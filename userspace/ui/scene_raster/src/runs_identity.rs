// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The runs paint exactly what the pixel loops painted (TASK-0367 P4): a circle, a polygon, a
//! shadow's falloff (`span_identity`'s `shadow_ref`, here under a rounded clip as well) and a
//! 1:1 sprite, laid through the cover and over kernels, each checked byte for byte against the
//! loop of `blend` / `blend_covered` it replaced — over rows, colours, a scroll shift, a scissor
//! and a motion's rounded clip.

use super::shapes::ellipse_coverage;
use super::xform::RoundClip;
use super::RowCanvas;
use nexus_layout_types::Rgba8;

const W: i32 = 300;

fn base_row() -> Vec<u8> {
    (0..W * 4).map(|i| ((i * 53 + 7) % 247) as u8).collect()
}

/// The canvases a run is checked on: plain, scrolled and scissored, and under a rounded clip on
/// one of its corner rows (`y` lies in the clip's top corners).
fn canvases(buf: &mut [u8], y: i32, case: usize) -> RowCanvas<'_> {
    let mut c = RowCanvas::new(buf, y, W);
    match case {
        0 => {}
        1 => (c.shift_x, c.clip_x) = (9, Some((4, 250))),
        _ => {
            let clip = RoundClip { x: 20, y: 0, w: 240, h: 80, radius: 30 };
            if let Some((cols, soft)) = clip.row(y) {
                (c.clip_x, c.round) = (Some(cols), soft);
            }
        }
    }
    c
}

fn colours() -> [Rgba8; 4] {
    [
        Rgba8::new(10, 200, 30, 0),
        Rgba8::new(250, 1, 99, 1),
        Rgba8::new(72, 74, 84, 122),
        Rgba8::new(52, 54, 62, 255),
    ]
}

/// Paints `run` and `pixels` onto the same base row for every row, colour and canvas; equal.
fn same(
    name: &str,
    run: impl Fn(&mut RowCanvas<'_>, Rgba8),
    pixels: impl Fn(&mut RowCanvas<'_>, Rgba8),
) {
    for y in [0, 3, 11, 29, 40] {
        for col in colours() {
            for case in 0..3 {
                let (mut a, mut b) = (base_row(), base_row());
                run(&mut canvases(&mut a, y, case), col);
                pixels(&mut canvases(&mut b, y, case), col);
                assert!(a == b, "{name}: y={y} col={col:?} case={case}");
            }
        }
    }
}

#[test]
fn a_circle_through_the_cover_kernel_paints_what_its_pixels_painted() {
    for rect in [(30.5f32, -4.0, 200.0, 60.0), (2.0, 0.0, 41.0, 41.0), (-15.0, 5.0, 340.0, 30.0)] {
        same(
            "circle",
            |c, col| c.fill_ellipse_row(rect, col),
            |c, col| {
                let py = c.y as f32 + 0.5;
                for xx in super::shapes::floor_i32(rect.0)..super::shapes::ceil_i32(rect.0 + rect.2)
                {
                    c.blend_covered(xx, col, ellipse_coverage(xx as f32 + 0.5, py, rect));
                }
            },
        );
    }
}

#[test]
fn a_polygon_through_the_cover_kernel_paints_what_its_pixels_painted() {
    let tri = [(10.0f32, -2.0f32), (290.0, 20.0), (60.0, 45.0)];
    same(
        "polygon",
        |c, col| c.fill_polygon_row(3, |i| tri[i], col),
        |c, col| {
            // The polygon's loop as it was: every pixel through `blend_covered`.
            for xx in -5..W + 5 {
                c.blend_covered(xx, col, polygon_coverage(&tri, c.y, xx));
            }
        },
    );
}

/// The exact horizontal overlap of the even-odd polygon `pts` with pixel `xx` on row `y`, over
/// four sub-scanlines — `fill_polygon_row`'s coverage, pixel by pixel.
fn polygon_coverage(pts: &[(f32, f32)], y: i32, xx: i32) -> f32 {
    const SUB: usize = 4;
    let (l, r) = (xx as f32, xx as f32 + 1.0);
    let mut area = 0.0f32;
    for k in 0..SUB {
        let cy = y as f32 + (k as f32 + 0.5) / SUB as f32;
        let mut xs = Vec::new();
        for i in 0..pts.len() {
            let ((ax, ay), (bx, by)) = (pts[i], pts[(i + 1) % pts.len()]);
            if (ay <= cy && by > cy) || (by <= cy && ay > cy) {
                xs.push(ax + (cy - ay) / (by - ay) * (bx - ax));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        for pair in xs.chunks_exact(2) {
            area += (pair[1].min(r) - pair[0].max(l)).max(0.0);
        }
    }
    area / SUB as f32
}

#[test]
fn a_shadow_under_a_rounded_clip_through_the_cover_kernel_paints_what_its_pixels_painted() {
    use nexus_layout_types::{BoxShadow, FxPx};
    let (x, y, w, h) = (60, 12, 150, 40);
    for col in colours() {
        let shadow = BoxShadow {
            color: col,
            offset_x: FxPx::new(3),
            offset_y: FxPx::new(4),
            blur_radius: FxPx::new(14),
            spread: FxPx::new(2),
        };
        let b = super::tests::boxed(x, y, w, h, 10, Rgba8::new(0, 0, 0, 0));
        same(
            "shadow",
            |c, _| super::paint_shadow_row(c, &shadow, &b, x, y, w, h),
            |c, _| super::span_identity::shadow_ref(c, &shadow, 10, (x, y, w, h)),
        );
    }
}

#[test]
fn a_sprite_laid_one_to_one_through_the_over_kernel_paints_what_its_pixels_painted() {
    let (sw, sh) = (180usize, 2usize);
    let rgba: Vec<u8> = (0..sw * sh * 4)
        .map(|i| match i % 4 {
            3 => [0u8, 255, 17, 128, 254][(i / 4) % 5],
            _ => ((i * 31 + 3) % 256) as u8,
        })
        .collect();
    let x = 50;
    same(
        "sprite",
        |c, _| {
            let (x0, x1) = c.visible(x.max(0), (x + sw as i32).min(c.width));
            let src = &rgba[(x0 - x) as usize * 4..(x1 - x) as usize * 4];
            if !c.over_run(x0, src) {
                for px in x0..x1 {
                    let o = (px - x) as usize * 4;
                    c.blend(px, Rgba8::new(rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]));
                }
            }
        },
        |c, _| {
            for px in x..x + sw as i32 {
                let o = (px - x) as usize * 4;
                c.blend(px, Rgba8 { r: rgba[o], g: rgba[o + 1], b: rgba[o + 2], a: rgba[o + 3] });
            }
        },
    );
}
