// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The painter's two per-node transforms beyond scale and translate (TASK-0354
//! P10/P13): the ROTATION of a box's own glyph shape about the box centre (the rest rotation of
//! `VisualStyle::rotate_deg` plus a motion's `NodeAnim::rot_deg`), and the ROUNDED CLIP a part is
//! painted under (`NodeAnim::clip` — a pip or a shimmer band entering its track from outside).
//! Both are inert at their defaults: no rotation is no [`Rot`] (the un-rotated arms run the float
//! expressions they always ran), no clip is `RowCanvas::round == None` (one branch in the pixel
//! writers). The clip takes its soft columns and their coverage from the SAME two functions the
//! capsule fill uses (`corner_band`, `round_rect_coverage`), so a clipped child ends byte-equal
//! to its capsule. Stack values only — nothing is allocated per row.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (`RoundClip` is public through `NodeAnim`)
//! TEST_COVERAGE: `tests` below (the turn's sine and cosine) + `crate::tests` (`a_rotation_*`,
//! `rotated_ink_stays_inside_its_box`, `a_clipped_child_ends_byte_equal_to_its_capsule`,
//! `a_part_outside_its_clip_paints_nothing`).

use crate::shapes::{corner_band, covered, round_rect_coverage};
use crate::RowCanvas;
use nexus_layout_types::{PathShape, Rgba8, ShapeKind};

/// A rounded rect a node is painted under: its pixels outside are not painted, and at the
/// rounded ends they take the coverage the rect's own fill has there. In the coordinates of the
/// surface the row is painted into (after a scroll shift and a scroll view), `radius` clamped to
/// half the smaller side like a fill's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundClip {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub radius: i32,
}

impl RoundClip {
    /// The clip on surface row `sy`: `None` when the row lies outside it (nothing is painted),
    /// else the surface columns `[c0, c1)` that may be painted and, on a row of its rounded
    /// corners, the row's soft edge. Mirrors `fill_round_rect_row` step for step.
    pub(crate) fn row(&self, sy: i32) -> Option<((i32, i32), Option<RowRound>)> {
        let (x, y, w, h) = (self.x, self.y, self.w, self.h);
        if w <= 0 || h <= 0 || sy < y || sy >= y.saturating_add(h) {
            return None;
        }
        let r = self.radius.max(0).min(w / 2).min(h / 2);
        if !(sy < y + r || sy >= y + h - r) {
            return Some(((x, x + w), None));
        }
        let rect = (x as f32, y as f32, w as f32, h as f32);
        let py = sy as f32 + 0.5;
        let (lo, hi) = corner_band(rect, py, r, (x, x + r));
        let far = 2 * x + w;
        Some(((lo, far - lo), Some(RowRound { rect, r: r as f32, py, hard: (hi, far - hi) })))
    }
}

/// One row of a [`RoundClip`] through its rounded corners: the surface columns `hard` are wholly
/// inside, every other column the scissor lets through is asked for its coverage.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RowRound {
    rect: (f32, f32, f32, f32),
    r: f32,
    py: f32,
    hard: (i32, i32),
}

impl RowRound {
    /// The fraction of surface pixel `sx` the clip covers on this row.
    #[inline]
    pub(crate) fn coverage(&self, sx: i32) -> f32 {
        if sx >= self.hard.0 && sx < self.hard.1 {
            1.0
        } else {
            round_rect_coverage(sx as f32 + 0.5, self.py, self.rect, self.r)
        }
    }

    /// `c` as the clip lets it through at surface pixel `sx` — the rounding of `blend_covered`.
    #[inline]
    pub(crate) fn cover(&self, sx: i32, c: Rgba8) -> Rgba8 {
        let cov = self.coverage(sx);
        if cov >= 1.0 {
            c
        } else {
            covered(c, cov)
        }
    }
}

impl RowCanvas<'_> {
    /// The part of model span `[x0, x1)` a span writer may lay as ONE span: all of it without a
    /// rounded clip. Under one, the pixels in the clip's soft columns are painted here, one at a
    /// time through `px` (which asks the clip), and the hard middle is returned.
    #[inline]
    pub(crate) fn soft_ends(
        &mut self,
        x0: i32,
        x1: i32,
        mut px: impl FnMut(&mut Self, i32),
    ) -> (i32, i32) {
        let Some(k) = self.round else { return (x0, x1) };
        let (a, b) = self.visible(x0, x1);
        let (m0, m1) =
            (k.hard.0.saturating_add(self.shift_x), k.hard.1.saturating_add(self.shift_x));
        for xx in a..b.min(m0) {
            px(self, xx);
        }
        for xx in a.max(m1)..b {
            px(self, xx);
        }
        (a.max(m0), b.min(m1))
    }
}

/// A rotation about a centre, clockwise on the surface (y grows downwards).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rot {
    cos: f32,
    sin: f32,
    cx: f32,
    cy: f32,
}

