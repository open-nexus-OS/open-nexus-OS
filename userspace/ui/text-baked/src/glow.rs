// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A blurred text shadow, one row at a time: the run's glyph coverage convolved with a small
//! Gaussian — a CSS text-shadow's blur radius r is a Gaussian of σ = r/2 — and blended in the
//! shadow's colour. Row-based like [`draw_text_row`](crate::draw_text_row): the glyph rows
//! around the target row are read from the atlas for every row, so no buffer outlives a call.
//! The label halo (RFC-0116 `labelShadow`, the design's glow behind a label on the wallpaper)
//! is the one user today.

use crate::ladder::face;
use crate::{blend_px, measure, FontSize, Pen, Tracking};

/// The Gaussian taps of a blur radius of 1 to 4 px (index r − 1), centre first, in 1/256: each
/// kernel mirrored around its centre sums to 256, and its length − 1 is how far it reaches.
const TAPS: [&[u32]; 4] =
    [&[202, 27], &[102, 62, 14, 1], &[68, 55, 28, 9, 2], &[50, 45, 31, 17, 7, 2, 1]];

/// The widest kernel, mirrored (reach 6 → 13 taps).
const MAX_KERNEL: usize = 13;

/// Columns the coverage accumulator holds: a wider run is blurred a tile at a time.
const TILE: usize = 512;

/// How far a text shadow of blur radius `blur` reaches beyond its glyphs, in pixels: 0 for a
/// sharp shadow; a radius above 4 px draws as 4.
#[must_use]
pub const fn blur_reach(blur: u32) -> u32 {
    match blur {
        0 => 0,
        r => (TAPS[(if r > 4 { 4 } else { r }) as usize - 1].len() - 1) as u32,
    }
}

