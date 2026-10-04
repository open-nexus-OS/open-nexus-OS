// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Edge decoration: the 1px hairline ring and the design system's
//! `inset 0 1px 0` top-shine.
//!
//! Both follow the corner radius rather than being four straight strips —
//! a square frame around a round element is what a naive per-edge fill
//! produces, and it reads as a bug.

use super::shapes::round_rect_coverage;
use super::RowCanvas;
use nexus_layout_types::Rgba8;

/// One row of the `inset 0 1px 0` top-shine, just inside the top border, anti-aliased.
pub(crate) fn paint_inset_highlight_row(
    canvas: &mut RowCanvas<'_>,
    rect: (i32, i32, i32, i32),
    radius: i32,
    border: &nexus_layout_types::EdgeBorder,
    color: Rgba8,
) {
    // `inset 0 1px 0`: the shine covers what lies inside the outline but not inside the outline
    // moved one pixel down — the top-facing edge band of the element's OWN shape: a line on a flat
    // top, an arc on a circle, nothing on a vertical side. (A straight line across the top,
    // inset by part of the radius, overhung every round button as a bar — TASK-0251 P2a step 3.)
    let (x, y, w, h) = rect;
    let inset = border.top.map_or(0, |t| t.width.0.max(0));
    let (ix, iy, iw, ih) = (x + inset, y + inset, w - 2 * inset, h - 2 * inset);
    if iw <= 0 || ih <= 0 {
        return;
    }
    let r = (radius - inset).max(0).min(iw / 2).min(ih / 2);
    // The band lives in the rows the top edge and its corner arcs span.
    if canvas.y < iy || canvas.y > iy + r {
        return;
    }
    let inner = (ix as f32, iy as f32, iw as f32, ih as f32);
    let py = canvas.y as f32 + 0.5;
    for xx in ix..ix + iw {
        let px = xx as f32 + 0.5;
        let band = round_rect_coverage(px, py, inner, r as f32)
            - round_rect_coverage(px, py - 1.0, inner, r as f32);
        canvas.blend_covered(xx, color, band);
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

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::EdgeBorder;

    /// On a round button the shine is an arc along the top of the circle — never a bar that
    /// overhangs it: on the circle's top rows only pixels near the middle light up.
    #[test]
    fn test_reject_a_shine_outside_a_round_element() {
        let shine = Rgba8 { r: 255, g: 255, b: 255, a: 255 };
        let mut buf = vec![0u8; 44 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 0, 44);
        paint_inset_highlight_row(&mut canvas, (0, 0, 44, 44), 22, &EdgeBorder::default(), shine);
        let lit: Vec<usize> =
            buf.chunks_exact(4).enumerate().filter(|(_, p)| p[0] > 0).map(|(i, _)| i).collect();
        assert!(!lit.is_empty(), "the top of the circle shines");
        assert!(lit.iter().all(|&i| (14..30).contains(&i)), "only the arc's top: {lit:?}");
        // A flat-topped panel shines across its width between the corners.
        let mut buf = vec![0u8; 44 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 0, 44);
        paint_inset_highlight_row(&mut canvas, (0, 0, 44, 20), 4, &EdgeBorder::default(), shine);
        assert_eq!(buf[22 * 4], 255);
    }
}
