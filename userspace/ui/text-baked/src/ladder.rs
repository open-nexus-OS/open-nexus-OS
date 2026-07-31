// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The baked type LADDER (RFC-0082): which `(size, weight)` faces exist, how
//! a request resolves to one, and how a codepoint resolves to a glyph inside
//! it — including the cross-face fallback that keeps CJK rendering in a
//! Latin-only face.
//!
//! Kept in lock-step with `FACES` in `build.rs`: that table decides what is
//! baked, `LADDER` here decides what can be asked for.

use super::{atlas, baked};

/// Shell text sizes, mapping to the baked faces (RFC-0082 ladder). Pick one
/// with [`FontSize::nearest`] rather than by hand: the ladder is deliberately
/// sparse, and `nearest` is the documented total function from a requested
/// (px, weight) to a face that actually exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSize {
    /// 13 px Regular — chrome labels, dropdown entries, window titles.
    Small,
    /// 16 px Regular — window body text (chat messages, search rows, app bodies).
    Body,
    /// 13 px SemiBold — emphasized captions, switcher labels.
    SmallSemi,
    /// 16 px SemiBold — emphasized body, section labels, dates.
    BodySemi,
    /// 21 px Regular — the `xl` type step.
    Title,
    /// 21 px SemiBold — display names, titles.
    TitleSemi,
    /// 36 px SemiBold — the `display` step, avatar initials.
    DisplaySemi,
    /// 120 px Light — hero numerals (lock-screen clock). Digits only.
    Hero,
    /// 11 px Regular — the `xs` step: sub-labels, group labels, captions.
    Caption,
    /// 11 px SemiBold — emphasized captions (tile labels, pill counters).
    CaptionSemi,
    /// 44 px Regular — a `.textFit` rung (TASK-0314): the step between the
    /// 36 px display face and the 52 px one, so box-relative type actually
    /// changes size as its container grows.
    Fit44,
    /// 52 px Light — the calculator handoff's display numeral. Light because
    /// that is what the handoff specifies (`font-weight: 300`).
    Fit52Light,
}

/// Weight class the ladder can serve. Deliberately coarser than CSS: these
/// are the instances that are actually baked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Light,
    Regular,
    SemiBold,
}

/// Every baked face as `(size, px, weight)` — the ladder's SSOT on the read
/// side, kept in lock-step with `FACES` in `build.rs`.
const LADDER: [(FontSize, i32, Weight); 12] = [
    (FontSize::Small, 13, Weight::Regular),
    (FontSize::Body, 16, Weight::Regular),
    (FontSize::SmallSemi, 13, Weight::SemiBold),
    (FontSize::BodySemi, 16, Weight::SemiBold),
    (FontSize::Title, 21, Weight::Regular),
    (FontSize::TitleSemi, 21, Weight::SemiBold),
    (FontSize::DisplaySemi, 36, Weight::SemiBold),
    (FontSize::Hero, 120, Weight::Light),
    (FontSize::Caption, 11, Weight::Regular),
    (FontSize::CaptionSemi, 11, Weight::SemiBold),
    (FontSize::Fit44, 44, Weight::Regular),
    (FontSize::Fit52Light, 52, Weight::Light),
];

impl FontSize {
    /// The baked face for a requested size and weight (RFC-0082, normative).
    ///
    /// **Size wins over weight.** First the nearest baked px is chosen (ties
    /// round UP); only among the faces at that px does the requested weight
    /// get a say, falling back to Regular and then to whatever is there. The
    /// other order would answer `nearest(14, Light)` with the 120 px hero face
    /// — a size miss is far more visible than a weight miss.
    ///
    /// Ties round up because a tie means the request sits exactly between two
    /// rungs, and the larger one is the legible choice. It is also what keeps
    /// the ladder extensible: the `sm` step (12) landed on the new 11/13 tie
    /// the moment the caption rung was baked, and rounding down would have
    /// silently shrunk every 12 px label in every shipped app.
    /// The baked pixel size of this rung — the inverse of [`Self::nearest`].
    ///
    /// Callers that pick a size from a box (`.textFit`) need to know what they
    /// actually got: the request is continuous, the ladder is not, and a test
    /// asserting the REQUEST would happily pass while every size collapsed
    /// onto one face.
    #[must_use]
    pub fn px(self) -> i32 {
        let mut i = 0;
        while i < LADDER.len() {
            if matches!(
                (LADDER[i].0, self),
                (FontSize::Small, FontSize::Small)
                    | (FontSize::Body, FontSize::Body)
                    | (FontSize::SmallSemi, FontSize::SmallSemi)
                    | (FontSize::BodySemi, FontSize::BodySemi)
                    | (FontSize::Title, FontSize::Title)
                    | (FontSize::TitleSemi, FontSize::TitleSemi)
                    | (FontSize::DisplaySemi, FontSize::DisplaySemi)
                    | (FontSize::Hero, FontSize::Hero)
                    | (FontSize::Caption, FontSize::Caption)
                    | (FontSize::CaptionSemi, FontSize::CaptionSemi)
                    | (FontSize::Fit44, FontSize::Fit44)
                    | (FontSize::Fit52Light, FontSize::Fit52Light)
            ) {
                return LADDER[i].1;
            }
            i += 1;
        }
        16
    }

