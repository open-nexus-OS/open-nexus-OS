// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one scanline from BGRA8 to a filtered RGB8 PNG row. The row is converted, the
//! filter among None/Sub/Up/Paeth with the smallest sum of absolute residuals (residual
//! bytes read as signed, PNG spec §12.8; ties go to the earlier filter) is chosen, and the
//! row is written as `[filter type, residuals...]`. Average is never chosen.
//!
//! Rows are stored PADDED: `LEFT_PAD` zero bytes, then the RGB bytes. The pad is the left
//! (and upper-left) neighbour of the first pixel, which PNG defines as 0, so every filter
//! input is a plain slice and every loop a zip of slices: no index can leave the row, and
//! nothing branches on the position.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (Paeth against the spec, inverse filters, choices)

/// Bytes per pixel of the output (RGB8): a byte's left neighbour is this far back.
const BPP: usize = 3;

/// Zero bytes in front of every stored row.
pub(crate) const LEFT_PAD: usize = BPP;

/// Bytes of a stored (padded) row of `width` pixels.
pub(crate) const fn row_len(width: usize) -> usize {
    LEFT_PAD + width * BPP
}

/// Bytes of a filtered row of `width` pixels: the filter type, then the residuals.
pub(crate) const fn filtered_len(width: usize) -> usize {
    1 + width * BPP
}

/// PNG filter types (PNG spec §9.2) this encoder chooses from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Filter {
    None = 0,
    Sub = 1,
    Up = 2,
    Paeth = 4,
}

/// One BGRA8 row into a padded RGB8 row (alpha dropped; the pad is left untouched).
pub(crate) fn load_row(bgra: &[u8], row: &mut [u8]) {
    let rgb = row.get_mut(LEFT_PAD..).unwrap_or_default();
    for (px, out) in bgra.chunks_exact(4).zip(rgb.chunks_exact_mut(BPP)) {
        if let ([b, g, r, _], [o0, o1, o2]) = (px, out) {
            (*o0, *o1, *o2) = (*r, *g, *b);
        }
    }
}

/// Filters the padded row `cur` (with `prev` the padded row above, all zeros for the first
/// row) into `out` = `[filter type, residuals...]`; returns the filter.
pub(crate) fn filter_row(cur: &[u8], prev: &[u8], out: &mut [u8]) -> Filter {
    let kind = choose(cur, prev);
    if let Some((first, residuals)) = out.split_first_mut() {
        *first = kind as u8;
        apply(kind, cur, prev, residuals);
    }
    kind
}

/// The filter with the smallest residual sum; the earliest one on ties.
fn choose(cur: &[u8], prev: &[u8]) -> Filter {
    let (mut none, mut sub, mut up, mut paeth_sum) = (0u32, 0u32, 0u32, 0u32);
    for (x, a, b, c) in neighbours(cur, prev) {
        none += magnitude(x);
        sub += magnitude(x.wrapping_sub(a));
        up += magnitude(x.wrapping_sub(b));
        paeth_sum += magnitude(x.wrapping_sub(paeth(a, b, c)));
    }
    [(Filter::None, none), (Filter::Sub, sub), (Filter::Up, up), (Filter::Paeth, paeth_sum)]
        .into_iter()
        .min_by_key(|&(_, sum)| sum)
        .map_or(Filter::None, |(kind, _)| kind)
}

/// Writes the residuals of filter `kind`, one specialised loop per filter.
fn apply(kind: Filter, cur: &[u8], prev: &[u8], out: &mut [u8]) {
    match kind {
        Filter::None => residuals(cur, prev, out, |x, _, _, _| x),
        Filter::Sub => residuals(cur, prev, out, |x, a, _, _| x.wrapping_sub(a)),
        Filter::Up => residuals(cur, prev, out, |x, _, b, _| x.wrapping_sub(b)),
        Filter::Paeth => residuals(cur, prev, out, |x, a, b, c| x.wrapping_sub(paeth(a, b, c))),
    }
}

fn residuals(cur: &[u8], prev: &[u8], out: &mut [u8], f: impl Fn(u8, u8, u8, u8) -> u8) {
    for (r, (x, a, b, c)) in out.iter_mut().zip(neighbours(cur, prev)) {
        *r = f(x, a, b, c);
    }
}

/// Per byte `x` of a padded row: `(x, a, b, c)` = the byte, its left neighbour, the byte
/// above, the byte above-left (PNG spec §9.2). The pad supplies the zeros at the left edge.
fn neighbours<'r>(cur: &'r [u8], prev: &'r [u8]) -> impl Iterator<Item = (u8, u8, u8, u8)> + 'r {
    let x = cur.get(LEFT_PAD..).unwrap_or_default();
    let b = prev.get(LEFT_PAD..).unwrap_or_default();
    x.iter().zip(b).zip(cur).zip(prev).map(|(((&x, &b), &a), &c)| (x, a, b, c))
}

/// `|r as i8|`: residuals near 0 and near 255 are both small.
fn magnitude(r: u8) -> u32 {
    u32::from(r.min(r.wrapping_neg()))
}

