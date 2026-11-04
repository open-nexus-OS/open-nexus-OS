// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The raster is the oracle (ADR-0094, TASK-0351 P7): the raster's gaussian of the oracle's
//! backdrop, saturated, is the oracle's expected pixel at every read and every library radius
//! — the kernel table read by both, the picture judged once.

use nexus_gfx::frame::oracle::{backdrop_rect, backdrop_row, expected, tolerance, READS};
use nexus_gfx::frame::BLUR_RADII;
use nexus_gfx::raster::{blur_gaussian, saturate, Surface};

/// The backdrop as a surface: the oracle's rectangle, rows tight.
fn backdrop_surface() -> (Vec<u8>, u32, u32) {
    let (_, y0, w, h) = backdrop_rect();
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for (i, row) in buf.chunks_exact_mut((w * 4) as usize).enumerate() {
        assert_eq!(backdrop_row(y0 + i as u32, row), row.len());
    }
    (buf, w, h)
}

#[test]
fn the_raster_s_gaussian_is_the_oracle_s_picture_at_every_radius() {
    let (x0, y0, _, _) = backdrop_rect();
    for radius in BLUR_RADII {
        let (mut buf, w, h) = backdrop_surface();
        let mut s = Surface::new(&mut buf, w);
        let (mut row, mut col) = (vec![0u8; (w * 4) as usize], vec![0u8; (h * 4) as usize]);
        blur_gaussian(&mut s, 0, 0, w, h, radius, &mut row, &mut col).expect("the blur");
        saturate(&mut s, 0, 0, w, h, 140);
        for (x, y) in &READS[..4] {
            let at = (((y - y0) * w + (x - x0)) * 4) as usize;
            let got = &buf[at..at + 3];
            let want = expected((*x, *y), radius);
            let d = got.iter().zip(&want).map(|(g, w)| g.abs_diff(*w)).max().unwrap_or(0);
            // The raster rounds to bytes between its two passes: a level, two at most.
            assert!(d <= 2, "radius {radius} at {x},{y}: raster {got:?} oracle {want:?}");
        }
    }
}

#[test]
fn the_tolerance_is_the_reduced_chain_s_deviation_and_the_filter_s_fractions() {
    assert_eq!(tolerance(8), 10);
    assert_eq!(tolerance(20), 13);
    assert_eq!(tolerance(40), 13);
}
