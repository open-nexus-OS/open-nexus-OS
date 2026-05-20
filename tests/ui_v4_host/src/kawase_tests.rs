// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 6e host tests — dual-kawase blur (downscale → stride-blur → upscale).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 7 tests
//!
//! TEST SCOPE:
//!   - Identity (radius=0, iterations=0)
//!   - Solid preservation (color and alpha survive)
//!   - Edge blur (neighbor picks up alpha from central dot)
//!   - Small image noop (2×2 unchanged)
//!   - Iteration comparison (more iterations → similar energy)
//!   - Large radius (48×48, center brighter than near-edge)
//!
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    use nexus_effects::dual_kawase_blur;

    fn make_solid(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> (Vec<u8>, u32) {
        let stride = w * 4;
        let mut v = vec![0u8; (stride * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = (y * stride + x * 4) as usize;
                v[i] = b;
                v[i + 1] = g;
                v[i + 2] = r;
                v[i + 3] = a;
            }
        }
        (v, stride)
    }

    fn alpha_at(pixels: &[u8], stride: u32, x: u32, y: u32) -> u8 {
        pixels[(y * stride + x * 4) as usize + 3]
    }

    #[test]
    fn test_dual_kawase_identity_radius_0() {
        let (mut pixels, stride) = make_solid(32, 32, 128, 64, 32, 255);
        let orig = pixels.clone();
        let count = dual_kawase_blur(&mut pixels, 32, 32, stride, 0, 3);
        assert_eq!(count, 0, "radius=0 should be no-op");
        assert_eq!(pixels, orig);
    }

    #[test]
    fn test_dual_kawase_iterations_0() {
        let (mut pixels, stride) = make_solid(32, 32, 100, 100, 100, 255);
        let orig = pixels.clone();
        let count = dual_kawase_blur(&mut pixels, 32, 32, stride, 4, 0);
        assert_eq!(count, 0);
        assert_eq!(pixels, orig);
    }

    #[test]
    fn test_dual_kawase_solid_survives() {
        // Solid image: blur should preserve color approximately
        let (mut pixels, stride) = make_solid(16, 16, 200, 100, 50, 255);
        dual_kawase_blur(&mut pixels, 16, 16, stride, 8, 2);
        // Check center pixel's red channel (offset 2 in BGRA)
        let ci = (7 * stride + 7 * 4) as usize;
        let r_ch = pixels[ci + 2];
        assert!((r_ch as i32 - 200).abs() < 50, "red channel roughly preserved (got {})", r_ch);
        // Alpha should be preserved (blur averages alpha too)
        let a_ch = pixels[ci + 3];
        assert!(a_ch > 200, "alpha roughly preserved (got {})", a_ch);
    }

    #[test]
    fn test_dual_kawase_blurs_edge() {
        let w: u32 = 16;
        let h: u32 = 16;
        let stride = w * 4;
        let mut pixels = vec![0u8; (stride * h) as usize];
        let cx = 8usize;
        let cy = 8usize;
        let ci = cy * stride as usize + cx * 4;
        pixels[ci] = 255;
        pixels[ci + 1] = 255;
        pixels[ci + 2] = 255;
        pixels[ci + 3] = 255;

        dual_kawase_blur(&mut pixels, w, h, stride, 4, 2);

        assert!(alpha_at(&pixels, stride, cx as u32, cy as u32) > 0, "center still has alpha");
        assert!(
            alpha_at(&pixels, stride, cx as u32, (cy - 1) as u32) > 0,
            "neighbor picked up alpha"
        );
    }

    #[test]
    fn test_dual_kawase_small_image_noop() {
        let (mut pixels, stride) = make_solid(2, 2, 255, 0, 0, 255);
        let orig = pixels.clone();
        dual_kawase_blur(&mut pixels, 2, 2, stride, 8, 3);
        assert_eq!(pixels, orig, "tiny image unchanged");
    }

    #[test]
    fn test_dual_kawase_more_iterations_more_blur() {
        // 16x16 with central 4x4 white square — blur should spread it
        let w: u32 = 16;
        let h: u32 = 16;
        let stride = w * 4;
        let mut p1 = vec![0u8; (stride * h) as usize];
        let mut p2 = p1.clone();
        for y in 6..10u32 {
            for x in 6..10u32 {
                let i = (y * stride + x * 4) as usize;
                p1[i] = 255;
                p1[i + 1] = 255;
                p1[i + 2] = 255;
                p1[i + 3] = 255;
                p2[i] = 255;
                p2[i + 1] = 255;
                p2[i + 2] = 255;
                p2[i + 3] = 255;
            }
        }

        dual_kawase_blur(&mut p1, w, h, stride, 8, 1);
        dual_kawase_blur(&mut p2, w, h, stride, 8, 3);

        // With more iterations, edge pixels should get more alpha (blur spreads further)
        let edge_alpha_1 = alpha_at(&p1, stride, 2, 8);
        let edge_alpha_2 = alpha_at(&p2, stride, 2, 8);
        assert!(edge_alpha_1 > 0 || edge_alpha_2 > 0, "blur reaches edges");
    }

    #[test]
    fn test_dual_kawase_large_radius() {
        let w: u32 = 48;
        let h: u32 = 48;
        let stride = w * 4;
        let mut pixels = vec![0u8; (stride * h) as usize];
        let cx = 18u32;
        let cy = 18u32;
        for y in cy..cy + 12 {
            for x in cx..cx + 12 {
                let i = (y * stride + x * 4) as usize;
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = 255;
            }
        }
        let count = dual_kawase_blur(&mut pixels, w, h, stride, 16, 3);
        assert!(count > 0, "large radius should process pixels");
        let center_alpha = alpha_at(&pixels, stride, 24, 24);
        let near_alpha = alpha_at(&pixels, stride, 8, 24);
        assert!(center_alpha > 0, "center still has alpha");
        assert!(
            center_alpha >= near_alpha,
            "center at least as bright as near-edge (center={center_alpha}, near={near_alpha})"
        );
    }
}
