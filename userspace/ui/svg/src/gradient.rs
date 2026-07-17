// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Resolved, device-space paint for a tessellated shape.
//!
//! The parsed [`SvgElement`](crate::elements::SvgElement) gradient defs are in
//! their *authoring* space (user space, or 0..1 bounding-box fractions). Before
//! rasterisation each shape's fill/stroke is resolved into a [`ShapePaint`] that
//! the scanline filler can evaluate **per output pixel** — either a flat colour
//! or a gradient with an `inverse` map from device pixels into the gradient's
//! coordinate space. Evaluating in gradient space (not device space) keeps
//! non-uniform `objectBoundingBox` gradients correct (the iso-lines stay aligned
//! to the box, not the screen axes).

use alloc::vec::Vec;

use crate::elements::{
    Color, GradientStop, GradientUnits, Paint, SvgDocument, SvgElement, Transform,
};

/// Axis-aligned bounding box of a shape in device space `(min_x, min_y, max_x, max_y)`.
pub type BBox = (f32, f32, f32, f32);

/// Resolved paint for one tessellated shape.
#[derive(Debug, Clone)]
pub enum ShapePaint {
    Solid(Color),
    Gradient(Gradient),
}

/// A device-space-evaluable gradient.
#[derive(Debug, Clone)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Maps a device-space pixel into the gradient's coordinate space, where
    /// `kind`'s geometry (`p0/p1` or `center/radius`) is expressed.
    pub inverse: Transform,
    /// Stops sorted by offset, with element opacity pre-multiplied into alpha.
    pub stops: Vec<GradientStop>,
}

#[derive(Debug, Clone, Copy)]
pub enum GradientKind {
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    Radial { cx: f32, cy: f32, r: f32, fx: f32, fy: f32 },
}

impl ShapePaint {
    /// Colour at device pixel centre `(px, py)`.
    #[inline]
    pub fn color_at(&self, px: f32, py: f32) -> Color {
        match self {
            ShapePaint::Solid(c) => *c,
            ShapePaint::Gradient(g) => g.color_at(px, py),
        }
    }

    /// A solid paint whose alpha is zero contributes nothing — lets the
    /// rasteriser skip a whole shape cheaply. Gradients are never skipped here
    /// (individual stops may still be transparent).
    #[inline]
    pub fn is_fully_transparent(&self) -> bool {
        matches!(self, ShapePaint::Solid(c) if c.a == 0)
    }
}

impl Gradient {
    #[inline]
    pub fn color_at(&self, px: f32, py: f32) -> Color {
        // Pixel centre → gradient space.
        let (gx, gy) = self.inverse.apply(px, py);
        let t = match self.kind {
            GradientKind::Linear { x0, y0, x1, y1 } => {
                let dx = x1 - x0;
                let dy = y1 - y0;
                let len2 = dx * dx + dy * dy;
                if len2 <= 1e-12 {
                    0.0
                } else {
                    ((gx - x0) * dx + (gy - y0) * dy) / len2
                }
            }
            GradientKind::Radial { cx, cy, r, fx, fy } => radial_t(gx, gy, cx, cy, r, fx, fy),
        };
        sample_stops(&self.stops, t)
    }
}

/// Gradient parameter for a radial gradient at gradient-space point `(gx, gy)`.
///
/// With a focal point `(fx, fy)` inside the circle, `t` is the fraction along the
/// ray from the focus through the point to where it meets the circle (SVG's
/// focal model). When the focus is at the centre this reduces to `dist / r`.
fn radial_t(gx: f32, gy: f32, cx: f32, cy: f32, r: f32, fx: f32, fy: f32) -> f32 {
    use crate::math::F32Math;
    if r <= 1e-6 {
        return 1.0;
    }
    let dx = gx - fx;
    let dy = gy - fy;
    // Vector from focus to centre.
    let fcx = fx - cx;
    let fcy = fy - cy;
    // Solve |f + s·d - c| = r for s >= 0; t = 1/s (point at s=1).
    // |d|² s² + 2 (d·fc) s + (|fc|² − r²) = 0.
    let a = dx * dx + dy * dy;
    if a <= 1e-12 {
        return 0.0;
    }
    let b = 2.0 * (dx * fcx + dy * fcy);
    let c = fcx * fcx + fcy * fcy - r * r;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return 1.0;
    }
    let s = (-b + disc.nexus_sqrt()) / (2.0 * a);
    if s <= 1e-6 {
        1.0
    } else {
        (1.0 / s).clamp(0.0, 1.0)
    }
}

