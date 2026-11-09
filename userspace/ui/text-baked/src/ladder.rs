// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The baked type LADDER: which `(size, weight)` faces exist — the design's
//! text styles (RFC-0116) on the atlas mechanics of RFC-0082 — how a request
//! resolves to one, and the face record the reader walks.
//!
//! What is baked is decided by `FACES` in `build.rs`, which also writes every
//! face record (`baked::S14W400`, …). This side only names them: a record
//! without a variant here is dead code (a warning, so a gate failure), a
//! variant without a record does not compile, and the `const` check below
//! proves the names and the records are one to one.

use super::{atlas, baked};

/// The baked faces, named by size and weight: `S14W400` is 14 px at weight
/// 400. One per text style at its default weight and one per weight override
/// the apps use (RFC-0116) — there are no other sizes. Resolve a request with
/// [`FontSize::exact`] or [`FontSize::nearest`] rather than by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontSize {
    /// 10 px Medium — `caption2`.
    S10W500,
    /// 10 px Bold — `caption2` + bold.
    S10W700,
    /// 11 px Medium — `caption`.
    S11W500,
    /// 11 px SemiBold — `overline`, `caption` + semibold.
    S11W600,
    /// 12 px Regular — `footnote`.
    S12W400,
    /// 12 px Medium — `footnote` + medium.
    S12W500,
    /// 12 px SemiBold — `footnote` + semibold.
    S12W600,
    /// 13 px Regular, with the CJK tail — `subheadline`.
    S13W400,
    /// 13 px Medium — `subheadline` + medium.
    S13W500,
    /// 13 px SemiBold — `subheadline` + semibold.
    S13W600,
    /// 14 px Regular, with the CJK tail — `body`, and the face every other
    /// face falls back to for a codepoint it does not carry.
    S14W400,
    /// 14 px Medium — `body` + medium (the md control label).
    S14W500,
    /// 14 px SemiBold — `body` + semibold.
    S14W600,
    /// 15 px SemiBold — `headline`.
    S15W600,
    /// 17 px SemiBold — `title3`.
    S17W600,
    /// 20 px Regular — `title2` + normal.
    S20W400,
    /// 20 px Medium — `title2` + medium.
    S20W500,
    /// 20 px SemiBold — `title2`.
    S20W600,
    /// 24 px Bold — `title1`.
    S24W700,
    /// 30 px Bold — `largeTitle`.
    S30W700,
    /// 64 px Light, digits only (tabular: `0-9 : , . − - + × ÷ % =` and the
    /// space) — `display`: the calculator display and the phone's lock clock.
    /// Anything else falls back to the 14 px face.
    S64W300,
    /// 128 px Light, digits only (tabular, the same charset) — `clock`: the lock
    /// screen's time.
    S128W300,
}

/// The weights the ladder bakes — the five a text style or its override can
/// name (`light`, `normal`, `medium`, `semibold`, `bold`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    /// 300.
    Light,
    /// 400.
    Regular,
    /// 500.
    Medium,
    /// 600.
    SemiBold,
    /// 700.
    Bold,
}

impl Weight {
    /// The `wght` axis coordinate (the CSS `font-weight`).
    const fn wght(self) -> i32 {
        match self {
            Weight::Light => 300,
            Weight::Regular => 400,
            Weight::Medium => 500,
            Weight::SemiBold => 600,
            Weight::Bold => 700,
        }
    }
}

impl FontSize {
    /// Every baked face, by size then weight. Its length IS the number of
    /// faces `build.rs` bakes: a face added there without a name here does
    /// not compile.
    pub const ALL: [FontSize; baked::FACE_COUNT] = [
        FontSize::S10W500,
        FontSize::S10W700,
        FontSize::S11W500,
        FontSize::S11W600,
        FontSize::S12W400,
        FontSize::S12W500,
        FontSize::S12W600,
        FontSize::S13W400,
        FontSize::S13W500,
        FontSize::S13W600,
        FontSize::S14W400,
        FontSize::S14W500,
        FontSize::S14W600,
        FontSize::S15W600,
        FontSize::S17W600,
        FontSize::S20W400,
        FontSize::S20W500,
        FontSize::S20W600,
        FontSize::S24W700,
        FontSize::S30W700,
        FontSize::S64W300,
        FontSize::S128W300,
    ];