    #[must_use]
    pub fn nearest(px: i32, weight: Weight) -> FontSize {
        let mut best_px = LADDER[0].1;
        let mut i = 0;
        while i < LADDER.len() {
            let cand = LADDER[i].1;
            let (d_new, d_old) = ((cand - px).abs(), (best_px - px).abs());
            if d_new < d_old || (d_new == d_old && cand > best_px) {
                best_px = cand;
            }
            i += 1;
        }
        let at_px = |w: Weight| {
            LADDER.iter().find(|&&(_, p, fw)| p == best_px && fw == w).map(|&(size, _, _)| size)
        };
        at_px(weight)
            .or_else(|| at_px(Weight::Regular))
            .or_else(|| LADDER.iter().find(|&&(_, p, _)| p == best_px).map(|&(size, _, _)| size))
            .unwrap_or(FontSize::Body)
    }
}

/// One baked face: coverage blob + per-glyph placement + line metrics.
pub(super) struct Face {
    /// Stable identity. The faces are `const` items, so two `&FACE` values do
    /// NOT reliably compare equal by address (const promotion may duplicate
    /// them) — identity has to be a value, not a pointer. Used to decide
    /// whether a glyph pair sits in the same face and may be kerned.
    pub(super) id: u8,
    /// This face's coverage span within the shared atlas
    /// (`atlas()[cov_off .. cov_off + cov_len]`, RFC-0080). Per-glyph `off` is
    /// 0-based within this slice.
    pub(super) cov_off: usize,
    pub(super) cov_len: usize,
    /// `(cov_offset, w, h, left_bearing, top_from_band_top, advance_px)` per
    /// charset glyph: dense ASCII 32..=126 first (index = code − 32), then
    /// the sparse EXTRAS tail (umlauts/ß + math symbols), then the sorted
    /// WIDE tail (kana/jamo/hangul/han — RFC-0075 Phase 8d, see build.rs).
    pub(super) glyphs: &'static [(u32, u16, u16, i16, i16, u16)],
    /// The sparse tail's codepoints, sorted ascending; glyph index = 95 + i.
    /// A SLICE, not a fixed array: the tail grows whenever the UI needs a
    /// non-ASCII glyph (the breadcrumb separator was the last one), and a
    /// baked-in length turned that one-line charset edit into eight type
    /// errors.
    pub(super) extras: &'static [u32],
    /// The WIDE tail's codepoints, sorted ascending; glyph index =
    /// 95 + extras.len() + i. Unkerned by design.
    pub(super) wide: &'static [u32],
    /// Sparse kerning: `(left_glyph_idx, right_glyph_idx, px)` — Latin span
    /// only (indices < 107 fit the u8 table). Empty on a `Digits` face
    /// (tabular figures must not shift).
    pub(super) kern: &'static [(u8, u8, i8)],
    pub(super) line_h: u32,
    pub(super) avg_advance: u32,
    pub(super) ascent: i32,
    /// Where a codepoint outside THIS face's charset is looked up (RFC-0082):
    /// the `Latin`/`Digits` faces point at the 16 px `Full` face, so CJK in a
    /// large run renders small instead of blank. `None` on the `Full` faces —
    /// they are the end of the chain.
    pub(super) fallback: Option<&'static Face>,
}

