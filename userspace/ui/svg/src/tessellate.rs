// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

use core::f32::consts;

use crate::elements::{Color, Paint, PathCommand, PathData, SvgDocument, SvgElement, Transform};
use crate::math::F32Math;

/// A single line segment in screen space (y-sorted: y0 <= y1).
#[derive(Debug, Clone, Copy)]
pub struct Edge {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub color: Color,
    pub shape_id: u32,
}

/// Convert an `SvgDocument` into a flat list of edges for scanline rendering.
pub fn tessellate_document(doc: &SvgDocument) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut next_shape_id = 0;
    let parent_transform = Transform::IDENTITY;
    let parent_opacity = 1.0;

    for elem in &doc.elements {
        tessellate_element(
            elem,
            &parent_transform,
            parent_opacity,
            &mut edges,
            &mut next_shape_id,
            doc,
        );
    }

    edges
}

fn tessellate_element(
    elem: &SvgElement,
    parent_tf: &Transform,
    parent_opacity: f32,
    edges: &mut Vec<Edge>,
    next_shape_id: &mut u32,
    doc: &SvgDocument,
) {
    match elem {
        SvgElement::Group { children, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);
            for child in children {
                tessellate_element(child, &tf, op, edges, next_shape_id, doc);
            }
        }
        SvgElement::Path { data, fill, stroke, stroke_width, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = path_to_segments(data, &tf);
            if let Some(paint) = fill {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, polygon_edges(&segments, c));
                }
            }
            if let Some(paint) = stroke {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, stroke_edges(&segments, *stroke_width, c));
                }
            }
        }
        SvgElement::Rect {
            x,
            y,
            width,
            height,
            rx,
            ry,
            fill,
            stroke,
            stroke_width,
            transform,
            opacity,
        } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = rect_segments(*x, *y, *width, *height, *rx, *ry, &tf);
            if let Some(paint) = fill {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, polygon_edges(&segments, c));
                }
            }
            if let Some(paint) = stroke {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, stroke_edges(&segments, *stroke_width, c));
                }
            }
        }
        SvgElement::Circle { cx, cy, r, fill, stroke, stroke_width, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = circle_segments(*cx, *cy, *r, &tf);
            if let Some(paint) = fill {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, polygon_edges(&segments, c));
                }
            }
            if let Some(paint) = stroke {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, stroke_edges(&segments, *stroke_width, c));
                }
            }
        }
        SvgElement::Ellipse { cx, cy, rx, ry, fill, stroke, stroke_width, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = ellipse_segments(*cx, *cy, *rx, *ry, &tf);
            if let Some(paint) = fill {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, polygon_edges(&segments, c));
                }
            }
            if let Some(paint) = stroke {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, stroke_edges(&segments, *stroke_width, c));
                }
            }
        }
        SvgElement::Line { x1, y1, x2, y2, stroke, stroke_width, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            if let Some(paint) = stroke {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    let (sx, sy) = tf.apply(*x1, *y1);
                    let (ex, ey) = tf.apply(*x2, *y2);
                    let half = stroke_width.max(0.5) / 2.0;
                    // Approximate line as thin rectangle
                    let dx = ex - sx;
                    let dy = ey - sy;
                    let len = (dx * dx + dy * dy).nexus_sqrt().max(0.001);
                    let nx = -dy / len * half;
                    let ny = dx / len * half;
                    let pts = vec![
                        (sx + nx, sy + ny),
                        (ex + nx, ey + ny),
                        (ex - nx, ey - ny),
                        (sx - nx, sy - ny),
                    ];
                    append_shape(edges, next_shape_id, polygon_edges(&pts, c));
                }
            }
        }
        SvgElement::Polygon { points, fill, stroke, stroke_width, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let pts: Vec<(f32, f32)> = points.iter().map(|(x, y)| tf.apply(*x, *y)).collect();
            if let Some(paint) = fill {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, polygon_edges(&pts, c));
                }
            }
            if let Some(paint) = stroke {
                if let Some(color) = resolve_paint(paint, doc) {
                    let c = blend_opacity(color, op);
                    append_shape(edges, next_shape_id, stroke_edges(&pts, *stroke_width, c));
                }
            }
        }
        // Defs entries are not rendered directly
        SvgElement::LinearGradient { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Transform composition
// ---------------------------------------------------------------------------

fn combine_transform(parent: &Transform, child: &Option<Transform>) -> Transform {
    match child {
        Some(c) => parent.compose(c),
        None => *parent,
    }
}

// ---------------------------------------------------------------------------
// Paint resolution
// ---------------------------------------------------------------------------

fn resolve_paint(paint: &Paint, doc: &SvgDocument) -> Option<Color> {
    match paint {
        Paint::Color(c) => Some(*c),
        Paint::GradientRef(id) => doc.defs.get(id).and_then(|elem| match elem {
            SvgElement::LinearGradient { stops, .. } => {
                // Use midpoint color of gradient
                if stops.is_empty() {
                    return Some(Color::BLACK);
                }
                // Find the stop at 0.5 or the middle stop
                let mid = stops
                    .iter()
                    .min_by(|a, b| {
                        (a.offset - 0.5).abs().partial_cmp(&(b.offset - 0.5).abs()).unwrap()
                    })
                    .unwrap();
                Some(mid.color)
            }
            _ => None,
        }),
        Paint::None => None,
    }
}

fn blend_opacity(color: Color, opacity: f32) -> Color {
    Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: (color.a as f32 * opacity.clamp(0.0, 1.0)) as u8,
    }
}

