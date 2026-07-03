// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime text rendering from the build-time A8 glyph atlases of the
//! vendored UI face (TASK-0070 Phase 6) — replaces the 5×7 bitmap font for all
//! dynamic surface text (chat messages, search rows, labels, titles). Lives at
//! the crate root (like `bitmap_font` before it) because the pure logic is
//! host-tested while `compositor` is OS-only.
//!
//! The API is ROW-BASED to match the surface renderers: every shell surface is
//! painted one pixel row at a time, so [`draw_text_row`] blends exactly the
//! slice of a text run that intersects the current row. A text run occupies
//! the band `top .. top + line_height(size)` with its baseline `ascent(size)`
//! pixels below the band top; rows outside the band return immediately.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable

use crate::assets;

/// Shell text sizes, mapping to the two baked atlases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontSize {
    /// 13 px — chrome labels, dropdown entries, window titles.
    Small,
    /// 16 px — window body text (chat messages, search rows, greeter name).
    Body,
}

/// One baked face: coverage blob + per-glyph placement + line metrics.
struct Face {
    cov: &'static [u8],
    /// `(cov_offset, w, h, left_bearing, top_from_band_top, advance_px)` per
    /// ASCII glyph 32..=126 (see build.rs `emit_glyph_atlas`).
    glyphs: &'static [(u32, u16, u16, i16, i16, u16); 95],
    /// Sparse kerning: `(left_glyph_idx, right_glyph_idx, px)`.
    kern: &'static [(u8, u8, i8)],
    line_h: u32,
    avg_advance: u32,
}

const SMALL: Face = Face {
    cov: assets::FONT13_COV,
    glyphs: assets::FONT13_GLYPHS,
    kern: assets::FONT13_KERN,
    line_h: assets::FONT13_LINE_H,
    avg_advance: assets::FONT13_AVG_ADVANCE,
};

const BODY: Face = Face {
    cov: assets::FONT16_COV,
    glyphs: assets::FONT16_GLYPHS,
    kern: assets::FONT16_KERN,
    line_h: assets::FONT16_LINE_H,
    avg_advance: assets::FONT16_AVG_ADVANCE,
};

const fn face(size: FontSize) -> &'static Face {
    match size {
        FontSize::Small => &SMALL,
        FontSize::Body => &BODY,
    }
}

/// Height of a text band (ascent + descent, no extra leading — callers add
/// their own line spacing).
pub(crate) const fn line_height(size: FontSize) -> u32 {
    face(size).line_h
}

/// Average glyph advance — the wrap heuristic for the interim char-count
/// wrapping (measured wrapping lands with the Phase-7 layout unification).
pub(crate) const fn avg_advance(size: FontSize) -> u32 {
    face(size).avg_advance
}

/// Glyph index for a char; non-ASCII falls back to `?` (same policy as the
/// old bitmap font's replacement glyph).
#[inline]
fn glyph_index(ch: char) -> usize {
    let c = ch as u32;
    if (32..=126).contains(&c) {
        (c - 32) as usize
    } else {
        ('?' as u32 - 32) as usize
    }
}

#[inline]
fn kern(f: &Face, left: usize, right: usize) -> i32 {
    for &(l, r, k) in f.kern {
        if l as usize == left && r as usize == right {
            return k as i32;
        }
    }
    0
}

/// Advance width of a run in pixels (kerning included).
pub(crate) fn measure(text: impl Iterator<Item = char>, size: FontSize) -> u32 {
    let f = face(size);
    let mut w = 0i32;
    let mut prev: Option<usize> = None;
    for ch in text {
        let gi = glyph_index(ch);
        if let Some(p) = prev {
            w += kern(f, p, gi);
        }
        w += f.glyphs[gi].5 as i32;
        prev = Some(gi);
    }
    w.max(0) as u32
}

