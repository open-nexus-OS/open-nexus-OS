// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Edge decoration: the 1px hairline ring and the design system's
//! `inset 0 1px 0` top-shine.
//!
//! Both follow the corner radius rather than being four straight strips —
//! a square frame around a round element is what a naive per-edge fill
//! produces, and it reads as a bug.

use super::RowCanvas;
use nexus_layout_types::Rgba8;

/// One row of the `inset 0 1px 0` top-shine: a single pixel line just inside
/// the top border, inset horizontally so it follows the corner radius instead
/// of poking out of the rounded corners.
pub(crate) fn paint_inset_highlight_row(
    canvas: &mut RowCanvas<'_>,
    rect: (i32, i32, i32, i32),
    radius: i32,
    border: &nexus_layout_types::EdgeBorder,
    color: Rgba8,
) {
    let (x, y, w, h) = rect;
    let inset = border.top.map_or(0, |t| t.width.0.max(0));
    // Corner radius eats the top row entirely; start the shine where the arc
    // has come far enough in that a straight line reads as part of the curve.
    let r = radius.max(0).min(w / 2).min(h / 2);
    let side = r * 3 / 10;
    let (sx, sw) = (x + inset + side, w - 2 * (inset + side));
    if sw > 0 && h > inset {
        canvas.fill_round_rect_row(sx, y + inset, sw, 1, 0, color);
    }
}

pub(crate) fn paint_borders_row(
    canvas: &mut RowCanvas<'_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radius: i32,
    border: &nexus_layout_types::EdgeBorder,
) {
    // Uniform border (the kit's `Style::border` sets all four edges the same):
    // stroke a ring that FOLLOWS the corner radius — four straight strips on a
    // rounded fill read as a square frame around a round element.
    if let (Some(t), Some(bo), Some(l), Some(r)) =
        (border.top, border.bottom, border.left, border.right)
    {
        let uniform = t.width == bo.width
            && t.width == l.width
            && t.width == r.width
            && t.color == bo.color
            && t.color == l.color
            && t.color == r.color;
        if uniform {
            canvas.stroke_round_rect_row(x, y, w, h, radius, t.width.0.max(1), t.color);
            return;
        }
    }
    if let Some(t) = border.top {
        canvas.fill_round_rect_row(x, y, w, t.width.0.max(0), 0, t.color);
    }
    if let Some(b) = border.bottom {
        let bw = b.width.0.max(0);
        canvas.fill_round_rect_row(x, y + h - bw, w, bw, 0, b.color);
    }
    if let Some(l) = border.left {
        canvas.fill_round_rect_row(x, y, l.width.0.max(0), h, 0, l.color);
    }
    if let Some(r) = border.right {
        let rw = r.width.0.max(0);
        canvas.fill_round_rect_row(x + w - rw, y, rw, h, 0, r.color);
    }
}
