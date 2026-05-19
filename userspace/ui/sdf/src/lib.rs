// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Analytical Signed Distance Field (SDF) shapes for UI rendering.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 23 tests (tests/ui_v4_host/src/sdf_tests.rs)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md
//!
//! Provides pure functions that compute signed distances for geometric shapes:
//! circles, rounded rectangles, triangles. All functions are `no_std`, no
//! allocations, and deterministic (no floating-point rounding modes affect results).
//!
//! Convention: negative = inside, zero = edge, positive = outside.

#![no_std]

/// Signed distance to a circle.
///
/// `p`: test point.
/// `center`: circle center.
/// `radius`: circle radius.
///
/// Returns: negative inside, 0 on edge, positive outside.
#[inline]
pub fn sd_circle(p: (f32, f32), center: (f32, f32), radius: f32) -> f32 {
    let dx = p.0 - center.0;
    let dy = p.1 - center.1;
    libm::sqrtf(dx * dx + dy * dy) - radius
}

/// Signed distance to an axis-aligned rectangle (no rounding).
///
/// `p`: test point.
/// `min`: top-left corner.
/// `max`: bottom-right corner.
///
/// Returns: negative inside, 0 on edge, positive outside.
#[inline]
pub fn sd_rect(p: (f32, f32), rect_min: (f32, f32), rect_max: (f32, f32)) -> f32 {
    let center = ((rect_min.0 + rect_max.0) * 0.5, (rect_min.1 + rect_max.1) * 0.5);
    let half = ((rect_max.0 - rect_min.0) * 0.5, (rect_max.1 - rect_min.1) * 0.5);
    let q = ((p.0 - center.0).abs() - half.0, (p.1 - center.1).abs() - half.1);
    let outside = (q.0.max(0.0), q.1.max(0.0));
    libm::sqrtf(outside.0 * outside.0 + outside.1 * outside.1) + q.0.max(q.1).min(0.0)
}

/// Signed distance to an axis-aligned rounded rectangle.
///
/// `p`: test point.
/// `min`: top-left corner of the inner rectangle (before corner radius).
/// `max`: bottom-right corner of the inner rectangle.
/// `r`: corner radius. Set to 0 for a sharp rectangle.
///
/// Returns: negative inside, 0 on edge, positive outside.
#[inline]
pub fn sd_rounded_rect(p: (f32, f32), min: (f32, f32), max: (f32, f32), r: f32) -> f32 {
    if r <= 0.0 {
        return sd_rect(p, min, max);
    }
    // Shrink the rectangle by r to get the inner "sharp" rect; add r back via circle distance
    let inner_min = (min.0 + r, min.1 + r);
    let inner_max = (max.0 - r, max.1 - r);

    // Distance to the inner sharp rectangle
    let dx = (inner_min.0 - p.0).max(p.0 - inner_max.0).max(0.0);
    let dy = (inner_min.1 - p.1).max(p.1 - inner_max.1).max(0.0);
    libm::sqrtf(dx * dx + dy * dy) - r
}