/// Blend the slice of the run `text` that intersects surface row `local_y`
/// into `row` (BGRA, straight alpha). The run's band starts at row `top`
/// (`i32`: a band may start above the surface when partially scrolled off),
/// the pen at column `x0`; pixels at or beyond `clip_end_x` (and beyond the
/// row buffer) are not touched. Glyph coverage is blended `src OVER dst`
/// scaled by `color`'s alpha — text composites correctly over glass tints.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_text_row(
    row: &mut [u8],
    local_y: u32,
    top: i32,
    x0: u32,
    clip_end_x: u32,
    text: impl Iterator<Item = char>,
    size: FontSize,
    color: [u8; 4],
) {
    let f = face(size);
    let band_y = local_y as i32 - top;
    if band_y < 0 || band_y >= f.line_h as i32 {
        return;
    }
    let row_px = (row.len() / 4) as u32;
    let clip = clip_end_x.min(row_px);
    let mut pen = x0 as i32;
    let mut prev: Option<usize> = None;
    for ch in text {
        let gi = glyph_index(ch);
        if let Some(p) = prev {
            pen += kern(f, p, gi);
        }
        let (off, w, h, left, gtop, adv) = f.glyphs[gi];
        let gy = band_y - gtop as i32;
        if w > 0 && gy >= 0 && (gy as u16) < h {
            let start = off as usize + gy as usize * w as usize;
            if let Some(src) = f.cov.get(start..start + w as usize) {
                for (i, &cov) in src.iter().enumerate() {
                    if cov == 0 {
                        continue;
                    }
                    let px = pen + left as i32 + i as i32;
                    if px < 0 {
                        continue;
                    }
                    let px = px as u32;
                    if px >= clip {
                        break;
                    }
                    blend_px(&mut row[px as usize * 4..px as usize * 4 + 4], color, cov);
                }
            }
        }
        pen += adv as i32;
        prev = Some(gi);
        if pen >= clip as i32 {
            break;
        }
    }
}

/// Blend `color` scaled by `coverage` over one straight-alpha BGRA pixel.
#[inline]
fn blend_px(dst: &mut [u8], color: [u8; 4], coverage: u8) {
    let sa = (coverage as u32 * color[3] as u32 + 127) / 255; // 0..255
    if sa == 0 {
        return;
    }
    let inv = 255 - sa;
    for c in 0..3 {
        dst[c] = ((color[c] as u32 * sa + dst[c] as u32 * inv + 127) / 255) as u8;
    }
    dst[3] = (sa + dst[3] as u32 * inv / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn draw_band(text: &str, size: FontSize, w: u32) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
        let mut rows = alloc::vec::Vec::new();
        for y in 0..line_height(size) + 4 {
            let mut row = alloc::vec![0u8; (w * 4) as usize];
            draw_text_row(&mut row, y, 2, 0, w, text.chars(), size, WHITE);
            rows.push(row);
        }
        rows
    }

    fn row_lit(row: &[u8]) -> usize {
        row.chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn draws_inside_band_only() {
        let rows = draw_band("Hello", FontSize::Body, 100);
        assert_eq!(row_lit(&rows[0]), 0, "above the band (top=2) stays untouched");
        assert_eq!(row_lit(&rows[1]), 0);
        let lit: usize = rows.iter().map(|r| row_lit(r)).sum();
        assert!(lit > 40, "a 5-glyph run lights a substantial pixel count, got {lit}");
    }

    #[test]
    fn measure_is_monotonic_and_positive() {
        let a = measure("hi".chars(), FontSize::Small);
        let b = measure("hi there".chars(), FontSize::Small);
        assert!(a > 0);
        assert!(b > a, "longer text measures wider ({a} vs {b})");
    }

    #[test]
    fn clip_end_is_respected() {
        let size = FontSize::Body;
        for y in 0..line_height(size) {
            let mut row = alloc::vec![0u8; 100 * 4];
            draw_text_row(&mut row, y, 0, 0, 20, "wwwwwwwwww".chars(), size, WHITE);
            for (x, px) in row.chunks_exact(4).enumerate() {
                assert!(x < 20 || px[3] == 0, "pixel {x} written past clip_end_x=20");
            }
        }
    }

    #[test]
    fn non_ascii_falls_back_without_panic() {
        let w = measure("a—b".chars(), FontSize::Body); // em-dash → '?'
        assert!(w > measure("ab".chars(), FontSize::Body));
        let rows = draw_band("—…—", FontSize::Body, 80);
        assert!(rows.iter().map(|r| row_lit(r)).sum::<usize>() > 0);
    }

    #[test]
    fn body_line_height_is_sane_for_chat_rows() {
        let lh = line_height(FontSize::Body);
        assert!((16..=24).contains(&lh), "16px face line height plausible, got {lh}");
        let lh_s = line_height(FontSize::Small);
        assert!((13..=20).contains(&lh_s), "13px face line height plausible, got {lh_s}");
    }
}
