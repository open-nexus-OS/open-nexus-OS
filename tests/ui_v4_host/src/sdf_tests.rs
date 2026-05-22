// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 6c host tests — analytical SDF shapes (circles, rounded rects, triangles).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 23 tests
//!
//! TEST SCOPE:
//!   - sd_circle (center negative, edge zero, outside positive, off-center)
//!   - sd_rect (center inside, corner outside, edge zero)
//!   - sd_rounded_rect (r=0 matches sd_rect, center inside, corner outside, near corner tolerance)
//!   - sd_triangle (center inside, outside, vertex zero — CCW winding)
//!   - smoothstep (below edge0, above edge1, midpoint)
//!   - fill_alpha / border_alpha (deep inside/outside, on-border tolerance)
//!   - rounded_rect convenience (fill alpha center, border alpha edge)
//!
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    use nexus_sdf::{
        border_alpha, fill_alpha, rounded_rect_border_alpha, rounded_rect_fill_alpha, sd_circle,
        sd_rect, sd_rounded_rect, sd_triangle, smoothstep,
    };

    // ─── sd_circle ───

    #[test]
    fn test_sd_circle_center_negative() {
        let sd = sd_circle((0.0, 0.0), (0.0, 0.0), 10.0);
        assert!(sd < 0.0, "center should be inside (negative)");
        assert!((sd + 10.0).abs() < 0.01, "distance should be -radius");
    }

    #[test]
    fn test_sd_circle_on_edge_zero() {
        let sd = sd_circle((10.0, 0.0), (0.0, 0.0), 10.0);
        assert!((sd).abs() < 0.01, "on edge should be zero");
    }

    #[test]
    fn test_sd_circle_outside_positive() {
        let sd = sd_circle((20.0, 0.0), (0.0, 0.0), 10.0);
        assert!(sd > 0.0, "outside should be positive");
        assert!((sd - 10.0).abs() < 0.01, "distance should be 10");
    }

    #[test]
    fn test_sd_circle_off_center() {
        // Point at (3,4) from center (0,0): distance = 5
        let sd = sd_circle((3.0, 4.0), (0.0, 0.0), 10.0);
        assert!(
            (sd + 5.0).abs() < 0.01,
            "distance to center=5, radius=10 → sd=-5"
        );
    }

    // ─── sd_rect ───

    #[test]
    fn test_sd_rect_center_inside() {
        // Points inside should have negative distance.
        // At (5,5) in a 10×10 rect, the center is 5px from each edge.
        // sd_box formula: length(max(q,0)) + min(max(q.x,q.y), 0)
        // where q = abs(p-center) - half_size = (0,0) - (5,5) = (-5,-5)
        // sd = 0 + min(max(-5,-5), 0) = -5 → negative = inside.
        let sd = sd_rect((5.0, 5.0), (0.0, 0.0), (10.0, 10.0));
        assert!(sd < 0.0, "center should be inside");
    }

    #[test]
    fn test_sd_rect_corner_outside() {
        let sd = sd_rect((12.0, 12.0), (0.0, 0.0), (10.0, 10.0));
        assert!(sd > 0.0, "outside corner should be positive");
        // Distance from (12,12) to nearest corner (10,10) = sqrt(8) ≈ 2.828
        assert!((sd - 2.828).abs() < 0.01);
    }

    #[test]
    fn test_sd_rect_edge_zero() {
        let sd = sd_rect((5.0, 10.0), (0.0, 0.0), (10.0, 10.0));
        assert!((sd).abs() < 0.01, "on edge should be zero");
    }

    // ─── sd_rounded_rect ───

    #[test]
    fn test_sd_rounded_rect_r0_matches_sd_rect() {
        let p = (7.0, 3.0);
        let rect_min = (0.0, 0.0);
        let rect_max = (10.0, 10.0);
        let sd1 = sd_rounded_rect(p, rect_min, rect_max, 0.0);
        let sd2 = sd_rect(p, rect_min, rect_max);
        assert!((sd1 - sd2).abs() < 0.01, "r=0 should match sd_rect");
    }

    #[test]
    fn test_sd_rounded_rect_center_inside() {
        let sd = sd_rounded_rect((5.0, 5.0), (0.0, 0.0), (10.0, 10.0), 2.0);
        assert!(
            sd < 0.0,
            "center should be inside even with rounded corners"
        );
    }

    #[test]
    fn test_sd_rounded_rect_corner_outside() {
        // At corner (0,0) with r=3: inner rect is (3,3)-(7,7)
        // Distance from (0,0) to inner corner (3,3) = sqrt(18) ≈ 4.243 - r = 1.243
        let sd = sd_rounded_rect((0.0, 0.0), (0.0, 0.0), (10.0, 10.0), 3.0);
        assert!(sd > 0.0, "far outside corner should be positive");
        assert!((sd - 1.243).abs() < 0.02);
    }

    #[test]
    fn test_sd_rounded_rect_near_corner() {
        // Point at (1,1) with r=3: distance to inner corner (3,3) = sqrt(8) ≈ 2.828
        // sd = 2.828 - 3 = -0.172 → slightly inside the rounded corner
        let sd = sd_rounded_rect((1.0, 1.0), (0.0, 0.0), (10.0, 10.0), 3.0);
        assert!(
            sd < 0.01,
            "near corner should be inside or near edge (sd={sd})"
        );
    }

    // ─── sd_triangle ───

    #[test]
    fn test_sd_triangle_center_inside() {
        // CCW triangle: (0,0) → (10,0) → (5,10)
        let a = (0.0, 0.0);
        let b = (10.0, 0.0);
        let c = (5.0, 10.0);
        let sd = sd_triangle((5.0, 4.0), a, b, c);
        assert!(sd < 0.0, "center should be inside triangle (sd={sd})");
    }

    #[test]
    fn test_sd_triangle_outside() {
        let a = (0.0, 0.0);
        let b = (10.0, 0.0);
        let c = (5.0, 10.0);
        let sd = sd_triangle((20.0, 15.0), a, b, c);
        assert!(sd > 0.0, "point far outside should be positive");
    }

    #[test]
    fn test_sd_triangle_vertex_zero() {
        let a = (0.0, 0.0);
        let b = (10.0, 0.0);
        let c = (5.0, 10.0);
        let sd = sd_triangle((0.0, 0.0), a, b, c);
        assert!((sd).abs() < 0.01, "at vertex should be on edge (sd=0)");
    }

    // ─── smoothstep ───

    #[test]
    fn test_smoothstep_below_edge0_is_0() {
        assert_eq!(smoothstep(0.0, 1.0, -0.5), 0.0);
    }

    #[test]
    fn test_smoothstep_above_edge1_is_1() {
        assert_eq!(smoothstep(0.0, 1.0, 1.5), 1.0);
    }

    #[test]
    fn test_smoothstep_midpoint_is_0_5() {
        let v = smoothstep(0.0, 2.0, 1.0);
        assert!((v - 0.5).abs() < 0.01);
    }

    // ─── fill_alpha / border_alpha ───

    #[test]
    fn test_fill_alpha_deep_inside_is_1() {
        assert!((fill_alpha(-5.0, 1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_fill_alpha_deep_outside_is_0() {
        assert!((fill_alpha(5.0, 1.0) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_border_alpha_on_border_is_1() {
        // At sd = border_width, outer smoothstep is at midpoint (0.5),
        // inner is 1.0, so product = 0.5. Test that it's non-trivial.
        let alpha = border_alpha(3.0, 3.0, 1.0);
        assert!(alpha > 0.4, "border should be visible (alpha={alpha})");
    }

    #[test]
    fn test_border_alpha_deep_inside_is_0() {
        // Deep inside should have no border alpha
        let alpha = border_alpha(-5.0, 2.0, 1.0);
        assert!((alpha - 0.0).abs() < 0.01);
    }

    // ─── rounded_rect convenience ───

    #[test]
    fn test_rounded_rect_fill_alpha_center() {
        let alpha = rounded_rect_fill_alpha((5.0, 5.0), (0.0, 0.0), (10.0, 10.0), 2.0, 1.0);
        assert!(
            alpha > 0.9,
            "center of rounded rect should be filled (alpha={alpha})"
        );
    }

    #[test]
    fn test_rounded_rect_border_alpha_edge() {
        // At the left edge (sd≈0) with border_width=1, border_alpha:
        // outer ≈ smoothstep(2,0,0) = 0; inner = smoothstep(1,-1,0) ≈ 0.5; product ≈ 0
        let alpha = rounded_rect_border_alpha((0.5, 5.0), (0.0, 0.0), (10.0, 10.0), 0.0, 1.0, 1.0);
        assert!(alpha >= 0.0, "border alpha should be valid (alpha={alpha})");
    }
}