    /// The baked pixel size — what a caller actually got.
    ///
    /// Callers that pick a size from a box (`.textFit`) need to know it: the
    /// request is continuous, the ladder is not, and a test asserting the
    /// REQUEST would happily pass while every size collapsed onto one face.
    #[must_use]
    pub const fn px(self) -> i32 {
        face(self).px
    }

    /// The baked weight.
    #[must_use]
    pub const fn weight(self) -> Weight {
        face(self).weight
    }

    /// The face baked at exactly `px` and `weight`, if there is one. Every
    /// text style's size and weight — default or override — has one.
    #[must_use]
    pub fn exact(px: i32, weight: Weight) -> Option<FontSize> {
        Self::ALL.into_iter().find(|s| s.px() == px && s.weight() == weight)
    }

    /// The baked face for any requested size and weight — the total function
    /// every painter and the layout route through (normative).
    ///
    /// **Size wins over weight.** The size is `px` itself when a face is baked
    /// there, else the largest baked size BELOW it (the smallest face under the
    /// smallest size): text never draws larger than the box that asked for it —
    /// a `.textFit` target or a line box sized for `px` holds the glyphs it
    /// gets. Only then does the weight get a say: the nearest weight baked at
    /// that size, a tie going to the heavier face. The other order would answer
    /// a 14 px Light request with the 64 px display face — a size miss is far
    /// more visible than a weight miss.
    #[must_use]
    pub fn nearest(px: i32, weight: Weight) -> FontSize {
        let smallest = Self::ALL.into_iter().map(FontSize::px).min().unwrap_or(14);
        let size =
            Self::ALL.into_iter().map(FontSize::px).filter(|&p| p <= px).max().unwrap_or(smallest);
        Self::ALL
            .into_iter()
            .filter(|s| s.px() == size)
            .min_by_key(|s| (s.weight().wght().abs_diff(weight.wght()), -s.weight().wght()))
            .unwrap_or(FontSize::S14W400)
    }
}

/// One baked face: coverage span + per-glyph placement + line metrics. The
/// records are generated by `build.rs` (`baked::S14W400`, …).
pub(crate) struct Face {
    /// Stable identity (the face's index in `build.rs` `FACES`). The faces
    /// are `const` items, so two `&FACE` values do NOT reliably compare equal
    /// by address (const promotion may duplicate them) — identity has to be a
    /// value, not a pointer. Used to decide whether a glyph pair sits in the
    /// same face and may be kerned.
    pub(crate) id: u8,
    /// The baked pixel size — also the em that tracking is measured in.
    pub(crate) px: i32,
    pub(crate) weight: Weight,
    /// This face's coverage span within the shared atlas
    /// (`atlas()[cov_off .. cov_off + cov_len]`, RFC-0080). Per-glyph `off` is
    /// 0-based within this slice.
    pub(crate) cov_off: usize,
    pub(crate) cov_len: usize,
    /// `(cov_offset, w, h, left_bearing, top_from_band_top, advance_px)` per
    /// charset glyph: dense ASCII 32..=126 first (index = code − 32), then
    /// the sparse EXTRAS tail (umlauts/ß + math symbols), then the sorted
    /// WIDE tail (kana/jamo/hangul/han — RFC-0075 Phase 8d, see build.rs).
    pub(crate) glyphs: &'static [(u32, u16, u16, i16, i16, u16)],
    /// The sparse tail's codepoints, sorted ascending; glyph index = 95 + i.
    /// A SLICE, not a fixed array: the tail grows whenever the UI needs a
    /// non-ASCII glyph, and a baked-in length turned that one-line charset
    /// edit into a page of type errors.
    pub(crate) extras: &'static [u32],
    /// The WIDE tail's codepoints, sorted ascending; glyph index =
    /// 95 + extras.len() + i. Unkerned by design.
    pub(crate) wide: &'static [u32],
    /// Sparse kerning: `(left_glyph_idx, right_glyph_idx, px)` — Latin span
    /// only (indices < 109 fit the u8 table). Empty on a `Digits` face
    /// (tabular figures must not shift).
    pub(crate) kern: &'static [(u8, u8, i8)],
    pub(crate) line_h: u32,
    pub(crate) avg_advance: u32,
    pub(crate) ascent: i32,
    /// Where a codepoint outside THIS face's charset is looked up: the
    /// `Latin`/`Digits` faces point at the 14 px `Full` face, so CJK in a
    /// title renders small instead of blank. `None` on the `Full` faces —
    /// they are the end of the chain.
    pub(crate) fallback: Option<&'static Face>,
}

