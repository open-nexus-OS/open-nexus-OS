// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Run METRICS: how wide a run is, and where it has to be cut.
//!
//! Split from the drawing module because these answer a different question.
//! `draw_text_row` blends one scanline and needs no notion of the run as a
//! whole; everything here measures the whole run and never touches a pixel.
//! Layout, wrapping and the ellipsis cut are all callers of this side. Both
//! sides place glyphs through the same [`Pen`], so a tracked run measures
//! exactly as wide as it paints.

use crate::ladder::face;
use crate::pen::{Pen, Tracking};
use crate::{resolve, FontSize};

/// Advance of a single glyph in pixels, its tracking included (no kerning) —
/// exactly what [`measure`] answers for a one-glyph run. A walker that adds
/// these up over a TRACKED run rounds once per glyph; measure the prefix
/// instead when the position has to match the painter to the pixel.
#[must_use]
pub fn advance(ch: char, size: FontSize, tracking: Tracking) -> u32 {
    let mut pen = Pen::new(face(size), tracking);
    pen.place(ch);
    pen.width().max(0) as u32
}

/// Width of a run in pixels: advances, kerning and `tracking` after every
/// glyph. Kerning only applies between two glyphs from the SAME face — a
/// pair straddling the fallback seam has no meaningful pair value.
pub fn measure(text: impl Iterator<Item = char>, size: FontSize, tracking: Tracking) -> u32 {
    let mut pen = Pen::new(face(size), tracking);
    for ch in text {
        pen.place(ch);
    }
    pen.width().max(0) as u32
}

/// The character a cut run ends with (`text-overflow: ellipsis` in the
/// design artifacts); baked into every face's EXTRAS tail so it never falls
/// back on a text face.
pub const ELLIPSIS: char = '\u{2026}';

/// How much of `text` fits in `max_w`, and whether it had to be cut.
///
/// Returns the number of CHARACTERS to draw and an ellipsis flag. When it
/// cuts, the returned prefix already leaves room for [`ELLIPSIS`]:
/// `measure(prefix + '…', size, tracking) <= max_w`, exactly — kerning into
/// the ellipsis and the tracking after it included.
///
/// This exists because the platform does not wrap: `layout_lines` always
/// yields exactly one line, so a run wider than its box was simply clipped
/// mid-glyph and a truncated label was indistinguishable from a short one.
#[must_use]
pub fn ellipsis_cut(text: &str, size: FontSize, tracking: Tracking, max_w: u32) -> (usize, bool) {
    if measure(text.chars(), size, tracking) <= max_w {
        return (text.chars().count(), false);
    }
    let primary = face(size);
    let (ef, egi) = resolve(primary, ELLIPSIS);
    let mut pen = Pen::new(primary, tracking);
    let mut n = 0usize;
    for ch in text.chars() {
        let mut next = pen;
        next.place(ch);
        let mut cut = next;
        cut.place_glyph(ef, egi);
        if i64::from(cut.width()) > i64::from(max_w) {
            break;
        }
        pen = next;
        n += 1;
    }
    (n, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    /// A run that fits is never touched; one that does not is cut short enough
    /// that the ellipsis still fits the same width. The second half is the
    /// property that matters: an ellipsis that overflows would be the very
    /// clipping it exists to announce.
    #[test]
    fn ellipsis_cut_marks_only_what_it_must() {
        for tracking in [Tracking::NONE, Tracking::em(10), Tracking::em(60), Tracking::em(-20)] {
            let size = FontSize::S11W500;
            let short = "WLAN";
            let full = measure(short.chars(), size, tracking);
            assert_eq!(ellipsis_cut(short, size, tracking, full), (4, false), "exact fit");
            assert_eq!(ellipsis_cut(short, size, tracking, full + 20), (4, false));

            let long = "Ansichtsmodus";
            let natural = measure(long.chars(), size, tracking);
            let budget = natural / 2;
            let (n, cut) = ellipsis_cut(long, size, tracking, budget);
            assert!(cut, "half the natural width must cut");
            assert!(n > 0 && n < long.chars().count(), "cut to {n} of {}", long.chars().count());
            let drawn: String = long.chars().take(n).chain(core::iter::once(ELLIPSIS)).collect();
            let w = measure(drawn.chars(), size, tracking);
            assert!(
                w <= budget,
                "{tracking:?}: the cut run PLUS its ellipsis ({w}px) must fit {budget}px"
            );
            // …and it is the LONGEST such prefix: one more char would not fit.
            let more: String = long.chars().take(n + 1).chain(core::iter::once(ELLIPSIS)).collect();
            assert!(measure(more.chars(), size, tracking) > budget, "{tracking:?}: cut too early");
        }
    }

    /// The ellipsis is a real baked glyph, not a placeholder box — it is in
    /// every text face's EXTRAS tail, so a cut label never ends in a grey
    /// square.
    #[test]
    fn the_ellipsis_glyph_is_baked() {
        // The digits faces carry no ellipsis: they fall back to the 14 px face for it.
        let digits = [FontSize::S64W300, FontSize::S128W300];
        for size in FontSize::ALL.into_iter().filter(|s| !digits.contains(s)) {
            let (f, _) = resolve(face(size), ELLIPSIS);
            assert_eq!(f.id, face(size).id, "{size:?} borrows its ellipsis from another face");
            assert!(advance(ELLIPSIS, size, Tracking::NONE) > 0, "no ellipsis advance at {size:?}");
        }
    }

    /// RFC-0116 tracking: `largeTitle` is −0.02 em at 30 px, −0.6 px a glyph,
    /// so a 10-glyph run is exactly 6 px shorter — accumulated as one
    /// fraction, not as ten rounded ones (which would be 10 × 1 px).
    #[test]
    fn tracking_accumulates_exactly() {
        let (size, run) = (FontSize::S30W700, "Handgloves");
        let plain = measure(run.chars(), size, Tracking::NONE);
        assert_eq!(measure(run.chars(), size, Tracking::em(-20)), plain - 6);
        // `overline`: +0.06 em at 11 px = +0.66 px a glyph → +6.6 → +7 px.
        let caps = "SETTINGS A";
        let plain = measure(caps.chars(), FontSize::S11W600, Tracking::NONE);
        assert_eq!(measure(caps.chars(), FontSize::S11W600, Tracking::em(60)), plain + 7);
        // 100 glyphs at `caption` (+0.01 em at 11 px = 0.11 px): +11, no drift.
        let long = "x".repeat(100);
        let plain = measure(long.chars(), FontSize::S11W500, Tracking::NONE);
        assert_eq!(measure(long.chars(), FontSize::S11W500, Tracking::em(10)), plain + 11);
        // A one-glyph run measures its advance, tracking included.
        for ch in ['H', 'i', ' ', '7'] {
            let one = measure(core::iter::once(ch), size, Tracking::em(-20));
            assert_eq!(advance(ch, size, Tracking::em(-20)), one, "{ch:?}");
        }
        // No tracking is no change at all.
        assert_eq!(advance('H', size, Tracking::NONE), advance('H', size, Tracking::default()));
    }
}
