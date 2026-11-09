// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The PEN of one run: where each glyph of a run is placed. `measure`,
//! `advance`, `ellipsis_cut` and the row draw all walk a run through this one
//! type, so a run's tracking (RFC-0116 letter-spacing) and its figure width
//! (tabular digits) can never be applied one way when it is measured and
//! another way when it is painted.

use crate::ladder::Face;
use crate::{kern, resolve};

/// How a run's glyphs are spaced: its letter-spacing and its figure width.
///
/// `milli_em` is the letter-spacing in thousandths of an em, signed — the
/// text styles' tracking (RFC-0116): `display` is `Tracking::em(-20)`
/// (−0.02 em), `overline` `Tracking::em(60)`. It is added after EVERY glyph,
/// the last one included (as CSS `letter-spacing` is), so a run of n glyphs
/// grows by n × tracking; the em is the pixel size of the face the run is
/// drawn in.
///
/// `tabular` gives the digits one width (CSS `tabular-nums`): each digit
/// advances by the face's widest digit, centred in that cell, and no kerning
/// touches a digit — a changing number never shifts its neighbours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Tracking {
    pub milli_em: i16,
    pub tabular: bool,
}

impl Tracking {
    /// No letter-spacing, proportional figures.
    pub const NONE: Tracking = Tracking { milli_em: 0, tabular: false };

    /// Letter-spacing of `milli_em` thousandths of an em, proportional figures.
    #[must_use]
    pub const fn em(milli_em: i16) -> Self {
        Self { milli_em, tabular: false }
    }

    /// The same spacing with tabular figures.
    #[must_use]
    pub const fn tabular(self) -> Self {
        Self { milli_em: self.milli_em, tabular: true }
    }
}

/// The glyph indices of `0`…`9`: every face's dense ASCII table starts at the
/// space (index = code − 32).
const DIGITS: core::ops::RangeInclusive<usize> = 16..=25;

/// The tabular cell of `f`: its widest digit's advance.
fn digit_cell(f: &Face) -> i32 {
    DIGITS.filter_map(|gi| f.glyphs.get(gi)).map(|g| i32::from(g.5)).max().unwrap_or(0)
}

/// Places the glyphs of one run, left to right.
///
/// Advances and kerning are whole pixels (baked); tracking is a fraction of
/// a pixel per glyph. Summing a rounded per-glyph value would drift by up to
/// half a pixel per glyph, so tracking is kept as a GLYPH COUNT and turned
/// into pixels with one rounding per position: glyph `i` starts at
/// `advances + kerning + round(i × tracking × px / 1000)` — exact for any run
/// length, in integer arithmetic only.
#[derive(Clone, Copy)]
pub(crate) struct Pen {
    primary: &'static Face,
    /// Advances and kerning so far, in whole pixels.
    whole: i32,
    /// Glyphs placed so far.
    placed: i64,
    /// Tracking per glyph, in thousandths of a pixel (milli-em × px).
    step: i64,
    prev: Option<(&'static Face, usize)>,
    /// Tabular figures: digits advance by [`digit_cell`] and are not kerned.
    tabular: bool,
}

impl Pen {
    pub(crate) fn new(primary: &'static Face, tracking: Tracking) -> Self {
        let step = i64::from(tracking.milli_em) * i64::from(primary.px);
        Self { primary, whole: 0, placed: 0, step, prev: None, tabular: tracking.tabular }
    }

    /// Places `ch`: the face and glyph index that draw it, and the x its
    /// glyph origin sits at, relative to the start of the run.
    #[inline]
    pub(crate) fn place(&mut self, ch: char) -> (&'static Face, usize, i32) {
        let (f, gi) = resolve(self.primary, ch);
        (f, gi, self.place_glyph(f, gi))
    }

    /// [`Self::place`] for a glyph already resolved against this run's face.
    #[inline]
    pub(crate) fn place_glyph(&mut self, f: &'static Face, gi: usize) -> i32 {
        let digit = |gi: usize| self.tabular && DIGITS.contains(&gi);
        // Kerning only applies between two glyphs from the SAME face — a pair
        // straddling the fallback seam has no meaningful pair value — and never
        // to a tabular digit.
        if let Some((pf, p)) = self.prev {
            if pf.id == f.id && !digit(p) && !digit(gi) {
                self.whole = self.whole.saturating_add(kern(f, p, gi));
            }
        }
        let adv = f.glyphs.get(gi).map_or(0, |g| i32::from(g.5));
        // A tabular digit takes the face's widest-digit cell, its glyph centred in it.
        let (centre, step) = if digit(gi) {
            let cell = digit_cell(f).max(adv);
            ((cell - adv) / 2, cell)
        } else {
            (0, adv)
        };
        let x = self.whole.saturating_add(self.tracked(self.placed)).saturating_add(centre);
        self.whole = self.whole.saturating_add(step);
        self.placed += 1;
        self.prev = Some((f, gi));
        x
    }

    /// The run's width so far — its advances, kerning and the tracking after
    /// every placed glyph: where the next glyph would start before kerning.
    #[inline]
    pub(crate) fn width(&self) -> i32 {
        self.whole.saturating_add(self.tracked(self.placed))
    }

    /// The tracking of `glyphs` glyphs in whole pixels, rounded half up once.
    #[inline]
    fn tracked(&self, glyphs: i64) -> i32 {
        if self.step == 0 {
            return 0;
        }
        let px = glyphs.saturating_mul(self.step).saturating_add(500).div_euclid(1000);
        px.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }
}