impl Face {
    /// This face's coverage slice out of the installed atlas backing.
    #[inline]
    pub(super) fn cov(&self) -> &'static [u8] {
        atlas().get(self.cov_off..self.cov_off + self.cov_len).unwrap_or(&[])
    }
}

const SMALL: Face = Face {
    id: 0,
    cov_off: baked::FONT13_COV_OFFSET,
    cov_len: baked::FONT13_COV_LEN,
    glyphs: baked::FONT13_GLYPHS,
    extras: baked::FONT13_EXTRAS,
    wide: baked::FONT13_WIDE,
    kern: baked::FONT13_KERN,
    line_h: baked::FONT13_LINE_H,
    avg_advance: baked::FONT13_AVG_ADVANCE,
    ascent: baked::FONT13_ASCENT,
    fallback: None,
};

pub(super) const BODY: Face = Face {
    id: 1,
    cov_off: baked::FONT16_COV_OFFSET,
    cov_len: baked::FONT16_COV_LEN,
    glyphs: baked::FONT16_GLYPHS,
    extras: baked::FONT16_EXTRAS,
    wide: baked::FONT16_WIDE,
    kern: baked::FONT16_KERN,
    line_h: baked::FONT16_LINE_H,
    avg_advance: baked::FONT16_AVG_ADVANCE,
    ascent: baked::FONT16_ASCENT,
    fallback: None,
};

const SMALL_SEMI: Face = Face {
    id: 2,
    cov_off: baked::FONT13_SEMI_COV_OFFSET,
    cov_len: baked::FONT13_SEMI_COV_LEN,
    glyphs: baked::FONT13_SEMI_GLYPHS,
    extras: baked::FONT13_SEMI_EXTRAS,
    wide: baked::FONT13_SEMI_WIDE,
    kern: baked::FONT13_SEMI_KERN,
    line_h: baked::FONT13_SEMI_LINE_H,
    avg_advance: baked::FONT13_SEMI_AVG_ADVANCE,
    ascent: baked::FONT13_SEMI_ASCENT,
    fallback: Some(&BODY),
};

const BODY_SEMI: Face = Face {
    id: 3,
    cov_off: baked::FONT16_SEMI_COV_OFFSET,
    cov_len: baked::FONT16_SEMI_COV_LEN,
    glyphs: baked::FONT16_SEMI_GLYPHS,
    extras: baked::FONT16_SEMI_EXTRAS,
    wide: baked::FONT16_SEMI_WIDE,
    kern: baked::FONT16_SEMI_KERN,
    line_h: baked::FONT16_SEMI_LINE_H,
    avg_advance: baked::FONT16_SEMI_AVG_ADVANCE,
    ascent: baked::FONT16_SEMI_ASCENT,
    fallback: Some(&BODY),
};

const TITLE: Face = Face {
    id: 4,
    cov_off: baked::FONT21_COV_OFFSET,
    cov_len: baked::FONT21_COV_LEN,
    glyphs: baked::FONT21_GLYPHS,
    extras: baked::FONT21_EXTRAS,
    wide: baked::FONT21_WIDE,
    kern: baked::FONT21_KERN,
    line_h: baked::FONT21_LINE_H,
    avg_advance: baked::FONT21_AVG_ADVANCE,
    ascent: baked::FONT21_ASCENT,
    fallback: Some(&BODY),
};

const TITLE_SEMI: Face = Face {
    id: 5,
    cov_off: baked::FONT21_SEMI_COV_OFFSET,
    cov_len: baked::FONT21_SEMI_COV_LEN,
    glyphs: baked::FONT21_SEMI_GLYPHS,
    extras: baked::FONT21_SEMI_EXTRAS,
    wide: baked::FONT21_SEMI_WIDE,
    kern: baked::FONT21_SEMI_KERN,
    line_h: baked::FONT21_SEMI_LINE_H,
    avg_advance: baked::FONT21_SEMI_AVG_ADVANCE,
    ascent: baked::FONT21_SEMI_ASCENT,
    fallback: Some(&BODY),
};

const DISPLAY_SEMI: Face = Face {
    id: 6,
    cov_off: baked::FONT36_SEMI_COV_OFFSET,
    cov_len: baked::FONT36_SEMI_COV_LEN,
    glyphs: baked::FONT36_SEMI_GLYPHS,
    extras: baked::FONT36_SEMI_EXTRAS,
    wide: baked::FONT36_SEMI_WIDE,
    kern: baked::FONT36_SEMI_KERN,
    line_h: baked::FONT36_SEMI_LINE_H,
    avg_advance: baked::FONT36_SEMI_AVG_ADVANCE,
    ascent: baked::FONT36_SEMI_ASCENT,
    fallback: Some(&BODY),
};