// ---------------------------------------------------------------------------
// Shape segment generators
// ---------------------------------------------------------------------------

fn path_to_segments(data: &PathData, tf: &Transform) -> Vec<(f32, f32)> {
    let mut points: Vec<(f32, f32)> = Vec::new();
    let mut cx: f32 = 0.0;
    let mut cy: f32 = 0.0;
    let mut start: Option<(f32, f32)> = None;

    // Track previous control point for smooth curves
    let mut prev_cx2: f32 = 0.0;
    let mut prev_cy2: f32 = 0.0;
    let mut prev_qx: f32 = 0.0;
    let mut prev_qy: f32 = 0.0;

    for cmd in &data.commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                if let Some(_s) = start.take() {
                    // Close previous subpath implicitly
                    if points.len() >= 2 {
                        // Don't close here unless Z was explicitly given
                    }
                }
                let (px, py) = tf.apply(*x, *y);
                points.push((px, py));
                cx = *x;
                cy = *y;
                start = Some((*x, *y));
            }
            PathCommand::MoveToRel { dx, dy } => {
                let nx = cx + dx;
                let ny = cy + dy;
                let (px, py) = tf.apply(nx, ny);
                points.push((px, py));
                cx = nx;
                cy = ny;
                start = Some((nx, ny));
            }
            PathCommand::LineTo { x, y } => {
                let (px, py) = tf.apply(*x, *y);
                points.push((px, py));
                cx = *x;
                cy = *y;
            }
            PathCommand::LineToRel { dx, dy } => {
                let nx = cx + dx;
                let ny = cy + dy;
                let (px, py) = tf.apply(nx, ny);
                points.push((px, py));
                cx = nx;
                cy = ny;
            }
            PathCommand::HorizontalTo { x } => {
                let (px, py) = tf.apply(*x, cy);
                points.push((px, py));
                cx = *x;
            }
            PathCommand::HorizontalToRel { dx } => {
                let nx = cx + dx;
                let (px, py) = tf.apply(nx, cy);
                points.push((px, py));
                cx = nx;
            }
            PathCommand::VerticalTo { y } => {
                let (px, py) = tf.apply(cx, *y);
                points.push((px, py));
                cy = *y;
            }
            PathCommand::VerticalToRel { dy } => {
                let ny = cy + dy;
                let (px, py) = tf.apply(cx, ny);
                points.push((px, py));
                cy = ny;
            }
            PathCommand::CubicTo { x1, y1, x2, y2, x, y } => {
                prev_cx2 = *x2;
                prev_cy2 = *y2;
                let pts = cubic_bezier_segments(cx, cy, *x1, *y1, *x2, *y2, *x, *y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = *x;
                cy = *y;
            }
            PathCommand::CubicToRel { dx1, dy1, dx2, dy2, dx, dy } => {
                let x1 = cx + dx1;
                let y1 = cy + dy1;
                let x2 = cx + dx2;
                let y2 = cy + dy2;
                let x = cx + dx;
                let y = cy + dy;
                prev_cx2 = x2;
                prev_cy2 = y2;
                let pts = cubic_bezier_segments(cx, cy, x1, y1, x2, y2, x, y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::SmoothCubicTo { x2, y2, x, y } => {
                let x1 = 2.0 * cx - prev_cx2;
                let y1 = 2.0 * cy - prev_cy2;
                prev_cx2 = *x2;
                prev_cy2 = *y2;
                let pts = cubic_bezier_segments(cx, cy, x1, y1, *x2, *y2, *x, *y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = *x;
                cy = *y;
            }
            PathCommand::SmoothCubicToRel { dx2, dy2, dx, dy } => {
                let x1 = 2.0 * cx - prev_cx2;
                let y1 = 2.0 * cy - prev_cy2;
                let x2 = cx + dx2;
                let y2 = cy + dy2;
                let x = cx + dx;
                let y = cy + dy;
                prev_cx2 = x2;
                prev_cy2 = y2;
                let pts = cubic_bezier_segments(cx, cy, x1, y1, x2, y2, x, y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::QuadraticTo { x1, y1, x, y } => {
                prev_qx = *x1;
                prev_qy = *y1;
                let pts = quadratic_bezier_segments(cx, cy, *x1, *y1, *x, *y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = *x;
                cy = *y;
            }
            PathCommand::QuadraticToRel { dx1, dy1, dx, dy } => {
                let x1 = cx + dx1;
                let y1 = cy + dy1;
                let x = cx + dx;
                let y = cy + dy;
                prev_qx = x1;
                prev_qy = y1;
                let pts = quadratic_bezier_segments(cx, cy, x1, y1, x, y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::SmoothQuadraticTo { x, y } => {
                let x1 = 2.0 * cx - prev_qx;
                let y1 = 2.0 * cy - prev_qy;
                prev_qx = x1;
                prev_qy = y1;
                let pts = quadratic_bezier_segments(cx, cy, x1, y1, *x, *y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = *x;
                cy = *y;
            }
            PathCommand::SmoothQuadraticToRel { dx, dy } => {
                let x1 = 2.0 * cx - prev_qx;
                let y1 = 2.0 * cy - prev_qy;
                let x = cx + dx;
                let y = cy + dy;
                prev_qx = x1;
                prev_qy = y1;
                let pts = quadratic_bezier_segments(cx, cy, x1, y1, x, y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::ClosePath => {
                if let Some((sx, sy)) = start.take() {
                    let (px, py) = tf.apply(sx, sy);
                    points.push((px, py));
                }
            }
            PathCommand::ArcTo { rx, ry, xrot, large, sweep, x, y } => {
                let pts = arc_segments(cx, cy, *rx, *ry, *xrot, *large, *sweep, *x, *y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = *x;
                cy = *y;
            }
            PathCommand::ArcToRel { rx, ry, xrot, large, sweep, dx, dy } => {
                let x = cx + dx;
                let y = cy + dy;
                let pts = arc_segments(cx, cy, *rx, *ry, *xrot, *large, *sweep, x, y);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    points.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
        }
    }

    points
}

fn rect_segments(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rx: f32,
    ry: f32,
    tf: &Transform,
) -> Vec<(f32, f32)> {
    let rx = rx.min(w / 2.0).max(0.0);
    let ry = ry.min(h / 2.0).max(0.0);

    if rx <= 0.0 || ry <= 0.0 {
        // Simple rectangle
        let corners = [
            (x, y),
            (x + w, y),
            (x + w, y + h),
            (x, y + h),
            (x, y), // close
        ];
        return corners.iter().map(|(px, py)| tf.apply(*px, *py)).collect();
    }

    // Rounded rect: approximate corners
    let mut pts = Vec::new();
    let segments_per_corner = 8;

    // Top edge + top-right corner
    for i in 0..=segments_per_corner {
        let angle = consts::PI * 1.5 + consts::FRAC_PI_2 * i as f32 / segments_per_corner as f32;
        let cx = x + w - rx;
        let cy = y + ry;
        pts.push((cx + rx * angle.nexus_cos(), cy + ry * angle.nexus_sin()));
    }
    // Right edge + bottom-right corner
    for i in 1..=segments_per_corner {
        let angle = consts::FRAC_PI_2 * i as f32 / segments_per_corner as f32;
        let cx = x + w - rx;
        let cy = y + h - ry;
        pts.push((cx + rx * angle.nexus_cos(), cy + ry * angle.nexus_sin()));
    }
    // Bottom edge + bottom-left corner
    for i in 1..=segments_per_corner {
        let angle = consts::PI * 0.5 + consts::FRAC_PI_2 * i as f32 / segments_per_corner as f32;
        let cx = x + rx;
        let cy = y + h - ry;
        pts.push((cx + rx * angle.nexus_cos(), cy + ry * angle.nexus_sin()));
    }
    // Left edge + top-left corner
    for i in 1..segments_per_corner {
        let angle = consts::PI + consts::FRAC_PI_2 * i as f32 / segments_per_corner as f32;
        let cx = x + rx;
        let cy = y + ry;
        pts.push((cx + rx * angle.nexus_cos(), cy + ry * angle.nexus_sin()));
    }

    pts.iter().map(|(px, py)| tf.apply(*px, *py)).collect()
}

fn circle_segments(cx: f32, cy: f32, r: f32, tf: &Transform) -> Vec<(f32, f32)> {
    let n = 32;
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let angle = 2.0 * consts::PI * i as f32 / n as f32;
        let x = cx + r * angle.nexus_cos();
        let y = cy + r * angle.nexus_sin();
        pts.push(tf.apply(x, y));
    }
    pts
}

fn ellipse_segments(cx: f32, cy: f32, rx: f32, ry: f32, tf: &Transform) -> Vec<(f32, f32)> {
    let n = 32;
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let angle = 2.0 * consts::PI * i as f32 / n as f32;
        let x = cx + rx * angle.nexus_cos();
        let y = cy + ry * angle.nexus_sin();
        pts.push(tf.apply(x, y));
    }
    pts
}

// ---------------------------------------------------------------------------
// Bezier curve segment approximation
// ---------------------------------------------------------------------------

/// Flatten an SVG elliptical arc (current point → endpoint) into line-segment
/// endpoints, via the spec's endpoint-to-center parameterisation (SVG F.6.5).
/// Returns the sampled points AFTER the start (the start is already in `points`),
/// including the endpoint. Degenerate arcs (zero radius / coincident endpoints)
/// fall back to a straight line.
#[allow(clippy::too_many_arguments)]
fn arc_segments(
    x1: f32,
    y1: f32,
    rx_in: f32,
    ry_in: f32,
    xrot_deg: f32,
    large: bool,
    sweep: bool,
    x2: f32,
    y2: f32,
) -> Vec<(f32, f32)> {
    // Coincident endpoints → nothing to draw (per spec).
    if (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6 {
        return Vec::new();
    }
    let mut rx = rx_in.abs();
    let mut ry = ry_in.abs();
    // Zero radius → straight line to the endpoint.
    if rx < 1e-6 || ry < 1e-6 {
        return alloc::vec![(x2, y2)];
    }
    let phi = xrot_deg.nexus_to_radians();
    let cos_p = phi.nexus_cos();
    let sin_p = phi.nexus_sin();

    // Step 1: transform endpoints to the rotated, mid-centred frame (x1', y1').
    let dx = (x1 - x2) * 0.5;
    let dy = (y1 - y2) * 0.5;
    let x1p = cos_p * dx + sin_p * dy;
    let y1p = -sin_p * dx + cos_p * dy;

    // Correct out-of-range radii (spec F.6.6).
    let lambda = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.nexus_sqrt();
        rx *= s;
        ry *= s;
    }

    // Step 2: centre (cx', cy') in the rotated frame.
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let num = (rx2 * ry2 - rx2 * y1p * y1p - ry2 * x1p * x1p).max(0.0);
    let den = rx2 * y1p * y1p + ry2 * x1p * x1p;
    let mut coef = if den > 0.0 { (num / den).nexus_sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;

    // Step 3: centre back in the original frame.
    let cx = cos_p * cxp - sin_p * cyp + (x1 + x2) * 0.5;
    let cy = sin_p * cxp + cos_p * cyp + (y1 + y2) * 0.5;

    // Step 4: start angle θ1 and sweep Δθ via atan2 on the normalised vectors.
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = uy.nexus_atan2(ux);
    let mut dtheta = (uy * vx - ux * vy).nexus_atan2(ux * vx + uy * vy);
    let two_pi = core::f32::consts::PI * 2.0;
    if !sweep && dtheta > 0.0 {
        dtheta -= two_pi;
    } else if sweep && dtheta < 0.0 {
        dtheta += two_pi;
    }

    // Segment count proportional to arc length (≈ one segment per 6° of sweep),
    // bounded so AA hides any residual faceting at typical icon/cursor sizes.
    let segs = ((dtheta.abs() / (core::f32::consts::PI / 30.0)).nexus_ceil() as usize)
        .clamp(2, 180);
    let mut pts = Vec::with_capacity(segs);
    for i in 1..=segs {
        let t = theta1 + dtheta * (i as f32 / segs as f32);
        let (st, ct) = (t.nexus_sin(), t.nexus_cos());
        let ex = cx + rx * ct * cos_p - ry * st * sin_p;
        let ey = cy + rx * ct * sin_p + ry * st * cos_p;
        pts.push((ex, ey));
    }
    pts
}

#[allow(clippy::too_many_arguments)]
fn cubic_bezier_segments(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    x3: f32,
    y3: f32,
) -> Vec<(f32, f32)> {
    let n = 16;
    let mut pts = Vec::with_capacity(n);
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let x = u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x3;
        let y = u * u * u * y0 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y3;
        pts.push((x, y));
    }
    pts
}

fn quadratic_bezier_segments(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
) -> Vec<(f32, f32)> {
    let n = 16;
    let mut pts = Vec::with_capacity(n);
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let x = u * u * x0 + 2.0 * u * t * x1 + t * t * x2;
        let y = u * u * y0 + 2.0 * u * t * y1 + t * t * y2;
        pts.push((x, y));
    }
    pts
}

// ---------------------------------------------------------------------------
// Polygon edge generation for scanline renderer
// ---------------------------------------------------------------------------

fn append_shape(edges: &mut Vec<Edge>, next_shape_id: &mut u32, mut shape_edges: Vec<Edge>) {
    if shape_edges.is_empty() {
        return;
    }
    let shape_id = *next_shape_id;
    *next_shape_id = (*next_shape_id).saturating_add(1);
    for edge in &mut shape_edges {
        edge.shape_id = shape_id;
    }
    edges.extend(shape_edges);
}

fn polygon_edges(points: &[(f32, f32)], color: Color) -> Vec<Edge> {
    if points.len() < 3 {
        return Vec::new();
    }

    let mut edges = Vec::new();
    for i in 0..points.len() {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % points.len()];

        // Skip horizontal edges (they don't affect scanline fill)
        if (y1 - y0).abs() < 0.001 {
            continue;
        }

        // Ensure y0 <= y1
        let (x0, y0, x1, y1) = if y0 <= y1 { (x0, y0, x1, y1) } else { (x1, y1, x0, y0) };

        edges.push(Edge { x0, y0, x1, y1, color, shape_id: 0 });
    }

    edges
}

fn stroke_edges(points: &[(f32, f32)], width: f32, color: Color) -> Vec<Edge> {
    if points.len() < 2 {
        return Vec::new();
    }

    let half = (width / 2.0).max(0.5);
    let mut edges = Vec::new();

    for i in 0..points.len() - 1 {
        let (x0, y0) = points[i];
        let (x1, y1) = points[i + 1];

        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).nexus_sqrt().max(0.001);
        let nx = -dy / len * half;
        let ny = dx / len * half;

        let quad =
            vec![(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)];
        edges.extend(polygon_edges(&quad, color));
    }

    edges
}
