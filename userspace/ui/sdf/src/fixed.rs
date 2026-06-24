// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Fixed-point (no-FPU) implementation of the SDF coverage math — the
//! integer sibling of this crate's float analytic SDF, for the riscv OS hot path
//! where the float path is undesirable. Same definitions (rounded-rect / circle
//! distance, AA fill / border / shadow coverage), evaluated in 8.8 fixed-point.
//! This is the production anti-aliased rasterization math (alpha edges via a
//! smoothstep band), parity-tested against the float reference in this crate.
//! `nexus-sdf` is the single SDF math SSOT (RFC-0067 P5); the nexus-gfx CPU
//! executor and the gpud GPU shaders both derive from these definitions.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 tests (alpha parity vs this crate's float reference)
//!
//! Moved out of windowd (`fixed_sdf.rs`, via nexus-gfx) so the rasterization math
//! lives with its analytic definition, not in the compositor service. Pure
//! integer math — no allocation, no FPU — identical on host and the riscv target.

const FIXED_SHIFT: i32 = 8;
const FIXED_ONE: i32 = 1 << FIXED_SHIFT;
const FIXED_HALF: i32 = FIXED_ONE / 2;

/// Convert a `u32` pixel coordinate to the fixed-point domain.
pub fn px_u32(value: u32) -> i32 {
    (value.min(i32::MAX as u32) as i32) << FIXED_SHIFT
}

/// Convert an `i32` pixel coordinate to the fixed-point domain (saturating).
pub fn px_i32(value: i32) -> i32 {
    value.saturating_mul(FIXED_ONE)
}

/// Fixed-point coordinate of a pixel's center (`value + 0.5`).
pub fn pixel_center(value: u32) -> i32 {
    px_u32(value).saturating_add(FIXED_HALF)
}

