// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

use core::f32::consts;

use crate::elements::{
    FillRule, LineCap, LineJoin, PathCommand, PathData, StrokeStyle, SvgDocument, SvgElement,
    Transform,
};
use crate::gradient::{resolve_shape_paint, BBox, ShapePaint};
use crate::math::F32Math;

/// A single line segment in screen space (y-sorted: y0 <= y1). Carries no colour:
/// the fill (solid or gradient) lives in the shape's `ShapePaint`, indexed by
/// `shape_id`, and is evaluated per pixel at raster time.
#[derive(Debug, Clone, Copy)]
pub struct Edge {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub shape_id: u32,
    /// Winding direction of the original (pre-y-sort) edge: +1 if it ran
    /// downward (y increasing), -1 if upward. Drives the nonzero winding rule.
    pub dir: i32,
    /// Fill rule for the shape this edge belongs to (all edges of a shape agree).
    pub fill_rule: FillRule,
}

/// Tessellate with a `root` transform applied to every element — used to render
/// at an arbitrary scale (HiDPI/5K). Curve flattening sees the scaled transform,
/// so geometry stays crisp at the target resolution.
///
/// Returns the edge list plus a parallel `paints` vector where `paints[shape_id]`
/// is the resolved (device-space) paint for that shape — solid or gradient. The
/// 1:1 indexing is maintained by [`append_shape`], which pushes exactly one paint
/// per emitted shape.
pub fn tessellate_document_with(doc: &SvgDocument, root: &Transform) -> (Vec<Edge>, Vec<ShapePaint>) {
    let mut edges = Vec::new();
    let mut paints = Vec::new();
    let mut next_shape_id = 0;

    for elem in &doc.elements {
        tessellate_element(elem, root, 1.0, &mut edges, &mut paints, &mut next_shape_id, doc);
    }

    (edges, paints)
}