impl Face {
    /// This face's coverage slice out of the installed atlas backing.
    #[inline]
    pub(crate) fn cov(&self) -> &'static [u8] {
        atlas().get(self.cov_off..self.cov_off + self.cov_len).unwrap_or(&[])
    }
}

pub(crate) const fn face(size: FontSize) -> &'static Face {
    match size {
        FontSize::S10W500 => &baked::S10W500,
        FontSize::S10W700 => &baked::S10W700,
        FontSize::S11W500 => &baked::S11W500,
        FontSize::S11W600 => &baked::S11W600,
        FontSize::S12W400 => &baked::S12W400,
        FontSize::S12W500 => &baked::S12W500,
        FontSize::S12W600 => &baked::S12W600,
        FontSize::S13W400 => &baked::S13W400,
        FontSize::S13W500 => &baked::S13W500,
        FontSize::S13W600 => &baked::S13W600,
        FontSize::S14W400 => &baked::S14W400,
        FontSize::S14W500 => &baked::S14W500,
        FontSize::S14W600 => &baked::S14W600,
        FontSize::S15W600 => &baked::S15W600,
        FontSize::S17W600 => &baked::S17W600,
        FontSize::S20W400 => &baked::S20W400,
        FontSize::S20W500 => &baked::S20W500,
        FontSize::S20W600 => &baked::S20W600,
        FontSize::S24W700 => &baked::S24W700,
        FontSize::S30W700 => &baked::S30W700,
        FontSize::S64W300 => &baked::S64W300,
        FontSize::S128W300 => &baked::S128W300,
    }
}

