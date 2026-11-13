// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The painter's line symbols (`ShapeKind::Stroke`): this row's slice of stroked polylines,
//! covered by the distance to the nearest segment — round caps and joins by construction, a pixel
//! two segments share painted once. The segments are walked once per row and each visits only
//! the pixels its own extent reaches (TASK-0367: a stroked icon asked every segment at every
//! pixel and was the row painter's dearest primitive — 8.6 us an icon row on the host); the
//! least distance per pixel is kept in a fixed stack array, a chunk of the row at a time. Split
//! out of `shapes.rs` (structure ratchet).

use super::shapes::{ceil_i32, floor_i32, sqrt_f32};
use super::RowCanvas;
use nexus_layout_types::{PathPoint, PathShape, Rgba8};

/// Squared distance from `(px, py)` to the segment `a..b`.
#[inline]
fn segment_distance2(px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t =
        if len2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (qx, qy) = (ax + t * dx - px, ay + t * dy - py);
    qx * qx + qy * qy
}

/// The pixels of a row a stroke's distances are kept for at a time (on the stack).
const STROKE_CHUNK: usize = 128;

impl RowCanvas<'_> {
    /// This row's slice of stroked polylines (`ShapeKind::Stroke`) mapped into `rect`: every
    /// pixel within half the stroke width (plus the one-pixel ramp) of the NEAREST segment is
    /// covered by that distance — round caps and joins by construction, and a pixel two
    /// segments share is painted once (no seam, no double blend at a join).
    pub(crate) fn stroke_paths_row(
        &mut self,
        paths: &[PathShape],
        rect: (f32, f32, f32, f32),
        width_milli: u16,
        c: Rgba8,
    ) {
        let (x, y, w, h) = rect;
        let (sx, sy) = (w / 1000.0, h / 1000.0);
        let reach = width_milli as f32 * sx.min(sy) * 0.5 + 0.5;
        let py = self.y as f32 + 0.5;
        if py < y - reach || py > y + h + reach {
            return;
        }
        let map = |p: &PathPoint| (x + p.x_milli as f32 * sx, y + p.y_milli as f32 * sy);
        // A pixel's distance is the least over the segments near it, so the segments are walked
        // once per row and each visits only the pixels its own extent reaches (TASK-0367): the
        // same distances and the same least as asking every segment at every pixel.
        let limit = reach * reach;
        let (lo, hi) = (floor_i32(x - reach), ceil_i32(x + w + reach));
        // Only the model columns that land on the row (its shift and its scissor).
        let (c0, c1) = self.clip_x.unwrap_or((0, self.width));
        let mut start = lo.max(self.shift_x + c0.max(0));
        let stop = hi.min(self.shift_x + c1.min(self.width));
        while start < stop {
            let end = (start + STROKE_CHUNK as i32).min(stop);
            let mut best = [limit; STROKE_CHUNK];
            let mut near = |a: (f32, f32), b: (f32, f32)| {
                let (left, right) = (a.0.min(b.0) - reach, a.0.max(b.0) + reach);
                // One pixel wider than the exact test below, which decides.
                let from = (floor_i32(left) - 1).max(start);
                let to = (ceil_i32(right) + 1).min(end);
                for xx in from..to {
                    let px = xx as f32 + 0.5;
                    if left > px || right < px {
                        continue;
                    }
                    let d2 = segment_distance2(px, py, a, b);
                    let slot = &mut best[(xx - start) as usize];
                    if d2 < *slot {
                        *slot = d2;
                    }
                }
            };
            for path in paths {
                let pts = &path.points;
                let segments =
                    pts.len().saturating_sub(1) + usize::from(path.closed && pts.len() > 2);
                if pts.len() == 1 {
                    // A lone point is a dot: a capsule of length zero.
                    let a = map(&pts[0]);
                    near(a, a);
                }
                for s in 0..segments {
                    let (a, b) = (map(&pts[s]), map(&pts[(s + 1) % pts.len()]));
                    if a.1.min(b.1) - reach > py || a.1.max(b.1) + reach < py {
                        continue;
                    }
                    near(a, b);
                }
            }
            for (i, d2) in best[..(end - start) as usize].iter().enumerate() {
                if *d2 < limit {
                    self.blend_covered(start + i as i32, c, reach - sqrt_f32(*d2));
                }
            }
            start = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba8 = Rgba8 { r: 255, g: 255, b: 255, a: 255 };

    /// Paints `f` over a black row `y` of `width` pixels; the blue channel per pixel.
    fn row(y: i32, width: i32, f: impl FnOnce(&mut RowCanvas<'_>)) -> Vec<u8> {
        let mut buf = vec![0u8; width as usize * 4];
        let mut canvas = RowCanvas::new(&mut buf, y, width);
        f(&mut canvas);
        buf.chunks_exact(4).map(|p| p[0]).collect()
    }

    fn pt(x: u16, y: u16) -> PathPoint {
        PathPoint::new(x, y)
    }

    /// A stroke reaches past its end point by half its width (a round cap), is soft at the
    /// sides, and a pixel two joined segments share is painted ONCE — a translucent colour shows
    /// no darker seam at the join.
    #[test]
    fn a_stroke_has_round_caps_and_paints_a_join_once() {
        let grey = Rgba8 { r: 200, g: 200, b: 200, a: 128 };
        // A polyline in a 20x20 box, mapped 1000 → 20 px: (2,10.5) → (10,10.5) → (18,10.5) —
        // on row 10's pixel centres — 2 px wide.
        let line =
            PathShape { points: vec![pt(100, 525), pt(500, 525), pt(900, 525)], closed: false };
        let r =
            row(10, 22, |c| c.stroke_paths_row(&[line.clone()], (0.0, 0.0, 20.0, 20.0), 100, grey));
        let mid = r[6];
        assert!(mid > 0, "the stroke is painted");
        assert_eq!(r[10], mid, "the join pixel equals a plain one (no double blend)");
        assert!(r[1] > 0, "the round cap reaches past the end point");
        assert_eq!(r[0], 0, "but no further than half the width plus the ramp");
        // One row above the centre line the stroke is partly covered.
        let above = row(9, 22, |c| c.stroke_paths_row(&[line], (0.0, 0.0, 20.0, 20.0), 100, grey));
        assert!(above[6] > 0 && above[6] < mid, "{above:?}");
    }

    /// The stroke as it was first written: every pixel asks every segment. The reference the
    /// row walk is byte-equal to.
    fn stroke_reference(
        canvas: &mut RowCanvas<'_>,
        paths: &[PathShape],
        rect: (f32, f32, f32, f32),
        width_milli: u16,
        c: Rgba8,
    ) {
        let (x, y, w, h) = rect;
        let (sx, sy) = (w / 1000.0, h / 1000.0);
        let reach = width_milli as f32 * sx.min(sy) * 0.5 + 0.5;
        let py = canvas.y as f32 + 0.5;
        if py < y - reach || py > y + h + reach {
            return;
        }
        let map = |p: &PathPoint| (x + p.x_milli as f32 * sx, y + p.y_milli as f32 * sy);
        for xx in floor_i32(x - reach)..ceil_i32(x + w + reach) {
            let px = xx as f32 + 0.5;
            let (mut best, mut hit) = (reach * reach, false);
            for path in paths {
                let pts = &path.points;
                let segments =
                    pts.len().saturating_sub(1) + usize::from(path.closed && pts.len() > 2);
                let mut ask = |a: (f32, f32), b: (f32, f32)| {
                    let d2 = segment_distance2(px, py, a, b);
                    if d2 < best {
                        (best, hit) = (d2, true);
                    }
                };
                if pts.len() == 1 {
                    ask(map(&pts[0]), map(&pts[0]));
                }
                for s in 0..segments {
                    let (a, b) = (map(&pts[s]), map(&pts[(s + 1) % pts.len()]));
                    if a.1.min(b.1) - reach > py || a.1.max(b.1) + reach < py {
                        continue;
                    }
                    if a.0.min(b.0) - reach > px || a.0.max(b.0) + reach < px {
                        continue;
                    }
                    ask(a, b);
                }
            }
            if hit {
                canvas.blend_covered(xx, c, reach - sqrt_f32(best));
            }
        }
    }

    /// Random polylines (open, closed, lone points, degenerate), widths and rects — some wider
    /// than a chunk, some hanging over the row's ends, shifted and scissored: every row the walk
    /// paints equals the reference's, byte for byte.
    #[test]
    fn the_stroke_walk_is_byte_equal_to_asking_every_segment_at_every_pixel() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut painted = 0usize;
        for case in 0..400 {
            let paths: Vec<PathShape> = (0..1 + next(4))
                .map(|_| PathShape {
                    points: (0..1 + next(9))
                        .map(|_| pt(next(1001) as u16, next(1001) as u16))
                        .collect(),
                    closed: next(2) == 0,
                })
                .collect();
            let width = 40 + next(360) as i32;
            let rect = (
                next(width as u64 + 60) as f32 - 30.0 + next(4) as f32 * 0.25,
                next(40) as f32 + next(4) as f32 * 0.25,
                4.0 + next(300) as f32,
                4.0 + next(64) as f32,
            );
            let width_milli = 20 + next(200) as u16;
            let colour = Rgba8 { r: 240, g: 20, b: next(256) as u8, a: 128 + next(128) as u8 };
            let (shift, clip) = match case % 3 {
                0 => (0, None),
                1 => (next(20) as i32, None),
                _ => (next(20) as i32, Some((next(20) as i32, width - next(20) as i32))),
            };
            for y in 0..110 {
                let paint = |walk: bool| {
                    let mut buf = vec![7u8; width as usize * 4];
                    let mut canvas = RowCanvas::new(&mut buf, y, width);
                    (canvas.shift_x, canvas.clip_x) = (shift, clip);
                    if walk {
                        canvas.stroke_paths_row(&paths, rect, width_milli, colour);
                    } else {
                        stroke_reference(&mut canvas, &paths, rect, width_milli, colour);
                    }
                    buf
                };
                let (walk, reference) = (paint(true), paint(false));
                assert_eq!(walk, reference, "case {case} row {y}");
                painted += usize::from(walk.iter().any(|b| *b != 7));
            }
        }
        assert!(painted > 5_000, "the cases paint: {painted} rows");
    }

    /// Strokes outside the row, empty polylines and a lone point (a dot) are all bounded: only
    /// the dot paints, and only around itself.
    #[test]
    fn test_reject_strokes_beyond_the_row_and_degenerate_polylines() {
        let far = PathShape { points: vec![pt(0, 0), pt(1000, 0)], closed: false };
        let empty = PathShape { points: vec![], closed: true };
        let r =
            row(19, 20, |c| c.stroke_paths_row(&[far, empty], (0.0, 0.0, 20.0, 20.0), 100, WHITE));
        assert!(r.iter().all(|&v| v == 0), "{r:?}");
        let dot = PathShape { points: vec![pt(500, 500)], closed: false };
        let r = row(10, 20, |c| c.stroke_paths_row(&[dot], (0.0, 0.0, 20.0, 20.0), 100, WHITE));
        assert!(r[10] > 0 && r[3] == 0 && r[17] == 0, "{r:?}");
    }
}
