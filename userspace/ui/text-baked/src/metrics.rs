// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Run METRICS: how wide a run is, and where it has to be cut.
//!
//! Split from the drawing module because these answer a different question.
//! `draw_text_row` blends one scanline and needs no notion of the run as a
//! whole; everything here measures the whole run and never touches a pixel.
//! Layout, wrapping and the ellipsis cut are all callers of this side.

use super::{kern, resolve};
use crate::ladder::{face, Face};
use crate::FontSize;

/// Advance of a single glyph in pixels (no kerning) — the unit the wrap
/// walker accumulates (kerning at these sizes rounds to ≤1px per pair and
/// the renderer clips, so wrap and paint cannot visibly drift).
#[must_use]
pub fn advance(ch: char, size: FontSize) -> u32 {
    let (f, gi) = resolve(face(size), ch);
    f.glyphs.get(gi).map_or(0, |g| g.5 as u32)
}

/// Advance width of a run in pixels (kerning included). Kerning only applies
/// between two glyphs from the SAME face — a pair straddling the fallback
/// seam has no meaningful pair value.
pub fn measure(text: impl Iterator<Item = char>, size: FontSize) -> u32 {
    let primary = face(size);
    let mut w = 0i32;
    let mut prev: Option<(&'static Face, usize)> = None;
    for ch in text {
        let (f, gi) = resolve(primary, ch);
        if let Some((pf, p)) = prev {
            if pf.id == f.id {
                w += kern(f, p, gi);
            }
        }
        w += f.glyphs.get(gi).map_or(0, |g| g.5 as i32);
        prev = Some((f, gi));
    }
    w.max(0) as u32
}

/// The character a cut run ends with. `text-overflow: ellipsis` in the design
/// handoffs; baked into every face's EXTRAS tail so it never falls back.
pub const ELLIPSIS: char = '\u{2026}';

/// How much of `text` fits in `max_w`, and whether it had to be cut.
///
/// Returns the number of CHARACTERS to draw and an ellipsis flag. When it
/// cuts, the returned prefix already leaves room for [`ELLIPSIS`], so
/// `prefix + '…'` fits `max_w`.
///
/// This exists because the platform does not wrap: `layout_lines` always
/// yields exactly one line, so a run wider than its box was simply clipped
/// mid-glyph and a truncated label was indistinguishable from a short one.
#[must_use]
pub fn ellipsis_cut(text: &str, size: FontSize, max_w: u32) -> (usize, bool) {
    let full = measure(text.chars(), size);
    if full <= max_w {
        return (text.chars().count(), false);
    }
    let budget = max_w.saturating_sub(advance(ELLIPSIS, size));
    let primary = face(size);
    let (mut w, mut n) = (0i32, 0usize);
    let mut prev: Option<(&'static Face, usize)> = None;
    for ch in text.chars() {
        let (f, gi) = resolve(primary, ch);
        let mut step = f.glyphs.get(gi).map_or(0, |g| g.5 as i32);
        if let Some((pf, p)) = prev {
            if pf.id == f.id {
                step += kern(f, p, gi);
            }
        }
        if w + step > budget as i32 {
            break;
        }
        w += step;
        n += 1;
        prev = Some((f, gi));
    }
    (n, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ellipsis_cut_marks_only_what_it_must() {
        let size = FontSize::Caption;
        let short = "WLAN";
        let full = measure(short.chars(), size);
        assert_eq!(ellipsis_cut(short, size, full), (4, false), "an exact fit is not cut");
        assert_eq!(ellipsis_cut(short, size, full + 20), (4, false));

        let long = "Ansichtsmodus";
        let natural = measure(long.chars(), size);
        let budget = natural / 2;
        let (n, cut) = ellipsis_cut(long, size, budget);
        assert!(cut, "half the natural width must cut");
        assert!(n > 0 && n < long.chars().count(), "cut to {n} of {}", long.chars().count());
        let drawn: String = long.chars().take(n).chain(core::iter::once(ELLIPSIS)).collect();
        assert!(
            measure(drawn.chars(), size) <= budget,
            "the cut run PLUS its ellipsis ({}px) must fit {budget}px",
            measure(drawn.chars(), size)
        );
    }

    /// The ellipsis is a real baked glyph, not a placeholder box — it is in
    /// every face's EXTRAS tail, so a cut label never ends in a grey square.
    #[test]
    fn the_ellipsis_glyph_is_baked() {
        for size in [FontSize::Caption, FontSize::Small, FontSize::Body] {
            assert!(advance(ELLIPSIS, size) > 0, "no ellipsis advance at {size:?}");
        }
    }
}