/// Sample a stop list at `t`, with pad (clamp) spread and linear RGBA
/// interpolation between adjacent stops. Stops are assumed sorted by offset.
pub fn sample_stops(stops: &[GradientStop], t: f32) -> Color {
    match stops {
        [] => Color { r: 0, g: 0, b: 0, a: 0 },
        [only] => only.color,
        _ => {
            let t = t.clamp(0.0, 1.0);
            let first = &stops[0];
            let last = &stops[stops.len() - 1];
            if t <= first.offset {
                return first.color;
            }
            if t >= last.offset {
                return last.color;
            }
            for w in 1..stops.len() {
                let b = &stops[w];
                if t <= b.offset {
                    let a = &stops[w - 1];
                    let span = (b.offset - a.offset).max(1e-6);
                    let f = ((t - a.offset) / span).clamp(0.0, 1.0);
                    return lerp_color(a.color, b.color, f);
                }
            }
            last.color
        }
    }
}

#[inline]
fn lerp_color(a: Color, b: Color, f: f32) -> Color {
    use crate::math::F32Math;
    let l =
        |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).nexus_round().clamp(0.0, 255.0) as u8;
    Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }
}

/// Build a normalised, sorted stop list with element opacity folded into alpha.
fn prepare_stops(src: &[GradientStop], opacity: f32) -> Vec<GradientStop> {
    let mut out: Vec<GradientStop> = src
        .iter()
        .map(|s| GradientStop {
            offset: s.offset.clamp(0.0, 1.0),
            color: Color { a: (s.color.a as f32 * opacity.clamp(0.0, 1.0)) as u8, ..s.color },
        })
        .collect();
    out.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(core::cmp::Ordering::Equal));
    // Enforce monotonic non-decreasing offsets (SVG: a stop never precedes a prior one).
    for i in 1..out.len() {
        if out[i].offset < out[i - 1].offset {
            out[i].offset = out[i - 1].offset;
        }
    }
    out
}

/// Resolve a `fill`/`stroke` paint into a device-space [`ShapePaint`].
///
/// `tf` is the element's user→device transform; `opacity` the inherited opacity;
/// `bbox` the shape's device-space bounding box (for `objectBoundingBox` units).
/// Returns `None` for `Paint::None` or an unresolvable/empty gradient.
pub fn resolve_shape_paint(
    paint: &Paint,
    doc: &SvgDocument,
    tf: &Transform,
    opacity: f32,
    bbox: BBox,
) -> Option<ShapePaint> {
    match paint {
        Paint::None => None,
        Paint::Color(c) => {
            Some(ShapePaint::Solid(Color { a: (c.a as f32 * opacity.clamp(0.0, 1.0)) as u8, ..*c }))
        }
        Paint::GradientRef(id) => match doc.defs.get(id)? {
            SvgElement::LinearGradient { x1, y1, x2, y2, stops, units, .. } => {
                if stops.is_empty() {
                    return None;
                }
                let inverse = unit_inverse(*units, tf, bbox)?;
                Some(ShapePaint::Gradient(Gradient {
                    kind: GradientKind::Linear { x0: *x1, y0: *y1, x1: *x2, y1: *y2 },
                    inverse,
                    stops: prepare_stops(stops, opacity),
                }))
            }
            SvgElement::RadialGradient { cx, cy, r, fx, fy, stops, units, .. } => {
                if stops.is_empty() {
                    return None;
                }
                let inverse = unit_inverse(*units, tf, bbox)?;
                Some(ShapePaint::Gradient(Gradient {
                    kind: GradientKind::Radial { cx: *cx, cy: *cy, r: *r, fx: *fx, fy: *fy },
                    inverse,
                    stops: prepare_stops(stops, opacity),
                }))
            }
            _ => None,
        },
    }
}