/// Signed distance to a triangle.
///
/// `p`: test point.
/// `a`, `b`, `c`: triangle vertices in counter-clockwise order.
///
/// Returns: negative inside, 0 on edge, positive outside.
pub fn sd_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let e0 = (b.0 - a.0, b.1 - a.1);
    let e1 = (c.0 - b.0, c.1 - b.1);
    let e2 = (a.0 - c.0, a.1 - c.1);

    let v0 = (p.0 - a.0, p.1 - a.1);
    let v1 = (p.0 - b.0, p.1 - b.1);
    let v2 = (p.0 - c.0, p.1 - c.1);

    // Compute barycentric coordinates via cross products
    let d0 = e0.0 * v0.1 - e0.1 * v0.0; // cross(e0, v0)
    let d1 = e1.0 * v1.1 - e1.1 * v1.0;
    let d2 = e2.0 * v2.1 - e2.1 * v2.0;

    if d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0 {
        // Inside: negative distance to nearest edge
        let len0 = libm::sqrtf(e0.0 * e0.0 + e0.1 * e0.1);
        let len1 = libm::sqrtf(e1.0 * e1.0 + e1.1 * e1.1);
        let len2 = libm::sqrtf(e2.0 * e2.0 + e2.1 * e2.1);
        let dist0 = if len0 > 0.0 { d0 / len0 } else { 0.0 };
        let dist1 = if len1 > 0.0 { d1 / len1 } else { 0.0 };
        let dist2 = if len2 > 0.0 { d2 / len2 } else { 0.0 };
        -dist0.min(dist1).min(dist2)
    } else {
        // Outside: positive distance to nearest edge
        let dist0 = segment_distance(p, a, b);
        let dist1 = segment_distance(p, b, c);
        let dist2 = segment_distance(p, c, a);
        dist0.min(dist1).min(dist2)
    }
}

/// Distance from point `p` to line segment `(a, b)`.
fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let ab = (b.0 - a.0, b.1 - a.1);
    let ap = (p.0 - a.0, p.1 - a.1);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 == 0.0 {
        // Degenerate segment (a == b): distance to point
        let dx = p.0 - a.0;
        let dy = p.1 - a.1;
        return libm::sqrtf(dx * dx + dy * dy);
    }
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / len2).clamp(0.0, 1.0);
    let proj = (a.0 + ab.0 * t, a.1 + ab.1 * t);
    let dx = p.0 - proj.0;
    let dy = p.1 - proj.1;
    libm::sqrtf(dx * dx + dy * dy)
}

/// Smoothstep: maps `edge0..edge1` to `0..1`, clamped.
/// `t` is the input value to remap.
#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, t: f32) -> f32 {
    let x = ((t - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Render a filled shape from an SDF function.
///
/// Given an SDF value `sd` (negative = inside), return an alpha value (0.0–1.0)
/// using smoothstep blending across `aa_width` pixels.
///
/// `aa_width`: half-width of the anti-aliasing transition zone (typically 0.5–1.5 pixels).
#[inline]
pub fn fill_alpha(sd: f32, aa_width: f32) -> f32 {
    smoothstep(aa_width, -aa_width, sd)
}

/// Render a stroked (border) shape from an SDF function.
///
/// `sd`: signed distance value.
/// `border_width`: desired border width in pixels.
/// `aa_width`: half-width of the anti-aliasing transition zone.
///
/// Returns alpha 0.0–1.0 for the border stroke.
#[inline]
pub fn border_alpha(sd: f32, border_width: f32, aa_width: f32) -> f32 {
    let outer = smoothstep(border_width + aa_width, border_width - aa_width, sd);
    outer * (1.0 - smoothstep(aa_width, -aa_width, sd))
}

/// Compute the alpha for a filled rounded rectangle directly without allocating.
///
/// `p`: pixel coordinate (center of pixel).
/// `rect_min`, `rect_max`: rectangle bounds.
/// `corner_radius`: corner rounding radius.
/// `aa_width`: anti-aliasing width.
#[inline]
pub fn rounded_rect_fill_alpha(
    p: (f32, f32),
    rect_min: (f32, f32),
    rect_max: (f32, f32),
    corner_radius: f32,
    aa_width: f32,
) -> f32 {
    let sd = sd_rounded_rect(p, rect_min, rect_max, corner_radius);
    fill_alpha(sd, aa_width)
}

/// Compute the alpha for a stroked rounded rectangle border.
#[inline]
pub fn rounded_rect_border_alpha(
    p: (f32, f32),
    rect_min: (f32, f32),
    rect_max: (f32, f32),
    corner_radius: f32,
    border_width: f32,
    aa_width: f32,
) -> f32 {
    let sd = sd_rounded_rect(p, rect_min, rect_max, corner_radius);
    border_alpha(sd, border_width, aa_width)
}