const CAPTION: Face = Face {
    id: 8,
    cov_off: baked::FONT11_COV_OFFSET,
    cov_len: baked::FONT11_COV_LEN,
    glyphs: baked::FONT11_GLYPHS,
    extras: baked::FONT11_EXTRAS,
    wide: baked::FONT11_WIDE,
    kern: baked::FONT11_KERN,
    line_h: baked::FONT11_LINE_H,
    avg_advance: baked::FONT11_AVG_ADVANCE,
    ascent: baked::FONT11_ASCENT,
    fallback: Some(&BODY),
};

const CAPTION_SEMI: Face = Face {
    id: 9,
    cov_off: baked::FONT11_SEMI_COV_OFFSET,
    cov_len: baked::FONT11_SEMI_COV_LEN,
    glyphs: baked::FONT11_SEMI_GLYPHS,
    extras: baked::FONT11_SEMI_EXTRAS,
    wide: baked::FONT11_SEMI_WIDE,
    kern: baked::FONT11_SEMI_KERN,
    line_h: baked::FONT11_SEMI_LINE_H,
    avg_advance: baked::FONT11_SEMI_AVG_ADVANCE,
    ascent: baked::FONT11_SEMI_ASCENT,
    fallback: Some(&BODY),
};

pub(super) const HERO: Face = Face {
    id: 7,
    cov_off: baked::FONT120_LIGHT_COV_OFFSET,
    cov_len: baked::FONT120_LIGHT_COV_LEN,
    glyphs: baked::FONT120_LIGHT_GLYPHS,
    extras: baked::FONT120_LIGHT_EXTRAS,
    wide: baked::FONT120_LIGHT_WIDE,
    kern: baked::FONT120_LIGHT_KERN,
    line_h: baked::FONT120_LIGHT_LINE_H,
    avg_advance: baked::FONT120_LIGHT_AVG_ADVANCE,
    ascent: baked::FONT120_LIGHT_ASCENT,
    fallback: Some(&BODY),
};

const FIT44: Face = Face {
    id: 10,
    cov_off: baked::FONT44_COV_OFFSET,
    cov_len: baked::FONT44_COV_LEN,
    glyphs: baked::FONT44_GLYPHS,
    extras: baked::FONT44_EXTRAS,
    wide: baked::FONT44_WIDE,
    kern: baked::FONT44_KERN,
    line_h: baked::FONT44_LINE_H,
    avg_advance: baked::FONT44_AVG_ADVANCE,
    ascent: baked::FONT44_ASCENT,
    fallback: Some(&BODY),
};

const FIT52_LIGHT: Face = Face {
    id: 11,
    cov_off: baked::FONT52_LIGHT_COV_OFFSET,
    cov_len: baked::FONT52_LIGHT_COV_LEN,
    glyphs: baked::FONT52_LIGHT_GLYPHS,
    extras: baked::FONT52_LIGHT_EXTRAS,
    wide: baked::FONT52_LIGHT_WIDE,
    kern: baked::FONT52_LIGHT_KERN,
    line_h: baked::FONT52_LIGHT_LINE_H,
    avg_advance: baked::FONT52_LIGHT_AVG_ADVANCE,
    ascent: baked::FONT52_LIGHT_ASCENT,
    fallback: Some(&BODY),
};

pub(super) const fn face(size: FontSize) -> &'static Face {
    match size {
        FontSize::Small => &SMALL,
        FontSize::Body => &BODY,
        FontSize::SmallSemi => &SMALL_SEMI,
        FontSize::BodySemi => &BODY_SEMI,
        FontSize::Title => &TITLE,
        FontSize::TitleSemi => &TITLE_SEMI,
        FontSize::DisplaySemi => &DISPLAY_SEMI,
        FontSize::Hero => &HERO,
        FontSize::Caption => &CAPTION,
        FontSize::CaptionSemi => &CAPTION_SEMI,
        FontSize::Fit44 => &FIT44,
        FontSize::Fit52Light => &FIT52_LIGHT,
    }
}