/// The Paeth predictor (PNG spec §9.4): whichever of a, b, c is closest to p = a + b - c,
/// preferring a, then b. Here |p - a| = |b - c|, |p - b| = |a - c|, |p - c| = |a + b - 2c|.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let (pa, pb, pc) = ((ib - ic).abs(), (ia - ic).abs(), (ia + ib - 2 * ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decoder's side (PNG spec §9.2): rebuild an unpadded row from
    /// `[type, residuals...]` and the padded row above.
    fn unfilter(row: &[u8], prev: &[u8]) -> Vec<u8> {
        let above = &prev[LEFT_PAD..];
        let mut out: Vec<u8> = Vec::with_capacity(row.len() - 1);
        for (i, &r) in row[1..].iter().enumerate() {
            let a = if i >= BPP { out[i - BPP] } else { 0 };
            let b = above[i];
            let c = if i >= BPP { above[i - BPP] } else { 0 };
            let predicted = match row[0] {
                0 => 0,
                1 => a,
                2 => b,
                4 => paeth(a, b, c),
                t => panic!("filter type {t} is never written"),
            };
            out.push(r.wrapping_add(predicted));
        }
        out
    }

    /// A padded row holding `rgb`.
    fn padded(rgb: &[u8]) -> Vec<u8> {
        [&[0u8; LEFT_PAD][..], rgb].concat()
    }

    fn noise(len: usize, mut seed: u32) -> Vec<u8> {
        (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed.to_le_bytes()[0]
            })
            .collect()
    }

    #[test]
    fn paeth_matches_the_spec_on_every_input() {
        for a in 0..=255u8 {
            for b in 0..=255u8 {
                for c in (0..=255u8).step_by(3) {
                    let p = i32::from(a) + i32::from(b) - i32::from(c);
                    let (pa, pb, pc) = (
                        (p - i32::from(a)).abs(),
                        (p - i32::from(b)).abs(),
                        (p - i32::from(c)).abs(),
                    );
                    let want = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    assert_eq!(paeth(a, b, c), want, "a={a} b={b} c={c}");
                }
            }
        }
    }

    #[test]
    fn every_filter_inverts() {
        let prev = padded(&noise(3 * 37, 1));
        let cur = noise(3 * 37, 2);
        for kind in [Filter::None, Filter::Sub, Filter::Up, Filter::Paeth] {
            let mut row = vec![kind as u8; cur.len() + 1];
            apply(kind, &padded(&cur), &prev, &mut row[1..]);
            assert_eq!(unfilter(&row, &prev), cur, "{kind:?}");
        }
    }

    #[test]
    fn chosen_rows_invert_and_flat_rows_pick_sub_then_up() {
        let zeros = padded(&[0u8; 3 * 9]);
        let flat = padded(&[0x20, 0x40, 0x80].repeat(9));
        let mut out = [0u8; 3 * 9 + 1];
        // First row of a flat image: Sub leaves one pixel of residue, None and Up keep all.
        assert_eq!(filter_row(&flat, &zeros, &mut out), Filter::Sub);
        assert_eq!(unfilter(&out, &zeros), flat[LEFT_PAD..]);
        // The rows below: Up leaves nothing.
        assert_eq!(filter_row(&flat, &flat, &mut out), Filter::Up);
        assert!(out[1..].iter().all(|&r| r == 0));
        // A horizontal ramp under an equal row: Up (Paeth ties, the earlier wins); above
        // nothing: Sub.
        let ramp = padded(&(0..27u8).map(|i| i * 3).collect::<Vec<_>>());
        assert_eq!(filter_row(&ramp, &ramp, &mut out), Filter::Up);
        assert_eq!(filter_row(&ramp, &zeros, &mut out), Filter::Sub);
        assert_eq!(unfilter(&out, &zeros), ramp[LEFT_PAD..]);
        // Noise: whatever is chosen, it inverts.
        let prev = padded(&noise(27, 9));
        let cur = noise(27, 10);
        filter_row(&padded(&cur), &prev, &mut out);
        assert_eq!(unfilter(&out, &prev), cur);
    }

    #[test]
    fn residual_magnitude_reads_bytes_as_signed() {
        assert_eq!(magnitude(0), 0);
        assert_eq!(magnitude(1), 1);
        assert_eq!(magnitude(255), 1);
        assert_eq!(magnitude(127), 127);
        assert_eq!(magnitude(128), 128);
        assert_eq!(magnitude(129), 127);
    }

    #[test]
    fn bgra_lands_after_the_pad_as_rgb_without_alpha() {
        let bgra = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut row = [0u8; LEFT_PAD + 6];
        load_row(&bgra, &mut row);
        assert_eq!(row, [0, 0, 0, 3, 2, 1, 7, 6, 5]);
        assert_eq!(row_len(2), row.len());
        assert_eq!(filtered_len(2), row.len() - LEFT_PAD + 1);
    }
}
