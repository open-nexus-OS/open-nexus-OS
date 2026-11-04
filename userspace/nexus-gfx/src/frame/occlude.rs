// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Occlusion for the board's scene passes (TASK-0335 P2, P4, TASK-0337): an OPAQUE
//! layer (`LAYER_OPAQUE`, full opacity — the content pane) hides everything below it inside its
//! rounded rectangle, and so does a glass whose blurred backdrop is drawn back (its cache rows
//! are opaque and written over what lies below), so the draws below it are cut to the pieces
//! around it: a rectangle minus one occluder is at most four bands (above, below, left, right)
//! — bounded and deterministic. One occluder a draw: the largest opaque rectangle above it that
//! touches it; a draw wholly under it is not drawn at all. The pieces are exact for one-to-one
//! textures (the atlas); a glass's reduced cache rows are cut on their scale's grid, the
//! occluder shrunk inward to it, so a piece's texels start on a texel.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below; `tests/frame_board.rs`, `tests/frame_occlusion.rs` (the
//!   pieces a frame draws)
//! INVARIANTS: the pieces of a rectangle are disjoint and their union is the rectangle minus
//!   the occluder (exactly, or — on a grid — minus a part of the occluder, never more); a
//!   translucent layer without a drawn glass back occludes nothing.

use super::layers::FrameLayer;

/// A rectangle in pixels: x, y, width, height.
pub type Px = (u32, u32, u32, u32);

/// Whether two rectangles share a pixel.
pub fn intersects(a: Px, b: Px) -> bool {
    a.2 > 0
        && a.3 > 0
        && b.2 > 0
        && b.3 > 0
        && a.0 < b.0 + b.2
        && b.0 < a.0 + a.2
        && a.1 < b.1 + b.3
        && b.1 < a.1 + a.3
}

/// Whether `occ` holds every pixel of `r`.
pub fn covers(occ: Px, r: Px) -> bool {
    r.2 == 0
        || r.3 == 0
        || (occ.0 <= r.0
            && occ.1 <= r.1
            && occ.0 + occ.2 >= r.0 + r.2
            && occ.1 + occ.3 >= r.1 + r.3)
}

/// The pixels of `r`.
pub fn area(r: Px) -> u64 {
    u64::from(r.2) * u64::from(r.3)
}

/// The pixels `a` and `b` share, `None` when they share none.
pub fn intersection(a: Px, b: Px) -> Option<Px> {
    if !intersects(a, b) {
        return None;
    }
    let (x0, y0) = (a.0.max(b.0), a.1.max(b.1));
    let (x1, y1) = ((a.0 + a.2).min(b.0 + b.2), (a.1 + a.3).min(b.1 + b.3));
    Some((x0, y0, x1 - x0, y1 - y0))
}

/// The rectangle a layer hides everything under: its rectangle inset by its corner radius
/// (the corners are cut, nothing hides there) — for a layer drawn at full opacity that is
/// opaque inside its rounded rectangle, or whose glass back is drawn in the pass (`back`: its
/// blurred backdrop, opaque, is written over what lies below it).
pub fn opaque_rect(l: &FrameLayer, back: bool) -> Option<Px> {
    if !(l.opaque || back) || l.opacity < 255 || l.width == 0 || l.height == 0 {
        return None;
    }
    let r = l.corner_radius.min(l.width / 2).min(l.height / 2);
    let (w, h) = (l.width.saturating_sub(2 * r), l.height.saturating_sub(2 * r));
    (w > 0 && h > 0).then_some((l.dst_x + r, l.dst_y + r, w, h))
}

/// The cross of a rounded rectangle of corner `radius`: its middle column (full height, the
/// corner squares' width cut off both sides) and its middle row (full width, the corner squares'
/// height cut off top and bottom) — every pixel of both lies inside the rounded rectangle at
/// coverage one, so an opaque body hides everything under its cross (the compositor's culling,
/// TASK-0351 P1: tighter than [`opaque_rect`], which keeps one band inset all around). `None`
/// parts when the rectangle is too small for them.
pub fn cross(r: Px, radius: u32) -> [Option<Px>; 2] {
    let some = |p: Px| (p.2 > 0 && p.3 > 0).then_some(p);
    if r.2 == 0 || r.3 == 0 {
        return [None, None];
    }
    let c = radius.min(r.2 / 2).min(r.3 / 2);
    [
        some((r.0 + c, r.1, r.2.saturating_sub(2 * c), r.3)),
        some((r.0, r.1 + c, r.2, r.3.saturating_sub(2 * c))),
    ]
}