fn tessellate_element(
    elem: &SvgElement,
    parent_tf: &Transform,
    parent_opacity: f32,
    edges: &mut Vec<Edge>,
    paints: &mut Vec<ShapePaint>,
    next_shape_id: &mut u32,
    doc: &SvgDocument,
) {
    match elem {
        SvgElement::Group { children, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);
            for child in children {
                tessellate_element(child, &tf, op, edges, paints, next_shape_id, doc);
            }
        }
        SvgElement::Path { data, fill, stroke, stroke_width, stroke_style, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            // Split the path into subpaths (at each MoveTo) so disjoint contours
            // — e.g. a donut's outer + inner ring — never bridge, and their
            // windings combine under one shape for correct holes.
            let subpaths = path_to_subpaths(data, &tf);
            // The geometry bbox (device space) anchors objectBoundingBox gradients.
            let bbox = bbox_of_subpaths(&subpaths);
            if let (Some(p), Some(bb)) = (fill, bbox) {
                if let Some(paint) = resolve_shape_paint(p, doc, &tf, op, bb) {
                    let mut shape_edges = Vec::new();
                    for sub in &subpaths {
                        shape_edges.extend(polygon_edges(sub, data.fill_rule));
                    }
                    append_shape(edges, paints, next_shape_id, shape_edges, paint);
                }
            }
            if let (Some(p), Some(bb)) = (stroke, bbox) {
                if let Some(paint) = resolve_shape_paint(p, doc, &tf, op, bb) {
                    // Stroke width is in user units, but `sub` is already device
                    // space — scale the width by the transform so the stroke keeps
                    // its intended weight at any render scale (HiDPI/icon upscaling).
                    let sw = *stroke_width * transform_scale(&tf);
                    let mut shape_edges = Vec::new();
                    for sub in &subpaths {
                        shape_edges.extend(stroke_edges(sub, sw, *stroke_style, false));
                    }
                    append_shape(edges, paints, next_shape_id, shape_edges, paint);
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
            stroke_style,
            transform,
            opacity,
        } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = rect_segments(*x, *y, *width, *height, *rx, *ry, &tf);
            emit_filled_shape(fill, &segments, FillRule::NonZero, &tf, op, doc, edges, paints, next_shape_id);
            emit_stroked_shape(stroke, &segments, *stroke_width * transform_scale(&tf), *stroke_style, true, &tf, op, doc, edges, paints, next_shape_id);
        }
        SvgElement::Circle { cx, cy, r, fill, stroke, stroke_width, stroke_style, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = circle_segments(*cx, *cy, *r, &tf);
            emit_filled_shape(fill, &segments, FillRule::NonZero, &tf, op, doc, edges, paints, next_shape_id);
            emit_stroked_shape(stroke, &segments, *stroke_width * transform_scale(&tf), *stroke_style, true, &tf, op, doc, edges, paints, next_shape_id);
        }
        SvgElement::Ellipse { cx, cy, rx, ry, fill, stroke, stroke_width, stroke_style, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let segments = ellipse_segments(*cx, *cy, *rx, *ry, &tf);
            emit_filled_shape(fill, &segments, FillRule::NonZero, &tf, op, doc, edges, paints, next_shape_id);
            emit_stroked_shape(stroke, &segments, *stroke_width * transform_scale(&tf), *stroke_style, true, &tf, op, doc, edges, paints, next_shape_id);
        }
        SvgElement::Line { x1, y1, x2, y2, stroke, stroke_width, stroke_style, transform, opacity } => {
            let _ = stroke_style;
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            if let Some(p) = stroke {
                let (sx, sy) = tf.apply(*x1, *y1);
                let (ex, ey) = tf.apply(*x2, *y2);
                if let Some(bb) = bbox_of_points(&[(sx, sy), (ex, ey)]) {
                    if let Some(paint) = resolve_shape_paint(p, doc, &tf, op, bb) {
                        let half = (*stroke_width * transform_scale(&tf)).max(0.5) / 2.0;
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
                        append_shape(edges, paints, next_shape_id, polygon_edges(&pts, FillRule::NonZero), paint);
                    }
                }
            }
        }
        SvgElement::Polygon { points, fill, stroke, stroke_width, stroke_style, transform, opacity } => {
            let tf = combine_transform(parent_tf, transform);
            let op = parent_opacity * opacity.clamp(0.0, 1.0);

            let pts: Vec<(f32, f32)> = points.iter().map(|(x, y)| tf.apply(*x, *y)).collect();
            emit_filled_shape(fill, &pts, FillRule::NonZero, &tf, op, doc, edges, paints, next_shape_id);
            emit_stroked_shape(stroke, &pts, *stroke_width * transform_scale(&tf), *stroke_style, true, &tf, op, doc, edges, paints, next_shape_id);
        }
        // Defs entries (gradients) are not rendered directly.
        SvgElement::LinearGradient { .. } | SvgElement::RadialGradient { .. } => {}
    }
}

/// Resolve `fill` over the polygon `points` (device space) and emit a filled shape.
#[allow(clippy::too_many_arguments)]
fn emit_filled_shape(
    fill: &Option<crate::elements::Paint>,
    points: &[(f32, f32)],
    fill_rule: FillRule,
    tf: &Transform,
    opacity: f32,
    doc: &SvgDocument,
    edges: &mut Vec<Edge>,
    paints: &mut Vec<ShapePaint>,
    next_shape_id: &mut u32,
) {
    if let (Some(p), Some(bb)) = (fill, bbox_of_points(points)) {
        if let Some(paint) = resolve_shape_paint(p, doc, tf, opacity, bb) {
            append_shape(edges, paints, next_shape_id, polygon_edges(points, fill_rule), paint);
        }
    }
}