/// Device→gradient-space map for the given units.
///
/// `userSpaceOnUse`: gradient geometry is in user space, so we invert the element
/// transform. `objectBoundingBox`: geometry is 0..1 over the device bbox, so the
/// map normalises device pixels into that unit box (and collapses if degenerate).
fn unit_inverse(units: GradientUnits, tf: &Transform, bbox: BBox) -> Option<Transform> {
    match units {
        GradientUnits::UserSpaceOnUse => tf.inverse(),
        GradientUnits::ObjectBoundingBox => {
            let (min_x, min_y, max_x, max_y) = bbox;
            let w = max_x - min_x;
            let h = max_y - min_y;
            if w <= 1e-6 || h <= 1e-6 {
                return None;
            }
            // gx = (px - min_x) / w, gy = (py - min_y) / h
            Some(Transform { a: 1.0 / w, b: 0.0, c: 0.0, d: 1.0 / h, e: -min_x / w, f: -min_y / h })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Transform;

    fn stop(offset: f32, r: u8, g: u8, b: u8) -> GradientStop {
        GradientStop { offset, color: Color { r, g, b, a: 255 } }
    }

    #[test]
    fn sample_clamps_and_interpolates() {
        let stops = [stop(0.0, 0, 0, 0), stop(1.0, 255, 255, 255)];
        assert_eq!(sample_stops(&stops, -1.0), stops[0].color);
        assert_eq!(sample_stops(&stops, 2.0), stops[1].color);
        let mid = sample_stops(&stops, 0.5);
        assert!((mid.r as i32 - 128).abs() <= 1, "midpoint grey, got {}", mid.r);
    }

    #[test]
    fn linear_userspace_is_horizontal_ramp() {
        // 0..100 black→white, identity transform (device == user).
        let g = Gradient {
            kind: GradientKind::Linear { x0: 0.0, y0: 0.0, x1: 100.0, y1: 0.0 },
            inverse: Transform::IDENTITY,
            stops: alloc::vec![stop(0.0, 0, 0, 0), stop(1.0, 255, 255, 255)],
        };
        assert_eq!(g.color_at(0.0, 0.0).r, 0);
        assert_eq!(g.color_at(100.0, 50.0).r, 255);
        // y is irrelevant for a horizontal axis.
        assert_eq!(g.color_at(50.0, 0.0).r, g.color_at(50.0, 999.0).r);
        assert!((g.color_at(50.0, 0.0).r as i32 - 128).abs() <= 2);
    }

    #[test]
    fn objectbbox_normalises_to_the_shape_box() {
        // Box at device (200..400, 100..300): a 0..1 horizontal gradient should
        // be black at the left box edge and white at the right, regardless of
        // where the box sits on screen.
        let bbox = (200.0, 100.0, 400.0, 300.0);
        let inverse =
            unit_inverse(GradientUnits::ObjectBoundingBox, &Transform::IDENTITY, bbox).unwrap();
        let g = Gradient {
            kind: GradientKind::Linear { x0: 0.0, y0: 0.0, x1: 1.0, y1: 0.0 },
            inverse,
            stops: alloc::vec![stop(0.0, 0, 0, 0), stop(1.0, 255, 255, 255)],
        };
        assert_eq!(g.color_at(200.0, 200.0).r, 0, "left edge black");
        assert_eq!(g.color_at(400.0, 200.0).r, 255, "right edge white");
        assert!((g.color_at(300.0, 200.0).r as i32 - 128).abs() <= 2, "centre grey");
    }

    #[test]
    fn radial_centre_to_edge_ramp() {
        // Unit circle radius 50 centred at origin, focus at centre.
        let g = Gradient {
            kind: GradientKind::Radial { cx: 0.0, cy: 0.0, r: 50.0, fx: 0.0, fy: 0.0 },
            inverse: Transform::IDENTITY,
            stops: alloc::vec![stop(0.0, 255, 255, 255), stop(1.0, 0, 0, 0)],
        };
        assert_eq!(g.color_at(0.0, 0.0).r, 255, "centre = first stop");
        assert_eq!(g.color_at(50.0, 0.0).r, 0, "edge = last stop");
        assert_eq!(g.color_at(99.0, 99.0).r, 0, "outside clamps to last stop");
        assert!((g.color_at(25.0, 0.0).r as i32 - 128).abs() <= 3, "halfway grey");
    }

    #[test]
    fn opacity_folds_into_stop_alpha() {
        let src = [stop(0.0, 10, 20, 30), stop(1.0, 40, 50, 60)];
        let out = prepare_stops(&src, 0.5);
        assert_eq!(out[0].color.a, 127);
        assert_eq!(out[1].color.a, 127);
    }

    #[test]
    fn unsorted_stops_are_sorted() {
        let src = [stop(1.0, 0, 0, 0), stop(0.0, 255, 255, 255)];
        let out = prepare_stops(&src, 1.0);
        assert!(out[0].offset <= out[1].offset);
        assert_eq!(out[0].color.r, 255, "offset 0 stop comes first after sort");
    }
}