/// `r` minus `occ`: the band above the occluder, the band below it, the band to its left and
/// the band to its right (each the full width or the occluder's rows), the empty ones `None`.
/// Without an occluder, or one that misses `r`, the rectangle itself.
pub fn cut(r: Px, occ: Option<Px>) -> [Option<Px>; 4] {
    let some = |p: Px| (p.2 > 0 && p.3 > 0).then_some(p);
    let Some(o) = occ.filter(|&o| intersects(r, o)) else {
        return [some(r), None, None, None];
    };
    let (x0, y0, x1, y1) = (r.0, r.1, r.0 + r.2, r.1 + r.3);
    let (ox0, oy0, ox1, oy1) = (o.0.max(x0), o.1.max(y0), (o.0 + o.2).min(x1), (o.1 + o.3).min(y1));
    [
        some((x0, y0, r.2, oy0 - y0)),
        some((x0, oy1, r.2, y1 - oy1)),
        some((x0, oy0, ox0 - x0, oy1 - oy0)),
        some((ox1, oy0, x1 - ox1, oy1 - oy0)),
    ]
}

/// How many pieces `cut` leaves.
pub fn count(r: Px, occ: Option<Px>) -> usize {
    cut(r, occ).iter().flatten().count()
}

/// Piece `k` of `cut`.
pub fn piece(r: Px, occ: Option<Px>, k: usize) -> Option<Px> {
    cut(r, occ).into_iter().flatten().nth(k)
}

