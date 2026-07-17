// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests for the canonical software rasterizer ([`nexus_gfx::raster`]) — the
//! single source of truth both the CPU reference backend and the live GPU
//! driver's CPU/VMO fallback run. These lock the primitives' behaviour
//! (anti-aliased coverage, the shared blend math, allocation-free scratch).

use nexus_gfx::raster::{
    self, blend_over_px, blend_premultiplied_px, blur_box, fill_rect_solid, fill_rounded_aa,
    Surface,
};

const W: u32 = 32;
const H: u32 = 32;

fn fb() -> Vec<u8> {
    vec![0u8; (W * H * 4) as usize]
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn fill_rect_solid_writes_exact_color() {
    let mut buf = fb();
    let mut s = Surface::new(&mut buf, W);
    fill_rect_solid(&mut s, 4, 4, 8, 8, [10, 20, 30, 255]);
    assert_eq!(px(&buf, 5, 5), [10, 20, 30, 255]);
    // Outside the rect stays clear.
    assert_eq!(px(&buf, 0, 0), [0, 0, 0, 0]);
    assert_eq!(px(&buf, 12, 12), [0, 0, 0, 0]);
}

#[test]
fn rounded_fill_is_anti_aliased() {
    let mut buf = fb();
    let mut s = Surface::new(&mut buf, W);
    // A rounded rect filling most of the surface with a generous radius.
    fill_rounded_aa(&mut s, 2, 2, 28, 28, 8, [200, 100, 50, 255]);
    // Deep interior is fully covered.
    let center = px(&buf, 16, 16);
    assert_eq!(center, [200, 100, 50, 255]);
    // The extreme corner of the bounding box is outside the rounded shape.
    assert_eq!(px(&buf, 2, 2), [0, 0, 0, 0]);
    // Somewhere along the rounded corner there must be a partially-covered
    // (anti-aliased) pixel — neither fully clear nor fully opaque. This is the
    // upgrade over the old hard-edged inside/outside test.
    let mut found_aa = false;
    for y in 2..12 {
        for x in 2..12 {
            let a = px(&buf, x, y)[3];
            if a > 0 && a < 255 {
                found_aa = true;
            }
        }
    }
    assert!(found_aa, "expected at least one anti-aliased edge pixel");
}

#[test]
fn blend_over_px_matches_fixed_point_reference() {
    // Half-alpha grey over black: (128*128 + 127*0)*257+32768 >> 16 ≈ 64.
    let out = blend_over_px([0, 0, 0, 0], [128, 128, 128, 128]);
    assert_eq!([out[0], out[1], out[2]], [64, 64, 64]);
    // Opaque source replaces the destination exactly.
    assert_eq!(blend_over_px([9, 9, 9, 255], [1, 2, 3, 255]), [1, 2, 3, 255]);
    // Zero-alpha source leaves the destination untouched.
    assert_eq!(blend_over_px([7, 8, 9, 255], [1, 2, 3, 0]), [7, 8, 9, 255]);
}

#[test]
fn blend_premultiplied_px_is_additive_over_inverse_alpha() {
    // Premultiplied opaque white over anything → white, opaque.
    assert_eq!(
        blend_premultiplied_px([10, 20, 30, 255], [255, 255, 255, 255]),
        [255, 255, 255, 255]
    );
    // Premultiplied half-cover (src already scaled) over white keeps the
    // destination contribution at (1 - a).
    let out = blend_premultiplied_px([200, 200, 200, 255], [0, 0, 0, 128]);
    assert_eq!(out[3], 255);
    assert!(out[0] > 90 && out[0] < 110, "dst*(1-a) ≈ 100, got {}", out[0]);
}

#[test]
fn blit_within_copies_a_region() {
    let mut buf = fb();
    {
        let mut s = Surface::new(&mut buf, W);
        fill_rect_solid(&mut s, 0, 0, 4, 4, [1, 2, 3, 255]);
    }
    let mut scratch = [0u8; (W * 4) as usize];
    let mut s = Surface::new(&mut buf, W);
    raster::blit_within(&mut s, 0, 0, 10, 10, 4, 4, &mut scratch).unwrap();
    assert_eq!(px(&buf, 11, 11), [1, 2, 3, 255]);
}

#[test]
fn blur_box_runs_allocation_free_and_reports_small_scratch() {
    let mut buf = fb();
    {
        let mut s = Surface::new(&mut buf, W);
        fill_rect_solid(&mut s, 8, 8, 16, 16, [255, 255, 255, 255]);
    }
    let mut scratch_row = [0u8; (W * 4) as usize];
    let mut scratch_col = [0u8; (H * 4) as usize];
    let mut s = Surface::new(&mut buf, W);
    // A fitting scratch blurs successfully.
    blur_box(&mut s, 4, 4, 24, 24, 3, &mut scratch_row, &mut scratch_col).unwrap();
    // The hard edge at the fill boundary is now softened (some mid grey).
    let edge = px(&buf, 7, 16)[0];
    assert!(edge > 0 && edge < 255, "expected a blurred edge, got {edge}");

    // A too-small scratch is reported, not silently truncated.
    let mut tiny = [0u8; 4];
    let mut s2 = Surface::new(&mut buf, W);
    assert_eq!(
        blur_box(&mut s2, 0, 0, 24, 24, 3, &mut tiny, &mut scratch_col),
        Err(raster::RasterError::ScratchTooSmall)
    );
}