// Names and records are one to one: `ALL` (as long as `build.rs` bakes faces)
// lists every variant once, in declaration order, and no two variants name
// the same record.
const _: () = {
    let mut i = 0;
    while i < FontSize::ALL.len() {
        assert!(FontSize::ALL[i] as usize == i, "FontSize::ALL skips or repeats a variant");
        let mut j = i + 1;
        while j < FontSize::ALL.len() {
            assert!(face(FontSize::ALL[i]).id != face(FontSize::ALL[j]).id, "one face, two names");
            j += 1;
        }
        i += 1;
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// The 13 text styles of RFC-0116 at their default weights, and the
    /// weight overrides the apps use.
    const STYLES: [(&str, i32, Weight); 13] = [
        ("clock", 128, Weight::Light),
        ("display", 64, Weight::Light),
        ("largeTitle", 30, Weight::Bold),
        ("title1", 24, Weight::Bold),
        ("title2", 20, Weight::SemiBold),
        ("title3", 17, Weight::SemiBold),
        ("headline", 15, Weight::SemiBold),
        ("body", 14, Weight::Regular),
        ("subheadline", 13, Weight::Regular),
        ("footnote", 12, Weight::Regular),
        ("caption", 11, Weight::Medium),
        ("caption2", 10, Weight::Medium),
        ("overline", 11, Weight::SemiBold),
    ];
    const OVERRIDES: [(&str, i32, Weight); 9] = [
        ("caption2 + bold", 10, Weight::Bold),
        ("footnote + medium", 12, Weight::Medium),
        ("footnote + semibold", 12, Weight::SemiBold),
        ("subheadline + medium", 13, Weight::Medium),
        ("subheadline + semibold", 13, Weight::SemiBold),
        ("body + medium", 14, Weight::Medium),
        ("body + semibold", 14, Weight::SemiBold),
        ("title2 + normal", 20, Weight::Regular),
        ("title2 + medium", 20, Weight::Medium),
    ];

    #[test]
    fn every_text_style_resolves_to_an_exact_face() {
        for (name, px, weight) in STYLES.into_iter().chain(OVERRIDES) {
            let exact = FontSize::exact(px, weight);
            assert!(exact.is_some(), "{name} ({px} px, {weight:?}) has no baked face");
            assert_eq!(Some(FontSize::nearest(px, weight)), exact, "{name}: nearest ≠ exact");
        }
    }

    #[test]
    fn every_name_reads_its_own_face() {
        // A crossed arm in `face()` (S12W500 → the 12/600 record) would pass
        // every size test and render the wrong weight.
        for size in FontSize::ALL {
            assert_eq!(format!("{size:?}"), format!("S{}W{}", size.px(), size.weight().wght()));
            assert_eq!(FontSize::exact(size.px(), size.weight()), Some(size));
            assert_eq!(FontSize::nearest(size.px(), size.weight()), size);
        }
    }

    #[test]
    fn nearest_picks_size_first_then_weight() {
        // A baked size keeps the request's size; the weight is the nearest
        // one baked there — Medium does NOT collapse onto Regular any more.
        assert_eq!(FontSize::nearest(11, Weight::Regular), FontSize::S11W500);
        assert_eq!(FontSize::nearest(12, Weight::Bold), FontSize::S12W600);
        assert_eq!(FontSize::nearest(14, Weight::Light), FontSize::S14W400);
        assert_eq!(FontSize::nearest(15, Weight::Regular), FontSize::S15W600);
        assert_eq!(FontSize::nearest(24, Weight::SemiBold), FontSize::S24W700);
        // A weight tie goes to the heavier face (caption2 + semibold).
        assert_eq!(FontSize::nearest(10, Weight::SemiBold), FontSize::S10W700);
        // Between sizes: the largest baked size below — never larger than asked.
        assert_eq!(FontSize::nearest(16, Weight::Regular), FontSize::S15W600, "16 → 15, not 17");
        assert_eq!(FontSize::nearest(18, Weight::SemiBold), FontSize::S17W600);
        assert_eq!(FontSize::nearest(22, Weight::Bold), FontSize::S20W600, "22 → 20, not 24");
        assert_eq!(FontSize::nearest(21, Weight::Regular), FontSize::S20W400);
        assert_eq!(FontSize::nearest(40, Weight::Light), FontSize::S30W700);
        assert_eq!(FontSize::nearest(63, Weight::Light), FontSize::S30W700, "63 → 30, not 64");
        assert_eq!(FontSize::nearest(120, Weight::Light), FontSize::S64W300, "120 → 64, not 128");
        assert_eq!(FontSize::nearest(128, Weight::Light), FontSize::S128W300);
        assert_eq!(FontSize::nearest(400, Weight::Light), FontSize::S128W300);
        // Below the smallest face: the smallest.
        assert_eq!(FontSize::nearest(1, Weight::Regular), FontSize::S10W500);
        // Size beats weight: Light exists only at 64, and no small request may
        // jump to the display face.
        assert_eq!(FontSize::nearest(13, Weight::Light), FontSize::S13W400);
        // Absent combinations stay absent.
        assert_eq!(FontSize::exact(16, Weight::Regular), None);
        assert_eq!(FontSize::exact(11, Weight::Regular), None);
        assert_eq!(FontSize::exact(64, Weight::Regular), None);
    }
}