impl Rot {
    /// `deg` degrees clockwise about the centre of `rect`; `None` for no turn (a multiple of a
    /// whole one), so the un-rotated paint never meets this type.
    pub(crate) fn about(deg: i32, (x, y, w, h): (f32, f32, f32, f32)) -> Option<Self> {
        let deg = deg.rem_euclid(360);
        if deg == 0 {
            return None;
        }
        let (sin, cos) = sin_cos_deg(deg);
        Some(Self { cos, sin, cx: x + w * 0.5, cy: y + h * 0.5 })
    }

    #[inline]
    pub(crate) fn map(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        (self.cx + dx * self.cos - dy * self.sin, self.cy + dx * self.sin + dy * self.cos)
    }
}

/// `(sin, cos)` of `deg` whole degrees in `0..360` (`no_std`: no libm). Exact at the quarter
/// turns; between them the series on at most 45 degrees, exact to under 1e-6.
fn sin_cos_deg(deg: i32) -> (f32, f32) {
    let series = |d: i32| {
        let x = d as f32 * (core::f32::consts::PI / 180.0);
        let x2 = x * x;
        let s = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0)));
        let c = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0 * (1.0 - x2 / 56.0)));
        (s, c)
    };
    let (quarter, rem) = (deg / 90, deg % 90);
    let (s, c) = if rem == 0 {
        (0.0, 1.0)
    } else if rem <= 45 {
        series(rem)
    } else {
        let (s, c) = series(90 - rem);
        (c, s)
    };
    match quarter {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// This row's slice of a box's glyph shape turned by `rot` inside the drawn rect `(x, y, w, h)`:
/// the points are rotated, then filled or stroked as at rest, and the ink is cut to the rect's
/// columns (its rows are the caller's gate) — a turned glyph never grows its box's damage.
/// `false` for a shape that does not rotate (a rect, a circle, a sprite): the caller paints it
/// as at rest.
pub(crate) fn paint_rotated_shape_row(
    canvas: &mut RowCanvas<'_>,
    shape: &ShapeKind,
    (x, y, w, h): (i32, i32, i32, i32),
    rot: Rot,
    c: Rgba8,
) -> bool {
    if matches!(shape, ShapeKind::Rect | ShapeKind::Circle | ShapeKind::Raster { .. }) {
        return false;
    }
    let clip0 = canvas.clip_x;
    let cols =
        (x.saturating_sub(canvas.shift_x), x.saturating_add(w).saturating_sub(canvas.shift_x));
    canvas.clip_x = Some(match clip0 {
        Some((c0, c1)) => (c0.max(cols.0), c1.min(cols.1)),
        None => cols,
    });
    let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
    let contour = |canvas: &mut RowCanvas<'_>, ps: &PathShape| {
        let pts = &ps.points;
        canvas.fill_polygon_row(
            pts.len(),
            |i| {
                let p = &pts[i];
                rot.map((xf + p.x_milli as f32 / 1000.0 * wf, yf + p.y_milli as f32 / 1000.0 * hf))
            },
            c,
        );
    };
    match shape {
        ShapeKind::TriangleUp => {
            let pts = [(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)].map(|p| rot.map(p));
            canvas.fill_polygon_row(3, |i| pts[i], c);
        }
        ShapeKind::TriangleDown => {
            let pts = [(xf, yf), (xf + wf, yf), (xf + wf / 2.0, yf + hf)].map(|p| rot.map(p));
            canvas.fill_polygon_row(3, |i| pts[i], c);
        }
        ShapeKind::Path(ps) => contour(canvas, ps),
        ShapeKind::Vector(contours) => {
            for ps in contours {
                contour(canvas, ps);
            }
        }
        ShapeKind::Stroke { paths, width_milli } => {
            canvas.stroke_paths_row_turned(paths, (xf, yf, wf, hf), *width_milli, Some(rot), c);
        }
        ShapeKind::Rect | ShapeKind::Circle | ShapeKind::Raster { .. } => {}
    }
    canvas.clip_x = clip0;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_turns_sine_and_cosine_are_exact_to_a_millionth() {
        for deg in 0..360 {
            let (s, c) = sin_cos_deg(deg);
            let rad = (deg as f32).to_radians();
            assert!((s - rad.sin()).abs() < 1e-6 && (c - rad.cos()).abs() < 1e-6, "{deg}");
        }
        assert_eq!(sin_cos_deg(90), (1.0, 0.0));
        assert_eq!(sin_cos_deg(180), (0.0, -1.0));
        assert_eq!(sin_cos_deg(270), (-1.0, 0.0));
    }

    #[test]
    fn no_turn_is_no_rotation_and_a_quarter_turn_is_clockwise() {
        let rect = (0.0, 0.0, 10.0, 10.0);
        assert!(Rot::about(0, rect).is_none());
        assert!(Rot::about(360, rect).is_none() && Rot::about(-720, rect).is_none());
        // The top middle goes to the right middle (y grows downwards).
        let quarter = Rot::about(90, rect).map(|r| r.map((5.0, 0.0)));
        assert_eq!(quarter, Some((10.0, 5.0)));
        assert_eq!(Rot::about(-270, rect).map(|r| r.map((5.0, 0.0))), Some((10.0, 5.0)));
    }
}
