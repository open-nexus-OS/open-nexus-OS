// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The blur oracle's scene (ADR-0094, TASK-0351 P7): one backdrop, one glass, the
//! same reads and the same expected pixels for every executor's golden — the board's chain
//! (`gpud/backend/gpu/render_blur.rs`) and the virtual GPU's (`gpud/virgl_blur_golden.rs`)
//! draw the backdrop [`backdrop_row`] writes, run the library's blur over the glass and read
//! [`READS`] back against [`expected`]: the full-resolution gaussian of the kernel table
//! (`kernels::Gaussian::full`) with the glass's saturation, held to [`tolerance`] — the reduced
//! chain's deviation from the full kernel measured on the host (`tools/nxgpu/tests/blur.rs`)
//! plus the texture unit's filter fractions. The raster is held to the same expectation
//! (`tests/frame_oracle.rs`), so the three executors are judged by one picture. Pure and
//! allocation-free.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/frame_oracle.rs`; `gpud: gpu blur radii ok` (board), `gpud: gpu blur
//!   golden ok` (virgl)
//! INVARIANTS: every read's taps lie inside the backdrop (the margin is wider than the largest
//!   radius and one bilinear fetch); the last read is the glass's corner pixel the rounded
//!   layer leaves out.

use super::kernels::Gaussian;

/// The glass: a 256x192 layer at (512, 96) in plane pixels.
pub const GLASS: (u32, u32, u32, u32) = (512, 96, 256, 192);
/// The glass's corner radius.
pub const CORNER: f32 = 16.0;
/// windowd's glass saturation (140 %).
pub const SATURATION: f32 = 1.4;
/// The backdrop around the glass: the widest taps reach 40 pixels, a bilinear fetch one more.
pub const MARGIN: u32 = 48;
/// The reads: inside the glass (its centre, near its left and top edges, a point low right)
/// and its corner pixel, which the rounded layer leaves out.
pub const READS: [(u32, u32); 5] = [(640, 192), (520, 192), (640, 100), (700, 260), (512, 96)];

/// The backdrop's rectangle in plane pixels: the glass and its margin.
pub const fn backdrop_rect() -> (u32, u32, u32, u32) {
    (GLASS.0 - MARGIN, GLASS.1 - MARGIN, GLASS.2 + 2 * MARGIN, GLASS.3 + 2 * MARGIN)
}

/// Levels a read may differ from the full-resolution kernel at `radius`: the reduced chain's
/// deviation on the host (seven levels at radius 8 at half resolution, ten at 20, 30 and 40 at
/// quarter) and the texture unit's filter fractions (three).
pub const fn tolerance(radius: u32) -> u8 {
    3 + if radius < 20 { 7 } else { 10 }
}

/// The backdrop's colour (R, G, B) at a plane pixel: red stripes, a green ramp down, a blue
/// ramp across.
pub fn backdrop(x: u32, y: u32) -> [u8; 3] {
    let (x0, y0, w, _) = backdrop_rect();
    let red = if (x / 16) % 2 == 0 { 220 } else { 30 };
    let across = x.saturating_sub(x0) * 255 / (w - 1);
    [red, y.saturating_sub(y0).min(255) as u8, across.min(255) as u8]
}

/// Row `y` of the backdrop's rectangle written into `out` (memory order B, G, R, A, opaque):
/// how many bytes it wrote (the rectangle's width times four, or none when `out` is shorter or
/// `y` is outside).
pub fn backdrop_row(y: u32, out: &mut [u8]) -> usize {
    let (x0, y0, w, h) = backdrop_rect();
    let bytes = (w * 4) as usize;
    if y < y0 || y >= y0 + h || out.len() < bytes {
        return 0;
    }
    for (i, px) in out[..bytes].chunks_exact_mut(4).enumerate() {
        let [r, g, b] = backdrop(x0 + i as u32, y);
        px.copy_from_slice(&[b, g, r, 0xff]);
    }
    bytes
}

/// The expected bytes (B, G, R) at a plane pixel inside the glass: the full-resolution
/// gaussian of the backdrop at `radius`, saturated.
pub fn expected((x, y): (u32, u32), radius: u32) -> [u8; 3] {
    let w = Gaussian::full(radius).weights();
    let mut c = [0.0f32; 3];
    for (dy, wy) in w.iter() {
        for (dx, wx) in w.iter() {
            let p = backdrop((x as i32 + dx).max(0) as u32, (y as i32 + dy).max(0) as u32);
            for (acc, v) in c.iter_mut().zip(p) {
                *acc += wx * wy * f32::from(v);
            }
        }
    }
    saturated(c)
}

/// `[r, g, b]` with the glass's saturation, rounded to bytes in memory order (B, G, R).
pub fn saturated(c: [f32; 3]) -> [u8; 3] {
    let grey = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    let s = |v: f32| libm::roundf(grey + (v - grey) * SATURATION).clamp(0.0, 255.0) as u8;
    [s(c[2]), s(c[1]), s(c[0])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_read_s_taps_lie_inside_the_backdrop() {
        let (x0, y0, w, h) = backdrop_rect();
        for (x, y) in READS {
            assert!(x >= x0 + MARGIN - 1 && x + MARGIN - 1 < x0 + w, "read {x},{y}");
            assert!(y >= y0 + MARGIN - 1 && y + MARGIN - 1 < y0 + h, "read {x},{y}");
        }
        assert_eq!(READS[4], (GLASS.0, GLASS.1), "the corner the rounded layer leaves out");
    }

    #[test]
    fn a_backdrop_row_is_its_pixels_in_memory_order() {
        let (x0, y0, w, _) = backdrop_rect();
        let mut row = [0u8; 4 * (256 + 96)];
        assert_eq!(backdrop_row(y0 + 10, &mut row), (w * 4) as usize);
        let [r, g, b] = backdrop(x0 + 3, y0 + 10);
        assert_eq!(&row[12..16], &[b, g, r, 0xff]);
        assert_eq!(backdrop_row(y0 - 1, &mut row), 0);
        assert_eq!(backdrop_row(y0, &mut row[..8]), 0);
    }

    #[test]
    fn radius_zero_expects_the_saturated_backdrop_itself() {
        let (x, y) = READS[0];
        let [r, g, b] = backdrop(x, y);
        assert_eq!(expected((x, y), 0), saturated([f32::from(r), f32::from(g), f32::from(b)]));
    }
}