/// Resolve `stroke` over the outline `points` (device space) and emit a stroked shape.
#[allow(clippy::too_many_arguments)]
fn emit_stroked_shape(
    stroke: &Option<crate::elements::Paint>,
    points: &[(f32, f32)],
    stroke_width: f32,
    stroke_style: StrokeStyle,
    closed: bool,
    tf: &Transform,
    opacity: f32,
    doc: &SvgDocument,
    edges: &mut Vec<Edge>,
    paints: &mut Vec<ShapePaint>,
    next_shape_id: &mut u32,
) {
    if let (Some(p), Some(bb)) = (stroke, bbox_of_points(points)) {
        if let Some(paint) = resolve_shape_paint(p, doc, tf, opacity, bb) {
            append_shape(
                edges,
                paints,
                next_shape_id,
                stroke_edges(points, stroke_width, stroke_style, closed),
                paint,
            );
        }
    }
}

/// Device-space bounding box of a point set, or `None` if fewer than 1 point.
fn bbox_of_points(points: &[(f32, f32)]) -> Option<BBox> {
    let mut it = points.iter();
    let &(x0, y0) = it.next()?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (x0, y0, x0, y0);
    for &(x, y) in it {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Some((min_x, min_y, max_x, max_y))
}

/// Bounding box across all subpaths.
fn bbox_of_subpaths(subpaths: &[Vec<(f32, f32)>]) -> Option<BBox> {
    let mut acc: Option<BBox> = None;
    for sub in subpaths {
        if let Some((nx, ny, mx, my)) = bbox_of_points(sub) {
            acc = Some(match acc {
                None => (nx, ny, mx, my),
                Some((ax, ay, bx, by)) => (ax.min(nx), ay.min(ny), bx.max(mx), by.max(my)),
            });
        }
    }
    acc
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
// Shape segment generators
// ---------------------------------------------------------------------------

fn path_to_subpaths(data: &PathData, tf: &Transform) -> Vec<Vec<(f32, f32)>> {
    let mut subpaths: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let mut cx: f32 = 0.0;
    let mut cy: f32 = 0.0;
    let mut start: Option<(f32, f32)> = None;
    // Curves flatten to a device-space tolerance, so they stay crisp at any scale.
    let scale = transform_scale(tf);

    // Track previous control point for smooth curves
    let mut prev_cx2: f32 = 0.0;
    let mut prev_cy2: f32 = 0.0;
    let mut prev_qx: f32 = 0.0;
    let mut prev_qy: f32 = 0.0;

    for cmd in &data.commands {
        match cmd {
            PathCommand::MoveTo { x, y } => {
                // A MoveTo starts a new subpath — flush the current one so
                // disjoint contours never bridge (correct holes under nonzero).
                if !cur.is_empty() {
                    subpaths.push(core::mem::take(&mut cur));
                }
                let (px, py) = tf.apply(*x, *y);
                cur.push((px, py));
                cx = *x;
                cy = *y;
                start = Some((*x, *y));
            }
            PathCommand::MoveToRel { dx, dy } => {
                if !cur.is_empty() {
                    subpaths.push(core::mem::take(&mut cur));
                }
                let nx = cx + dx;
                let ny = cy + dy;
                let (px, py) = tf.apply(nx, ny);
                cur.push((px, py));
                cx = nx;
                cy = ny;
                start = Some((nx, ny));
            }
            PathCommand::LineTo { x, y } => {
                let (px, py) = tf.apply(*x, *y);
                cur.push((px, py));
                cx = *x;
                cy = *y;
            }
            PathCommand::LineToRel { dx, dy } => {
                let nx = cx + dx;
                let ny = cy + dy;
                let (px, py) = tf.apply(nx, ny);
                cur.push((px, py));
                cx = nx;
                cy = ny;
            }
            PathCommand::HorizontalTo { x } => {
                let (px, py) = tf.apply(*x, cy);
                cur.push((px, py));
                cx = *x;
            }
            PathCommand::HorizontalToRel { dx } => {
                let nx = cx + dx;
                let (px, py) = tf.apply(nx, cy);
                cur.push((px, py));
                cx = nx;
            }
            PathCommand::VerticalTo { y } => {
                let (px, py) = tf.apply(cx, *y);
                cur.push((px, py));
                cy = *y;
            }
            PathCommand::VerticalToRel { dy } => {
                let ny = cy + dy;
                let (px, py) = tf.apply(cx, ny);
                cur.push((px, py));
                cy = ny;
            }
            PathCommand::CubicTo { x1, y1, x2, y2, x, y } => {
                prev_cx2 = *x2;
                prev_cy2 = *y2;
                let pts = cubic_bezier_segments(cx, cy, *x1, *y1, *x2, *y2, *x, *y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
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
                let pts = cubic_bezier_segments(cx, cy, x1, y1, x2, y2, x, y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::SmoothCubicTo { x2, y2, x, y } => {
                let x1 = 2.0 * cx - prev_cx2;
                let y1 = 2.0 * cy - prev_cy2;
                prev_cx2 = *x2;
                prev_cy2 = *y2;
                let pts = cubic_bezier_segments(cx, cy, x1, y1, *x2, *y2, *x, *y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
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
                let pts = cubic_bezier_segments(cx, cy, x1, y1, x2, y2, x, y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::QuadraticTo { x1, y1, x, y } => {
                prev_qx = *x1;
                prev_qy = *y1;
                let pts = quadratic_bezier_segments(cx, cy, *x1, *y1, *x, *y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
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
                let pts = quadratic_bezier_segments(cx, cy, x1, y1, x, y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::SmoothQuadraticTo { x, y } => {
                let x1 = 2.0 * cx - prev_qx;
                let y1 = 2.0 * cy - prev_qy;
                prev_qx = x1;
                prev_qy = y1;
                let pts = quadratic_bezier_segments(cx, cy, x1, y1, *x, *y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
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
                let pts = quadratic_bezier_segments(cx, cy, x1, y1, x, y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
            PathCommand::ClosePath => {
                if let Some((sx, sy)) = start.take() {
                    let (px, py) = tf.apply(sx, sy);
                    cur.push((px, py));
                }
            }
            PathCommand::ArcTo { rx, ry, xrot, large, sweep, x, y } => {
                let pts = arc_segments(cx, cy, *rx, *ry, *xrot, *large, *sweep, *x, *y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
                }
                cx = *x;
                cy = *y;
            }
            PathCommand::ArcToRel { rx, ry, xrot, large, sweep, dx, dy } => {
                let x = cx + dx;
                let y = cy + dy;
                let pts = arc_segments(cx, cy, *rx, *ry, *xrot, *large, *sweep, x, y, scale);
                for (px, py) in pts {
                    let (tx, ty) = tf.apply(px, py);
                    cur.push((tx, ty));
                }
                cx = x;
                cy = y;
            }
        }
    }

    if !cur.is_empty() {
        subpaths.push(cur);
    }
    subpaths
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
    scale: f32,
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
    // Δθ = angle from u to v. Its sign is the cross product u×v = ux·vy − uy·vx
    // (W3C SVG F.6.5). Using the negated cross flips the sweep sign, so the
    // `sweep`/`large` correction below then adds a full turn → a 270° loop where a
    // 90° rounded corner was meant (the "bent nail" artifact on Lucide arcs).
    let mut dtheta = (ux * vy - uy * vx).nexus_atan2(ux * vx + uy * vy);
    let two_pi = core::f32::consts::PI * 2.0;
    if !sweep && dtheta > 0.0 {
        dtheta -= two_pi;
    } else if sweep && dtheta < 0.0 {
        dtheta += two_pi;
    }

    // Segment count: the greater of an angular bound (~6°/segment) and a
    // device-arc-length bound (~2px chords), so arcs stay smooth when scaled up
    // (5K) while staying cheap at icon size. `scale` is user→device.
    let avg_r = (rx + ry) * 0.5;
    let device_arclen = avg_r * scale.max(1e-3) * dtheta.abs();
    let by_len = (device_arclen / 2.0).nexus_ceil() as usize;
    let by_angle = (dtheta.abs() / (core::f32::consts::PI / 30.0)).nexus_ceil() as usize;
    let segs = by_len.max(by_angle).clamp(2, 512);
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

/// Target flatness in *device* pixels — sub-pixel, so curves stay smooth at any
/// scale (5K included). `scale` maps user-space deviation to device pixels.
const FLATNESS_PX: f32 = 0.2;
/// Recursion guard for adaptive subdivision.
const MAX_SUBDIV: u32 = 18;

fn midpoint(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}

/// Perpendicular distance of `p` from the line through `a`,`b` (or |p−a| if a==b).
fn point_line_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).nexus_sqrt();
    if len < 1e-6 {
        let (ex, ey) = (p.0 - a.0, p.1 - a.1);
        return (ex * ex + ey * ey).nexus_sqrt();
    }
    ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len
}

/// Adaptively flatten a cubic Bézier (de Casteljau) until each control point is
/// within `tol` (user space) of the chord, then emit the endpoint. `tol` is
/// `FLATNESS_PX / scale`, so the device-space error stays sub-pixel at any scale.
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
    scale: f32,
) -> Vec<(f32, f32)> {
    let tol = (FLATNESS_PX / scale.max(1e-3)).max(1e-5);
    let mut out = Vec::new();
    subdivide_cubic((x0, y0), (x1, y1), (x2, y2), (x3, y3), tol, 0, &mut out);
    out
}

fn subdivide_cubic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    tol: f32,
    depth: u32,
    out: &mut Vec<(f32, f32)>,
) {
    if depth >= MAX_SUBDIV
        || (point_line_dist(p1, p0, p3).max(point_line_dist(p2, p0, p3)) <= tol)
    {
        out.push(p3);
        return;
    }
    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let p0123 = midpoint(p012, p123);
    subdivide_cubic(p0, p01, p012, p0123, tol, depth + 1, out);
    subdivide_cubic(p0123, p123, p23, p3, tol, depth + 1, out);
}

/// Adaptively flatten a quadratic Bézier — same scheme as the cubic.
fn quadratic_bezier_segments(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    scale: f32,
) -> Vec<(f32, f32)> {
    let tol = (FLATNESS_PX / scale.max(1e-3)).max(1e-5);
    let mut out = Vec::new();
    subdivide_quadratic((x0, y0), (x1, y1), (x2, y2), tol, 0, &mut out);
    out
}

fn subdivide_quadratic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    tol: f32,
    depth: u32,
    out: &mut Vec<(f32, f32)>,
) {
    if depth >= MAX_SUBDIV || point_line_dist(p1, p0, p2) <= tol {
        out.push(p2);
        return;
    }
    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p012 = midpoint(p01, p12);
    subdivide_quadratic(p0, p01, p012, tol, depth + 1, out);
    subdivide_quadratic(p012, p12, p2, tol, depth + 1, out);
}

/// Approximate uniform scale of an affine transform (√|det|) — user→device.
fn transform_scale(tf: &Transform) -> f32 {
    (tf.a * tf.d - tf.b * tf.c).abs().nexus_sqrt().max(1e-3)
}

// ---------------------------------------------------------------------------
// Polygon edge generation for scanline renderer
// ---------------------------------------------------------------------------

/// Append one shape's edges under a fresh `shape_id` and push its paint so that
/// `paints[shape_id]` lines up. An empty edge set emits nothing (and no paint),
/// preserving the 1:1 invariant.
fn append_shape(
    edges: &mut Vec<Edge>,
    paints: &mut Vec<ShapePaint>,
    next_shape_id: &mut u32,
    mut shape_edges: Vec<Edge>,
    paint: ShapePaint,
) {
    if shape_edges.is_empty() {
        return;
    }
    let shape_id = *next_shape_id;
    *next_shape_id = (*next_shape_id).saturating_add(1);
    debug_assert_eq!(shape_id as usize, paints.len(), "paint index must track shape_id");
    for edge in &mut shape_edges {
        edge.shape_id = shape_id;
    }
    edges.extend(shape_edges);
    paints.push(paint);
}

fn polygon_edges(points: &[(f32, f32)], fill_rule: FillRule) -> Vec<Edge> {
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

        // Winding direction from the original orientation, captured before the
        // y-sort below (downward = +1, upward = -1) for the nonzero rule.
        let dir = if y1 > y0 { 1 } else { -1 };
        // Ensure y0 <= y1
        let (x0, y0, x1, y1) = if y0 <= y1 { (x0, y0, x1, y1) } else { (x1, y1, x0, y0) };

        edges.push(Edge { x0, y0, x1, y1, shape_id: 0, dir, fill_rule });
    }

    edges
}

/// Tessellate a polyline stroke into filled geometry: an offset quad per segment,
/// plus a join at each interior vertex and a cap at each open end, per
/// `StrokeStyle`. All pieces share one shape and are unioned by the nonzero rule,
/// so the overlaps at joins never punch holes. `closed` wraps the last vertex to
/// the first (a join, no caps) — for rect/circle/ellipse/polygon outlines.
fn stroke_edges(
    points: &[(f32, f32)],
    width: f32,
    style: StrokeStyle,
    closed: bool,
) -> Vec<Edge> {
    // Drop consecutive duplicates — zero-length segments have no normal.
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    for &p in points {
        if pts
            .last()
            .map_or(true, |&q| (p.0 - q.0).abs() > 1e-4 || (p.1 - q.1).abs() > 1e-4)
        {
            pts.push(p);
        }
    }
    // A closed outline whose last point repeats the first: drop the duplicate.
    if closed && pts.len() >= 2 {
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        if (first.0 - last.0).abs() < 1e-4 && (first.1 - last.1).abs() < 1e-4 {
            pts.pop();
        }
    }

    let half = (width / 2.0).max(0.5);
    let mut edges = Vec::new();

    if pts.len() < 2 {
        if pts.len() == 1 && style.line_cap == LineCap::Round {
            edges.extend(disc_edges(pts[0].0, pts[0].1, half));
        }
        return edges;
    }

    let n = pts.len();
    let seg_count = if closed { n } else { n - 1 };

    // Offset quad per segment.
    for i in 0..seg_count {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).nexus_sqrt().max(1e-4);
        let nx = -dy / len * half;
        let ny = dx / len * half;
        let quad =
            vec![(x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny)];
        edges.extend(polygon_edges(&quad, FillRule::NonZero));
    }

    // Joins at interior vertices (plus the wrap vertex when closed).
    let join_iter: alloc::vec::Vec<usize> =
        if closed { (0..n).collect() } else { (1..n - 1).collect() };
    for i in join_iter {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        edges.extend(join_edges(prev, cur, next, half, style));
    }

    // Caps at the two open ends.
    if !closed {
        edges.extend(cap_edges(pts[1], pts[0], half, style.line_cap));
        edges.extend(cap_edges(pts[n - 2], pts[n - 1], half, style.line_cap));
    }

    edges
}

/// Normalize a vector; returns `None` if it is ~zero length.
fn normalize(dx: f32, dy: f32) -> Option<(f32, f32)> {
    let len = (dx * dx + dy * dy).nexus_sqrt();
    if len < 1e-5 {
        None
    } else {
        Some((dx / len, dy / len))
    }
}

/// Intersection of lines (p1 + t·d1) and (p2 + s·d2); `None` if ~parallel.
fn line_intersect(p1: (f32, f32), d1: (f32, f32), p2: (f32, f32), d2: (f32, f32)) -> Option<(f32, f32)> {
    let denom = d1.0 * d2.1 - d1.1 * d2.0;
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = ((p2.0 - p1.0) * d2.1 - (p2.1 - p1.1) * d2.0) / denom;
    Some((p1.0 + d1.0 * t, p1.1 + d1.1 * t))
}

/// A filled disc (24-gon) — round joins and round caps.
fn disc_edges(cx: f32, cy: f32, r: f32) -> Vec<Edge> {
    let n = 24;
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let a = 2.0 * consts::PI * i as f32 / n as f32;
        pts.push((cx + r * a.nexus_cos(), cy + r * a.nexus_sin()));
    }
    polygon_edges(&pts, FillRule::NonZero)
}

/// Join geometry at vertex `cur` between the incoming (`prev`→`cur`) and outgoing
/// (`cur`→`next`) segments. Round = a disc; bevel = the corner wedge; miter =
/// bevel plus the outer spike when within the miter limit.
fn join_edges(
    prev: (f32, f32),
    cur: (f32, f32),
    next: (f32, f32),
    half: f32,
    style: StrokeStyle,
) -> Vec<Edge> {
    let (din, dout) = match (normalize(cur.0 - prev.0, cur.1 - prev.1), normalize(next.0 - cur.0, next.1 - cur.1)) {
        (Some(a), Some(b)) => (a, b),
        _ => return Vec::new(),
    };
    let cross = din.0 * dout.1 - din.1 * dout.0;
    if cross.abs() < 1e-5 {
        return Vec::new(); // collinear — the segment quads already meet flush
    }
    if style.line_join == LineJoin::Round {
        return disc_edges(cur.0, cur.1, half);
    }
    let nin = (-din.1, din.0);
    let nout = (-dout.1, dout.0);
    let in_left = (cur.0 + nin.0 * half, cur.1 + nin.1 * half);
    let in_right = (cur.0 - nin.0 * half, cur.1 - nin.1 * half);
    let out_left = (cur.0 + nout.0 * half, cur.1 + nout.1 * half);
    let out_right = (cur.0 - nout.0 * half, cur.1 - nout.1 * half);
    let mut e = Vec::new();
    // Bevel: fill both corner wedges (the outer is the visible gap; the inner is
    // interior and harmless under nonzero).
    e.extend(polygon_edges(&[in_left, out_left, cur], FillRule::NonZero));
    e.extend(polygon_edges(&[in_right, out_right, cur], FillRule::NonZero));
    if style.line_join == LineJoin::Miter {
        // The outer offset lines intersect farther from `cur` than the inner; pick
        // that side and extend to the miter tip if within the limit.
        let m_left = line_intersect(in_left, din, out_left, dout);
        let m_right = line_intersect(in_right, din, out_right, dout);
        let dist2 = |p: (f32, f32)| (p.0 - cur.0) * (p.0 - cur.0) + (p.1 - cur.1) * (p.1 - cur.1);
        let outer = match (m_left, m_right) {
            (Some(l), Some(r)) => {
                if dist2(l) >= dist2(r) {
                    Some((in_left, l, out_left))
                } else {
                    Some((in_right, r, out_right))
                }
            }
            (Some(l), None) => Some((in_left, l, out_left)),
            (None, Some(r)) => Some((in_right, r, out_right)),
            (None, None) => None,
        };
        if let Some((a, m, b)) = outer {
            let mlen = (dist2(m)).nexus_sqrt();
            if mlen <= style.miter_limit * half {
                e.extend(polygon_edges(&[a, m, b], FillRule::NonZero));
            }
        }
    }
    e
}

/// Cap geometry at an open end `end`, where `from` is the previous point (so the
/// outward direction is `end - from`). Round = a disc; square = a half-width
/// extension; butt = nothing.
fn cap_edges(
    from: (f32, f32),
    end: (f32, f32),
    half: f32,
    cap: LineCap,
) -> Vec<Edge> {
    match cap {
        LineCap::Butt => Vec::new(),
        LineCap::Round => disc_edges(end.0, end.1, half),
        LineCap::Square => {
            let Some(dir) = normalize(end.0 - from.0, end.1 - from.1) else {
                return Vec::new();
            };
            let nrm = (-dir.1, dir.0);
            let e_l = (end.0 + nrm.0 * half, end.1 + nrm.1 * half);
            let e_r = (end.0 - nrm.0 * half, end.1 - nrm.1 * half);
            let f_l = (e_l.0 + dir.0 * half, e_l.1 + dir.1 * half);
            let f_r = (e_r.0 + dir.0 * half, e_r.1 + dir.1 * half);
            polygon_edges(&[e_l, f_l, f_r, e_r], FillRule::NonZero)
        }
    }
}

#[cfg(test)]
mod arc_tests {
    use super::arc_segments;

    // A top-right rounded corner: from (20,4) to (22,6), r=2, sweep=1. The correct
    // arc is a 90° quarter-circle centred at (20,6), staying inside the box
    // x∈[20,22], y∈[4,6]. The old Δθ sign bug swept 270° the other way (centre
    // ±2 → x down to ~18, y up to ~8) → the "bent nail" loop. Guard the bounds.
    #[test]
    fn rounded_corner_arc_stays_in_quadrant() {
        let pts = arc_segments(20.0, 4.0, 2.0, 2.0, 0.0, false, true, 22.0, 6.0, 1.0);
        assert!(pts.len() >= 4, "arc flattens to several segments ({})", pts.len());
        for &(x, y) in &pts {
            assert!(
                (19.9..=22.1).contains(&x) && (3.9..=6.1).contains(&y),
                "arc point ({x},{y}) escaped the quarter-circle box — sweep went the wrong way"
            );
        }
        let &(lx, ly) = pts.last().unwrap();
        assert!((lx - 22.0).abs() < 0.05 && (ly - 6.0).abs() < 0.05, "arc ends at the endpoint");
    }

    // Sweep=0 (the other direction) bulges the opposite way — centre (22,4),
    // staying inside x∈[20,22], y∈[4,6] as well but curving through the far side.
    #[test]
    fn arc_sweep_flag_picks_the_other_center() {
        let cw = arc_segments(20.0, 4.0, 2.0, 2.0, 0.0, false, true, 22.0, 6.0, 1.0);
        let ccw = arc_segments(20.0, 4.0, 2.0, 2.0, 0.0, false, false, 22.0, 6.0, 1.0);
        // Midpoints differ (the two arcs bulge to opposite sides of the chord).
        let mid = |v: &[(f32, f32)]| v[v.len() / 2];
        let (ax, ay) = mid(&cw);
        let (bx, by) = mid(&ccw);
        assert!(
            (ax - bx).abs() > 0.5 || (ay - by).abs() > 0.5,
            "sweep flag must change the arc side ({ax},{ay} vs {bx},{by})"
        );
    }
}

#[cfg(test)]
mod a4_tests {
    use super::{cubic_bezier_segments, quadratic_bezier_segments};

    #[test]
    fn cubic_subdivision_scales_with_resolution() {
        // A bowed cubic: more segments when rendered larger (device-space tol).
        let small = cubic_bezier_segments(0.0, 0.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0, 1.0);
        let large = cubic_bezier_segments(0.0, 0.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0, 50.0);
        assert!(small.len() >= 2);
        assert!(
            large.len() > small.len(),
            "higher scale subdivides more ({} vs {})",
            large.len(),
            small.len()
        );
    }

    #[test]
    fn straight_cubic_needs_no_subdivision() {
        // Collinear control points → already flat → just the endpoint.
        let pts = cubic_bezier_segments(0.0, 0.0, 3.0, 0.0, 6.0, 0.0, 9.0, 0.0, 1.0);
        assert!(pts.len() <= 2, "flat cubic stays coarse ({} pts)", pts.len());
    }

    #[test]
    fn quadratic_subdivision_scales_with_resolution() {
        let small = quadratic_bezier_segments(0.0, 0.0, 5.0, 10.0, 10.0, 0.0, 1.0);
        let large = quadratic_bezier_segments(0.0, 0.0, 5.0, 10.0, 10.0, 0.0, 50.0);
        assert!(large.len() > small.len(), "{} vs {}", large.len(), small.len());
    }
}