/// The occluder shrunk inward to the grid of `scale` pixels from `origin` (a glass's cache
/// texels): the pieces around it then start and end on texels. `None` when nothing is left.
pub fn on_grid(occ: Px, origin: (u32, u32), scale: u32) -> Option<Px> {
    let s = scale.max(1);
    let up = |v: u32, o: u32| o + (v.saturating_sub(o)).div_ceil(s) * s;
    let down = |v: u32, o: u32| o + (v.saturating_sub(o)) / s * s;
    let (x0, y0) = (up(occ.0, origin.0), up(occ.1, origin.1));
    let (x1, y1) = (down(occ.0 + occ.2, origin.0), down(occ.1 + occ.3, origin.1));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// Pieces of a rectangle kept apart while the occluders are taken off it: past this the
/// coverage test gives up and says `false` (never a hole — an uncertain window is drawn).
pub const COVER_PIECES: usize = 64;

/// Whether the union of `occluders` holds every pixel of `r` (the compositor's culling, TASK-0351
/// P1): `r` is taken apart occluder by occluder — each piece minus the occluder is its at most
/// four bands — until nothing is left (`true`) or every occluder is spent (`false`). Exact for
/// any arrangement of rectangles; bounded by [`COVER_PIECES`] pieces, past which it is `false`.
pub fn covered(r: Px, occluders: &[Px]) -> bool {
    if r.2 == 0 || r.3 == 0 {
        return true;
    }
    let mut pieces = [None::<Px>; COVER_PIECES];
    pieces[0] = Some(r);
    let mut n = 1;
    for &o in occluders {
        if o.2 == 0 || o.3 == 0 {
            continue;
        }
        let mut next = [None::<Px>; COVER_PIECES];
        let mut m = 0;
        for p in pieces[..n].iter().flatten() {
            for band in cut(*p, Some(o)).into_iter().flatten() {
                if m == COVER_PIECES {
                    return false;
                }
                next[m] = Some(band);
                m += 1;
            }
        }
        pieces = next;
        n = m;
        if n == 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_s_cross_leaves_exactly_its_corner_squares_uncovered() {
        let r = (100, 100, 800, 600);
        let [col, row] = cross(r, 12);
        assert_eq!(col, Some((112, 100, 776, 600)));
        assert_eq!(row, Some((100, 112, 800, 576)));
        let cross_rects: alloc::vec::Vec<Px> = [col, row].into_iter().flatten().collect();
        assert!(!covered(r, &cross_rects), "the corners are not in the cross");
        let corners =
            [(100, 100, 12, 12), (888, 100, 12, 12), (100, 688, 12, 12), (888, 688, 12, 12)];
        let mut all = cross_rects.clone();
        all.extend(corners);
        assert!(covered(r, &all), "cross and corner squares make the rectangle");
        // A window well inside the body's cross is covered by it.
        assert!(covered((200, 200, 300, 200), &cross_rects));
        // A radius past the half side leaves no cross.
        assert_eq!(cross((0, 0, 10, 10), 5), [None, None]);
        assert_eq!(cross(r, 0), [Some(r), Some(r)]);
    }

    #[test]
    fn a_rectangle_under_two_halves_is_covered_and_under_one_is_not() {
        let r = (100, 100, 800, 600);
        assert!(covered(r, &[(100, 100, 400, 600), (500, 100, 400, 600)]));
        assert!(covered(r, &[(0, 0, 2000, 2000)]), "one bigger occluder");
        assert!(covered(r, &[r]), "itself");
        assert!(!covered(r, &[(100, 100, 400, 600)]), "half of it shows");
        assert!(!covered(r, &[]), "nothing above it");
        assert!(covered((5, 5, 0, 9), &[]), "an empty rectangle shows nothing");
    }

    #[test]
    fn test_reject_coverage_with_a_gap() {
        let r = (0, 0, 100, 100);
        // Four quarters that leave a one-pixel column between the left and the right ones.
        let occ = [(0, 0, 49, 50), (50, 0, 50, 50), (0, 50, 49, 50), (50, 50, 50, 50)];
        assert!(!covered(r, &occ));
        let closed = [(0, 0, 50, 50), (50, 0, 50, 50), (0, 50, 50, 50), (50, 50, 50, 50)];
        assert!(covered(r, &closed));
        // Rounded corners: the body's own inset is the occluder — its corners stay uncovered.
        assert!(!covered(r, &[(8, 8, 84, 84)]));
        // Past the piece bound the answer is `false`, never a hole.
        let many: alloc::vec::Vec<Px> =
            (0..COVER_PIECES as u32).map(|i| (i * 3, i * 3, 1, 1)).collect();
        assert!(!covered(r, &many));
    }

    #[test]
    fn a_rectangle_minus_an_occluder_is_its_four_bands_disjoint_and_complete() {
        let r = (100, 100, 800, 600);
        let o = (300, 250, 200, 100);
        let p = cut(r, Some(o));
        assert_eq!(p[0], Some((100, 100, 800, 150)));
        assert_eq!(p[1], Some((100, 350, 800, 350)));
        assert_eq!(p[2], Some((100, 250, 200, 100)));
        assert_eq!(p[3], Some((500, 250, 400, 100)));
        let sum: u64 = p.iter().flatten().map(|&q| area(q)).sum();
        assert_eq!(sum, area(r) - area(o));
        for (i, a) in p.iter().flatten().enumerate() {
            for b in p.iter().flatten().skip(i + 1) {
                assert!(!intersects(*a, *b), "pieces overlap: {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn an_occluder_at_an_edge_leaves_fewer_bands_and_one_covering_leaves_none() {
        assert_eq!(count((0, 0, 100, 100), Some((0, 0, 100, 40))), 1);
        assert_eq!(count((0, 0, 100, 100), Some((0, 0, 100, 100))), 0);
        assert_eq!(count((0, 0, 100, 100), Some((0, 0, 200, 200))), 0);
        assert_eq!(count((0, 0, 100, 100), Some((200, 200, 10, 10))), 1);
        assert_eq!(count((0, 0, 100, 100), None), 1);
        assert_eq!(piece((0, 0, 100, 100), Some((20, 20, 60, 60)), 3), Some((80, 20, 20, 60)));
    }

    #[test]
    fn test_reject_a_translucent_or_rounded_corner_as_an_occluder() {
        let mut l =
            FrameLayer { dst_x: 10, dst_y: 10, width: 100, height: 80, ..Default::default() };
        assert_eq!(opaque_rect(&l, false), None, "a layer without the flag hides nothing");
        l.opaque = true;
        l.opacity = 200;
        assert_eq!(opaque_rect(&l, false), None, "a faded layer shows what is below");
        l.opacity = 255;
        assert_eq!(opaque_rect(&l, false), Some((10, 10, 100, 80)));
        l.corner_radius = 12;
        assert_eq!(opaque_rect(&l, false), Some((22, 22, 76, 56)), "the corners are not claimed");
        l.corner_radius = 50;
        assert_eq!(opaque_rect(&l, false), None, "a pill hides no full row");
    }

    #[test]
    fn a_drawn_glass_back_hides_what_is_below_and_test_reject_a_glass_without_it() {
        let mut l = FrameLayer {
            dst_x: 10,
            dst_y: 10,
            width: 100,
            height: 80,
            corner_radius: 12,
            backdrop_blur: 40,
            opacity: 255,
            ..Default::default()
        };
        assert_eq!(opaque_rect(&l, false), None, "a glass whose back is not drawn hides nothing");
        assert_eq!(opaque_rect(&l, true), Some((22, 22, 76, 56)));
        l.opacity = 200;
        assert_eq!(opaque_rect(&l, true), None, "a faded glass hides nothing");
    }

    #[test]
    fn two_rectangles_share_their_overlap_and_nothing_apart() {
        assert_eq!(intersection((0, 0, 100, 100), (50, 60, 100, 100)), Some((50, 60, 50, 40)));
        assert_eq!(intersection((0, 0, 100, 100), (100, 0, 10, 10)), None);
        assert_eq!(intersection((10, 10, 5, 5), (0, 0, 100, 100)), Some((10, 10, 5, 5)));
    }

    #[test]
    fn the_grid_shrinks_an_occluder_inward_to_its_texels() {
        assert_eq!(on_grid((103, 201, 100, 100), (100, 200), 4), Some((104, 204, 96, 96)));
        assert_eq!(on_grid((100, 200, 8, 8), (100, 200), 4), Some((100, 200, 8, 8)));
        assert_eq!(on_grid((101, 201, 3, 3), (100, 200), 4), None);
    }
}