/// Blend row `local_y` of the run `text` blurred by `blur` px into `row` (BGRA, the same
/// coverage-over-destination blend as [`draw_text_row`](crate::draw_text_row)). The run is
/// placed exactly as `draw_text_row` places it — band from row `top`, pen at column `x0`, the
/// caller adds the shadow's offset to both — and the blur reaches [`blur_reach`] pixels past
/// its glyphs in every direction, so rows that far above and below the band take coverage too.
/// Pixels at or beyond `clip_end_x` (and beyond the row) are not touched. Blur 0 draws the run
/// sharp.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_blur_row(
    row: &mut [u8],
    local_y: u32,
    top: i32,
    x0: i32,
    clip_end_x: u32,
    text: impl Iterator<Item = char> + Clone,
    size: FontSize,
    tracking: Tracking,
    color: [u8; 4],
    blur: u32,
) {
    if blur == 0 {
        crate::draw_text_row(row, local_y, top, x0, clip_end_x, text, size, tracking, color);
        return;
    }
    let taps = TAPS[blur.min(4) as usize - 1];
    let reach = (taps.len() - 1) as i32;
    let mut kernel = [0u32; MAX_KERNEL];
    for (d, &t) in taps.iter().enumerate() {
        kernel[reach as usize + d] = t;
        kernel[reach as usize - d] = t;
    }
    let kernel = &kernel[..2 * reach as usize + 1];
    let primary = face(size);
    let band_y = local_y as i32 - top;
    if band_y < -reach || band_y >= primary.line_h as i32 + reach {
        return;
    }
    let clip = clip_end_x.min((row.len() / 4) as u32) as i32;
    let width = measure(text.clone(), size, tracking) as i32;
    let (mut tile0, span1) = ((x0 - reach).max(0), x0.saturating_add(width + reach).min(clip));
    while tile0 < span1 {
        let tile1 = (tile0 + TILE as i32).min(span1);
        let mut acc = [0u32; TILE];
        let mut pen = Pen::new(primary, tracking);
        for ch in text.clone() {
            let (f, gi, x) = pen.place(ch);
            let Some(&(off, w, h, left, gtop, _)) = f.glyphs.get(gi) else { continue };
            let gx = x0.saturating_add(x).saturating_add(i32::from(left));
            if w == 0 || gx + i32::from(w) + reach <= tile0 || gx - reach >= tile1 {
                continue;
            }
            // The glyph row on the target row (a fallback face's baseline aligned as in
            // `draw_text_row`); the vertical taps read the rows around it.
            let gy0 = band_y - (gtop as i32 + primary.ascent - f.ascent);
            let cov = f.cov();
            for (dy, &kv) in (-reach..=reach).zip(kernel) {
                let gy = gy0 + dy;
                if gy < 0 || gy >= i32::from(h) {
                    continue;
                }
                let start = off as usize + gy as usize * w as usize;
                let Some(src) = cov.get(start..start + w as usize) else { continue };
                for (i, &c) in src.iter().enumerate() {
                    if c == 0 {
                        continue;
                    }
                    let weight = kv * u32::from(c);
                    let left = gx + i as i32 - reach;
                    for (k, &kh) in kernel.iter().enumerate() {
                        let px = left + k as i32;
                        if px >= tile0 && px < tile1 {
                            acc[(px - tile0) as usize] += weight * kh;
                        }
                    }
                }
            }
            // A glyph placed past the tile's blurred edge (bearings are a few pixels at most)
            // reaches nothing in it.
            if x0.saturating_add(pen.width()) >= tile1 + reach + 4 {
                break;
            }
        }
        // Σ taps² · coverage ≤ 65536 · 255: the shift brings it back to a coverage.
        for (i, &a) in acc[..(tile1 - tile0) as usize].iter().enumerate() {
            let coverage = ((a + (1 << 15)) >> 16).min(255) as u8;
            if coverage > 0 {
                let px = (tile0 + i as i32) as usize * 4;
                blend_px(&mut row[px..px + 4], color, coverage);
            }
        }
        tile0 = tile1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{draw_text_row, line_height};
    use alloc::vec::Vec;

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const SIZE: FontSize = FontSize::S13W400;

    /// The run drawn at `top` 8, pen 8, over `line_height + 16` rows of a transparent strip;
    /// blur `None` draws it sharp with `draw_text_row`.
    fn strip(text: &str, blur: Option<u32>) -> Vec<Vec<u8>> {
        let w = measure(text.chars(), SIZE, Tracking::NONE) + 16;
        (0..line_height(SIZE) + 16)
            .map(|y| {
                let mut row = alloc::vec![0u8; (w * 4) as usize];
                match blur {
                    Some(b) => draw_text_blur_row(
                        &mut row,
                        y,
                        8,
                        8,
                        w,
                        text.chars(),
                        SIZE,
                        Tracking::NONE,
                        WHITE,
                        b,
                    ),
                    None => draw_text_row(
                        &mut row,
                        y,
                        8,
                        8,
                        w,
                        text.chars(),
                        SIZE,
                        Tracking::NONE,
                        WHITE,
                    ),
                }
                row
            })
            .collect()
    }

    /// `(x0, y0, x1, y1)` of the lit pixels, inclusive.
    fn lit_box(rows: &[Vec<u8>]) -> (usize, usize, usize, usize) {
        let mut b = (usize::MAX, usize::MAX, 0, 0);
        for (y, row) in rows.iter().enumerate() {
            for (x, p) in row.chunks_exact(4).enumerate() {
                if p[3] > 0 {
                    b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                }
            }
        }
        b
    }

    #[test]
    fn the_reach_follows_the_kernel_and_clamps() {
        assert_eq!([0, 1, 2, 3, 4, 9].map(blur_reach), [0, 1, 3, 4, 6, 6]);
        for taps in TAPS {
            let sum: u32 = taps[0] + 2 * taps[1..].iter().sum::<u32>();
            assert_eq!(sum, 256, "a kernel keeps the coverage it blurs: {taps:?}");
        }
    }

    #[test]
    fn blur_zero_is_the_sharp_run() {
        assert_eq!(strip("Halo", Some(0)), strip("Halo", None));
    }

    /// The halo the board showed was a sharp copy: a blurred shadow spreads past the ink on every
    /// side — by more than a pixel and never past its reach — and is softer than the ink.
    #[test]
    fn a_blurred_shadow_spreads_past_the_ink_and_is_softer() {
        let (ink, glow) = (strip("Hil", None), strip("Hil", Some(3)));
        let (i, g) = (lit_box(&ink), lit_box(&glow));
        let reach = blur_reach(3) as usize;
        for (spread, side) in
            [(i.0 - g.0, "left"), (i.1 - g.1, "top"), (g.2 - i.2, "right"), (g.3 - i.3, "bottom")]
        {
            assert!((2..=reach).contains(&spread), "{side}: spreads {spread} px (reach {reach})");
        }
        let peak =
            |rows: &[Vec<u8>]| rows.iter().flat_map(|r| r.chunks_exact(4).map(|p| p[3])).max();
        assert_eq!(peak(&ink), Some(255));
        assert!(peak(&glow) < Some(200), "a thin stroke's glow stays soft: {:?}", peak(&glow));
    }

    #[test]
    fn the_glow_keeps_its_coverage_and_respects_the_clip() {
        let total = |rows: &[Vec<u8>]| -> u32 {
            rows.iter().flat_map(|r| r.chunks_exact(4).map(|p| u32::from(p[3]))).sum()
        };
        let (ink, glow) = (total(&strip("Mm", None)), total(&strip("Mm", Some(3))));
        // The kernel sums to one: blurring moves coverage, it does not add or lose much of it
        // (rounding the faint tail away loses a little).
        assert!(glow * 10 >= ink * 8 && glow * 10 <= ink * 11, "ink {ink}, glow {glow}");
        let mut row = alloc::vec![0u8; 64 * 4];
        for y in 0..line_height(SIZE) + 16 {
            draw_text_blur_row(&mut row, y, 8, 8, 20, "Mm".chars(), SIZE, Tracking::NONE, WHITE, 3);
        }
        assert!(row[20 * 4..].iter().all(|&b| b == 0), "nothing past the clip");
    }
}