fn isqrt_u64(value: u64) -> u32 {
    if value == 0 {
        return 0;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x.min(u64::from(u32::MAX)) as u32
}

/// Signed distance (fixed-point) from `(point_x, point_y)` to a rounded rect.
#[allow(clippy::too_many_arguments)]
pub fn rounded_rect_sd(
    point_x: i32,
    point_y: i32,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    radius: i32,
) -> i32 {
    let radius = radius.max(0);
    let inner_min_x = min_x.saturating_add(radius);
    let inner_min_y = min_y.saturating_add(radius);
    let inner_max_x = max_x.saturating_sub(radius);
    let inner_max_y = max_y.saturating_sub(radius);
    let dx = (inner_min_x.saturating_sub(point_x)).max(point_x.saturating_sub(inner_max_x)).max(0);
    let dy = (inner_min_y.saturating_sub(point_y)).max(point_y.saturating_sub(inner_max_y)).max(0);
    let dist =
        isqrt_u64((dx as u64).saturating_mul(dx as u64) + (dy as u64).saturating_mul(dy as u64));
    (dist as i32).saturating_sub(radius)
}

/// Fixed-point circle SDF: distance from (px, py) to center (cx, cy) minus radius.
#[allow(dead_code)]
pub fn circle_sd(px: i32, py: i32, cx: i32, cy: i32, radius: i32) -> i32 {
    let dx = px.saturating_sub(cx);
    let dy = py.saturating_sub(cy);
    let dist_sq = (dx as i64).saturating_mul(dx as i64) + (dy as i64).saturating_mul(dy as i64);
    (isqrt_u64(dist_sq as u64) as i32).saturating_sub(radius)
}

fn smoothstep_unit(x: i32) -> i32 {
    let x = x.clamp(0, FIXED_ONE);
    let x2 = x.saturating_mul(x);
    let term = 3 * FIXED_ONE - 2 * x;
    x2.saturating_mul(term) >> (FIXED_SHIFT * 2)
}

fn smoothstep(edge0: i32, edge1: i32, value: i32) -> i32 {
    let denom = edge1.saturating_sub(edge0);
    if denom == 0 {
        return if value >= edge1 { FIXED_ONE } else { 0 };
    }
    let x = value.saturating_sub(edge0).saturating_mul(FIXED_ONE) / denom;
    smoothstep_unit(x)
}

/// Anti-aliased fill coverage (0..255) for a signed distance — alpha edges via a
/// 1px smoothstep band (the production AA, not a binary inside/outside test).
pub fn fill_alpha(sd: i32) -> u32 {
    let alpha = smoothstep(FIXED_ONE, -FIXED_ONE, sd);
    (alpha as u32 * 255) / FIXED_ONE as u32
}

/// Anti-aliased border coverage (0..255) at `stroke_width` around the edge.
pub fn border_alpha(sd: i32, stroke_width: u32) -> u32 {
    let stroke = px_u32(stroke_width);
    let outer = smoothstep(stroke.saturating_add(FIXED_ONE), stroke.saturating_sub(FIXED_ONE), sd);
    let inner = smoothstep(FIXED_ONE, -FIXED_ONE, sd);
    let alpha = outer.saturating_mul(FIXED_ONE.saturating_sub(inner)) / FIXED_ONE;
    (alpha as u32 * 255) / FIXED_ONE as u32
}

/// Soft-shadow coverage (0..`max_alpha`) at `distance` from the edge of a shape,
/// falling off quadratically over `blur_radius`.
pub fn shadow_alpha_from_distance(distance: i32, blur_radius: u32, max_alpha: u32) -> u32 {
    let blur = px_u32(blur_radius).max(1);
    if distance >= blur {
        return 0;
    }
    let t = blur.saturating_sub(distance.max(0)).saturating_mul(FIXED_ONE) / blur;
    ((t as u32).saturating_mul(t as u32).saturating_mul(max_alpha))
        / ((FIXED_ONE as u32).saturating_mul(FIXED_ONE as u32))
}

#[cfg(test)]
mod tests {
    use super::{
        border_alpha, fill_alpha, pixel_center, px_i32, px_u32, rounded_rect_sd,
        shadow_alpha_from_distance,
    };

    #[test]
    fn rounded_rect_fill_alpha_tracks_float_sdf() {
        let rect = (56u32, 440u32, 826u32, 260u32);
        let radius = 12u32;
        let min = (rect.0 as f32, rect.1 as f32);
        let max = (rect.0.saturating_add(rect.2) as f32, rect.1.saturating_add(rect.3) as f32);
        let min_x = px_u32(rect.0);
        let min_y = px_u32(rect.1);
        let max_x = px_u32(rect.0.saturating_add(rect.2));
        let max_y = px_u32(rect.1.saturating_add(rect.3));
        let radius_fx = px_u32(radius);
        for (x, y) in [(56, 440), (60, 440), (62, 444), (68, 452), (100, 440), (55, 439)] {
            // Float reference lives in this same crate (the SDF math SSOT).
            let sd =
                crate::sd_rounded_rect((x as f32 + 0.5, y as f32 + 0.5), min, max, radius as f32);
            let expected = (crate::fill_alpha(sd, 1.0) * 255.0) as i32;
            let fixed_sd = rounded_rect_sd(
                pixel_center(x),
                pixel_center(y),
                min_x,
                min_y,
                max_x,
                max_y,
                radius_fx,
            );
            let actual = fill_alpha(fixed_sd) as i32;
            assert!(
                (expected - actual).abs() <= 3,
                "sample ({x},{y}) expected {expected} actual {actual}"
            );
        }
    }

    #[test]
    fn shadow_alpha_tracks_float_curve() {
        assert_eq!(px_i32(-2), -512);
        let blur = 30;
        let max_alpha = 128;
        for distance in [0, 1, 8, 15, 29, 30] {
            let fixed = shadow_alpha_from_distance(px_u32(distance), blur, max_alpha) as i32;
            let t = 1.0 - distance as f32 / blur as f32;
            let expected = if distance >= blur { 0 } else { (t * t * max_alpha as f32) as i32 };
            assert!(
                (expected - fixed).abs() <= 1,
                "distance {distance} expected {expected} actual {fixed}"
            );
        }
    }

    #[test]
    fn border_alpha_is_nonzero_only_near_edge_band() {
        assert_eq!(border_alpha(px_u32(4), 1), 0);
        assert_eq!(border_alpha(-px_u32(4), 1), 0);
        assert!(border_alpha(0, 1) > 0);
    }
}
