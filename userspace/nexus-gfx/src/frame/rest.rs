// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A glass drawn from the rows of its REST rectangle (RFC-0109, TASK-0354): the planner
//! keeps a glass's cache rows for the rectangle its layer has at rest (`Glass.at`, its window's
//! transform not applied), and a write blurs that rectangle; the layer is drawn where its
//! transform puts it. [`texels`] names the cache texels a piece of the drawn glass back shows: at
//! the rest size the integers the back always had (one texel per `s` pixels from the margin), at
//! any other size the piece's linear image in the rest rectangle, rounded outward and held
//! inside the slot's own result — the columns `[1, 1 + at.w.div_ceil(s))` and the rows `[row + 1,
//! row + 1 + at.h.div_ceil(s))`. The board's texture descriptor clamps to the rectangle it is
//! given, so a stretched glass never samples a neighbour's rows nor past its plane. Pure.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (the rest size is the old integers; a stretched piece holds
//!   its linear image and stays inside the slot); `tests/frame_rest.rs` (through the board, with
//!   transforms)
//! INVARIANTS: a texel rectangle is never empty and never leaves the slot's result rows; a rest
//!   rectangle without area (a zero-width layer a scale draws one pixel wide) shows the slot's
//!   first texel inside the margin — its rows hold two, both its own.

use super::board::Tex;
use super::occlude::Px;

/// The texels `[t0, t0 + n)` that the drawn pixels `[off, off + len)` of a side `drawn` long
/// show when the side is spread over a rest side `at` long, `s` pixels a texel: the drawn pixel
/// `o` is the rest pixel `o · at / drawn`, the texel `o · at / (drawn · s)` — rounded outward,
/// inside `[0, at.div_ceil(s))`, at least one texel.
fn span(off: u32, len: u32, drawn: u32, at: u32, s: u32) -> (u32, u32) {
    let n = at.div_ceil(s).max(1);
    let den = u64::from(drawn.max(1)) * u64::from(s);
    let lo = u64::from(off) * u64::from(at) / den;
    let hi = (u64::from(off.saturating_add(len)) * u64::from(at)).div_ceil(den);
    let t0 = u32::try_from(lo).unwrap_or(u32::MAX).min(n - 1);
    let t1 = u32::try_from(hi).unwrap_or(u32::MAX).clamp(t0 + 1, n);
    (t0, t1 - t0)
}

/// The cache texels piece `p` of a glass back shows: the back drawn over `drawn` from the rows of
/// a rest rectangle of size `at`, `s` pixels a texel, its result rows starting at `row` (the
/// slot's first row — the margin row; the result begins one texel in). At the rest size the
/// back's own integers; else the piece's linear image (`span`).
pub(super) fn texels(drawn: Px, at: (u32, u32), s: u32, row: u32, p: Px) -> Tex {
    let (x, y, w, h) = drawn;
    let s = s.max(1);
    let (dx, dy) = (p.0.saturating_sub(x), p.1.saturating_sub(y));
    let ((c, cw), (r, rh)) = if (w, h) == at {
        ((dx / s, p.2.div_ceil(s)), (dy / s, p.3.div_ceil(s)))
    } else {
        (span(dx, p.2, w, at.0, s), span(dy, p.3, h, at.1, s))
    };
    Tex::Cache { x: 1 + c, row: row + 1 + r, w: cw, h: rh }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(t: Tex) -> (u32, u32, u32, u32) {
        match t {
            Tex::Cache { x, row, w, h } => (x, row, w, h),
            other => panic!("not a cache texture: {other:?}"),
        }
    }

    #[test]
    fn at_its_rest_size_a_piece_shows_the_texels_it_always_did() {
        let drawn = (100, 50, 400, 200);
        let pieces: [(Px, u32); 3] =
            [((100, 50, 400, 200), 4), ((120, 70, 360, 160), 4), ((480, 50, 20, 20), 2)];
        for (p, s) in pieces {
            let want = (1 + (p.0 - 100) / s, 8 + (p.1 - 50) / s, p.2.div_ceil(s), p.3.div_ceil(s));
            assert_eq!(cache(texels(drawn, (400, 200), s, 7, p)), want, "{p:?} at scale {s}");
        }
    }

    #[test]
    fn a_stretched_piece_holds_its_linear_image_inside_the_slot() {
        // Drawn 96 % and 112 % of a 352 x 566 rest rectangle, cut in odd pieces.
        let at = (352u32, 566u32);
        for (w, h) in [(338u32, 543u32), (394, 634), (352, 600), (300, 566)] {
            for s in [2u32, 4] {
                let (n, m) = (at.0.div_ceil(s), at.1.div_ceil(s));
                for (ox, len) in [(0u32, w), (0, 13), (w / 3, w / 3), (w - 7, 7), (17, w - 17)] {
                    let p = (40 + ox, 40, len, h);
                    let (tx, row, tw, th) = cache(texels((40, 40, w, h), at, s, 100, p));
                    assert!(tw >= 1 && th >= 1);
                    assert!(tx >= 1 && tx + tw <= 1 + n, "{w}x{h} s{s}: columns {tx}+{tw} of {n}");
                    assert!(row >= 101 && row + th <= 101 + m, "{w}x{h} s{s}: rows {row}+{th}");
                    // It holds the exact image of the piece: [ox, ox + len) · at / (w · s).
                    let k = f64::from(at.0) / (f64::from(w) * f64::from(s));
                    let (e0, e1) = (f64::from(ox) * k, f64::from(ox + len) * k);
                    assert!(f64::from(tx - 1) <= e0 + 1e-9 && e1 <= f64::from(tx - 1 + tw) + 1e-9);
                }
            }
        }
    }

    #[test]
    fn test_reject_a_texel_rectangle_past_a_tiny_rest_rectangle() {
        // A rest rectangle of one pixel drawn 40 wide: one texel, the slot's own.
        assert_eq!(cache(texels((0, 0, 40, 40), (1, 1), 4, 0, (0, 0, 40, 40))), (1, 1, 1, 1));
        assert_eq!(cache(texels((0, 0, 40, 40), (1, 1), 4, 0, (39, 39, 1, 1))), (1, 1, 1, 1));
        // Without area (`glass_rows(0, s)` = 2 rows of 2 texels): the slot's own texel (1, 1).
        assert_eq!(cache(texels((5, 5, 1, 1), (0, 0), 4, 9, (5, 5, 1, 1))), (1, 10, 1, 1));
    }
}
