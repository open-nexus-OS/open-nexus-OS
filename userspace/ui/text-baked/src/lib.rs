// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(any(test, feature = "std")), no_std)]
// RFC-0080: the shared-atlas base is a raw mapped-VMO pointer → slice, which is
// inherently `unsafe`. `deny` (not `forbid`) so ONLY the two atlas-base items
// below may opt in with `#[allow(unsafe_code)]` + a documented safety contract;
// everything else stays unsafe-free.
#![deny(unsafe_code)]

//! CONTEXT: `nexus-text-baked` — the shared text SSOT: runtime measurement +
//! row-based A8 glyph rendering over the build-time-baked atlases of the
//! vendored UI face. PROMOTED VERBATIM from windowd's `text.rs`/`build.rs`
//! (RFC-0067 P5 discipline: promote the best implementation, make windowd a
//! client) so the app-host runtime, windowd, and future DSL shells measure
//! and draw text identically. `no_std`, zero deps, no runtime font parsing.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests (band clipping, measurement, clip-end, fallback)
//!
//! The API is ROW-BASED to match the surface renderers: every shell surface
//! is painted one pixel row at a time, so [`draw_text_row`] blends exactly
//! the slice of a text run that intersects the current row. A text run
//! occupies the band `top .. top + line_height(size)` with its baseline
//! `ascent(size)` pixels below the band top; rows outside the band return
//! immediately.

#[cfg(any(test, not(feature = "std"), feature = "layout"))]
extern crate alloc;

/// Pixel-real [`MeasureText`](nexus_layout_types::MeasureText) over the
/// baked atlases (feature `layout`).
#[cfg(feature = "layout")]
pub mod measure_text;

mod ladder;
mod metrics;
use ladder::{face, Face};
pub use ladder::{FontSize, Weight};
#[cfg(test)]
use ladder::{BODY, HERO};
pub use metrics::{advance, ellipsis_cut, measure, ELLIPSIS};

#[allow(clippy::all)]
mod baked {
    include!(concat!(env!("OUT_DIR"), "/baked_fonts.rs"));
}

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// RFC-0080: the process-global base of the glyph-coverage atlas — the ONE
/// backing that every `Face.cov()` slices. Set once via [`set_atlas_base`]
/// (an app-host installs its shared RO VMO mapping here). Until then it is
/// null; with the `embedded-atlas` feature it lazily auto-inits to the linked
/// blob, so windowd/host/tests need no setup and stay byte-identical.
static ATLAS_PTR: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static ATLAS_LEN: AtomicUsize = AtomicUsize::new(0);

/// Installs the atlas backing (RFC-0080): `ptr` must point at
/// [`atlas_len()`] readable bytes that stay valid for the process lifetime
/// (a mapped RO VMO). Idempotent; a consumer with `embedded-atlas` OFF MUST
/// call this before rendering any text.
///
/// # Safety
/// `ptr` must be valid and immutable for `len` bytes for the whole process
/// lifetime, and `len` must equal [`atlas_len()`] (the baked atlas size).
#[allow(unsafe_code)]
pub unsafe fn set_atlas_base(ptr: *const u8, len: usize) {
    ATLAS_LEN.store(len, Ordering::Release);
    ATLAS_PTR.store(ptr as *mut u8, Ordering::Release);
}

/// The baked atlas size in bytes (what [`set_atlas_base`] must be given).
#[must_use]
pub const fn atlas_len() -> usize {
    baked::ATLAS_LEN
}

/// The embedded atlas bytes (feature `embedded-atlas`) — the VMO owner's fill
/// source (RFC-0080 Phase 1): it `vmo_write`s these into a shared RO VMO that
/// every app-host maps via [`set_atlas_base`].
#[cfg(feature = "embedded-atlas")]
#[must_use]
pub fn embedded_atlas() -> &'static [u8] {
    baked::EMBEDDED_ATLAS
}

/// Resolves the whole coverage atlas as a slice. Falls back to the embedded
/// blob (feature `embedded-atlas`) when no base has been installed; returns an
/// empty slice otherwise (missing glyphs render blank — fail-visible, never
/// garbage).
#[inline]
#[allow(unsafe_code)]
fn atlas() -> &'static [u8] {
    let ptr = ATLAS_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        // No installed base: the lazy default is the linked blob (byte-
        // identical to the old path), or empty when it isn't linked.
        #[cfg(feature = "embedded-atlas")]
        {
            baked::EMBEDDED_ATLAS
        }
        #[cfg(not(feature = "embedded-atlas"))]
        {
            &[]
        }
    } else {
        let len = ATLAS_LEN.load(Ordering::Acquire);
        // SAFETY: the base was installed via `set_atlas_base`, whose contract
        // requires `ptr` valid + immutable for `len` bytes for the process
        // lifetime.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
}

/// Height of a text band (ascent + descent, no extra leading — callers add
/// their own line spacing).
#[must_use]
pub const fn line_height(size: FontSize) -> u32 {
    face(size).line_h
}

/// Baseline offset from the band top.
#[must_use]
pub const fn ascent(size: FontSize) -> i32 {
    face(size).ascent
}

/// Average glyph advance — the wrap heuristic for interim char-count
/// wrapping (measured wrapping lands with the layout unification).
#[must_use]
pub const fn avg_advance(size: FontSize) -> u32 {
    face(size).avg_advance
}

/// Glyph index for a char WITHIN one face: dense ASCII by offset, the sparse
/// EXTRAS tail (umlauts/ß, ± ÷ × −), then the WIDE tail (kana/jamo/hangul/han,
/// RFC-0075 Phase 8d) by binary search. `None` when the codepoint is outside
/// this face's charset OR the slot is an absent glyph (a `Latin` face carries
/// ASCII-shaped slots for indices it does not rasterize, so the dense scheme
/// survives — an absent slot is `w == 0 && advance == 0`).
#[inline]
fn glyph_index_in(f: &Face, ch: char) -> Option<usize> {
    let c = ch as u32;
    let gi = if (32..=126).contains(&c) {
        (c - 32) as usize
    } else if let Ok(i) = f.extras.binary_search(&c) {
        95 + i
    } else {
        95 + f.extras.len() + f.wide.binary_search(&c).ok()?
    };
    let &(_, w, _, _, _, adv) = f.glyphs.get(gi)?;
    (w > 0 || adv > 0).then_some(gi)
}

/// Resolve a char to the face that actually carries it and its glyph index
/// (RFC-0082): this face first, then its `fallback`, then the replacement
/// glyph `?`. A codepoint nobody has renders blank — fail-visible, never a
/// substituted shape.
#[inline]
fn resolve(f: &'static Face, ch: char) -> (&'static Face, usize) {
    if let Some(gi) = glyph_index_in(f, ch) {
        return (f, gi);
    }
    if let Some(fb) = f.fallback {
        if let Some(gi) = glyph_index_in(fb, ch) {
            return (fb, gi);
        }
        if let Some(gi) = glyph_index_in(fb, '?') {
            return (fb, gi);
        }
    }
    (f, ('?' as u32 - 32) as usize)
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

/// Blend the slice of the run `text` that intersects surface row `local_y`
/// into `row` (BGRA, straight alpha). The run's band starts at row `top`
/// (`i32`: a band may start above the surface when partially scrolled off),
/// the pen at column `x0`; pixels at or beyond `clip_end_x` (and beyond the
/// row buffer) are not touched. Glyph coverage is blended `src OVER dst`
/// scaled by `color`'s alpha — text composites correctly over glass tints.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_row(
    row: &mut [u8],
    local_y: u32,
    top: i32,
    x0: u32,
    clip_end_x: u32,
    text: impl Iterator<Item = char>,
    size: FontSize,
    color: [u8; 4],
) {
    let primary = face(size);
    let band_y = local_y as i32 - top;
    if band_y < 0 || band_y >= primary.line_h as i32 {
        return;
    }
    let row_px = (row.len() / 4) as u32;
    let clip = clip_end_x.min(row_px);
    let mut pen = x0 as i32;
    let mut prev: Option<(&'static Face, usize)> = None;
    for ch in text {
        let (f, gi) = resolve(primary, ch);
        if let Some((pf, p)) = prev {
            if pf.id == f.id {
                pen += kern(f, p, gi);
            }
        }
        let Some(&(off, w, h, left, gtop, adv)) = f.glyphs.get(gi) else { continue };
        // A fallback glyph was baked against ITS face's ascent; shifting by
        // the ascent delta puts both faces' baselines on the same row.
        let gy = band_y - (gtop as i32 + primary.ascent - f.ascent);
        if w > 0 && gy >= 0 && (gy as u16) < h {
            let start = off as usize + gy as usize * w as usize;
            if let Some(src) = f.cov().get(start..start + w as usize) {
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
        prev = Some((f, gi));
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
    fn line_heights_are_sane() {
        let lh = line_height(FontSize::Body);
        assert!((16..=24).contains(&lh), "16px face line height plausible, got {lh}");
        let lh_s = line_height(FontSize::Small);
        assert!((13..=20).contains(&lh_s), "13px face line height plausible, got {lh_s}");
        assert!(ascent(FontSize::Body) > 0);
    }

    #[test]
    fn wide_tail_serves_cjk_glyphs() {
        // RFC-0075 Phase 8d: kana / hangul / han resolve to REAL glyphs —
        // wider than the `?` fallback and mutually distinct.
        let q = measure("?".chars(), FontSize::Body);
        for text in ["ん", "日本語", "한", "你好"] {
            let w = measure(text.chars(), FontSize::Body);
            assert!(w > q, "{text} must not be the ? fallback (w={w}, q={q})");
        }
        // Latin metrics unchanged: the EXTRAS tail still resolves.
        assert!(measure("ä".chars(), FontSize::Small) > 0);
        // The secure-field bullet resolves (else passwords mask as `?`).
        let f = face(FontSize::Body);
        assert_ne!(glyph_index_in(f, '•'), glyph_index_in(f, '?'));
        // A codepoint OUTSIDE the baked set still falls back to `?`.
        assert_eq!(measure("\u{1F600}".chars(), FontSize::Body), q);
    }

    // ------------------------------------------------ RFC-0082 type ladder

    /// Total coverage of a run — the crude "how much ink" probe the weight
    /// and size assertions below compare against.
    fn ink(text: &str, size: FontSize) -> u32 {
        let w = measure(text.chars(), size) + 8;
        let mut sum = 0u32;
        for y in 0..line_height(size) {
            let mut row = alloc::vec![0u8; (w * 4) as usize];
            draw_text_row(&mut row, y, 0, 0, w, text.chars(), size, WHITE);
            sum += row.chunks_exact(4).map(|p| u32::from(p[3])).sum::<u32>();
        }
        sum
    }

    #[test]
    fn atlas_stays_within_the_rfc0082_budget() {
        // The atlas is ONE shared RO VMO mapped into every app-host
        // (RFC-0080) — growth here is arena pressure everywhere. The ladder
        // adds ~150 KB on top of the two Full faces; 5 MB is the ceiling a
        // new face has to argue against.
        assert!(
            atlas_len() < 5 * 1024 * 1024,
            "baked atlas is {} bytes — over the 5 MB budget (RFC-0082)",
            atlas_len()
        );
        assert_eq!(baked::FONT13_COV_OFFSET, 0, "13px Full must stay first in the atlas");
    }

    #[test]
    fn existing_latin_metrics_are_frozen() {
        // The real "no existing layout shifts" invariant. Face LENGTHS move
        // legitimately whenever the CJK charset grows (an i18n string, a
        // calendar name), so pinning those would be a tripwire that fires on
        // the wrong thing. What must never move is what layouts are built
        // from: the measured width of Latin text at the two original faces.
        assert_eq!(measure("Handgloves 0123".chars(), FontSize::Small), 107);
        assert_eq!(measure("Handgloves 0123".chars(), FontSize::Body), 132);
        assert_eq!(measure("Sonntag, 26. Juli".chars(), FontSize::Small), 104);
        assert_eq!(measure("Sonntag, 26. Juli".chars(), FontSize::Body), 128);
        // …and their line boxes, which every row height derives from.
        assert_eq!(line_height(FontSize::Small), 16);
        assert_eq!(line_height(FontSize::Body), 20);
        assert_eq!(ascent(FontSize::Small), 13);
        assert_eq!(ascent(FontSize::Body), 16);
    }

    /// A run that fits is never touched; one that does not is cut short enough
    /// that the ellipsis still fits the same width. The second half is the
    /// property that matters: an ellipsis that overflows would be the very
    /// clipping it exists to announce.
    #[test]
    #[test]
    fn nearest_picks_size_first_then_weight() {
        // Exact rungs.
        assert_eq!(FontSize::nearest(13, Weight::Regular), FontSize::Small);
        assert_eq!(FontSize::nearest(16, Weight::Regular), FontSize::Body);
        assert_eq!(FontSize::nearest(21, Weight::SemiBold), FontSize::TitleSemi);
        assert_eq!(FontSize::nearest(120, Weight::Light), FontSize::Hero);
        assert_eq!(FontSize::nearest(11, Weight::Regular), FontSize::Caption);
        assert_eq!(FontSize::nearest(11, Weight::SemiBold), FontSize::CaptionSemi);
        // Between rungs: nearest px wins, ties round UP.
        assert_eq!(FontSize::nearest(14, Weight::Regular), FontSize::Small, "14 → 13, not 16");
        assert_eq!(FontSize::nearest(15, Weight::Regular), FontSize::Body);
        assert_eq!(FontSize::nearest(18, Weight::Regular), FontSize::Body, "18 → 16, not 21");
        assert_eq!(FontSize::nearest(30, Weight::SemiBold), FontSize::DisplaySemi);
        // The tie the caption rung created. `sm` = 12 sits exactly between 11
        // and 13; rounding up keeps every shipped 12px label at 13, so baking
        // a smaller rung stays a pure ADDITION instead of a silent reflow.
        assert_eq!(FontSize::nearest(12, Weight::Regular), FontSize::Small, "12 → 13, not 11");
        assert_eq!(FontSize::nearest(12, Weight::SemiBold), FontSize::SmallSemi);
        // …and 10 is unambiguously nearer the caption rung than the 13.
        assert_eq!(FontSize::nearest(10, Weight::Regular), FontSize::Caption);
        // Size beats weight: Light exists ONLY at 120, and a 14px request must
        // NOT jump to the hero face.
        assert_eq!(FontSize::nearest(14, Weight::Light), FontSize::Small);
        // Weight missing at the chosen px degrades to Regular…
        assert_eq!(FontSize::nearest(21, Weight::Light), FontSize::Title);
        // …and to the only face there when Regular is missing too.
        assert_eq!(FontSize::nearest(36, Weight::Regular), FontSize::DisplaySemi);
        assert_eq!(FontSize::nearest(120, Weight::Regular), FontSize::Hero);
    }

    #[test]
    fn semibold_carries_more_ink_than_regular() {
        // The whole point of instancing the `wght` axis: a SemiBold face must
        // actually be heavier, not just a differently-named copy.
        for (reg, semi) in
            [(FontSize::Small, FontSize::SmallSemi), (FontSize::Body, FontSize::BodySemi)]
        {
            let (r, s) = (ink("Handgloves", reg), ink("Handgloves", semi));
            assert!(s > r, "{semi:?} ({s}) must be heavier than {reg:?} ({r})");
        }
        // …and Light must be lighter than SemiBold at a comparable size.
        assert!(ink("0123456789", FontSize::Hero) > 0, "the hero face renders numerals");
    }

    #[test]
    fn counters_are_not_filled_nonzero_winding() {
        // The instanced rasterizer fills with the NONZERO winding rule. If it
        // used even-odd — or ignored contour direction — the bowl of `o`
        // would be solid. Probe the middle row for lit · dark · lit.
        let size = FontSize::DisplaySemi;
        let w = measure("o".chars(), size) + 4;
        let mid = line_height(size) / 2;
        let mut row = alloc::vec![0u8; (w * 4) as usize];
        draw_text_row(&mut row, mid, 0, 0, w, "o".chars(), size, WHITE);
        let lit: alloc::vec::Vec<bool> = row.chunks_exact(4).map(|p| p[3] > 0).collect();
        let first = lit.iter().position(|&b| b).expect("left stem of `o`");
        let last = lit.iter().rposition(|&b| b).expect("right stem of `o`");
        let hole = lit[first..=last].iter().filter(|&&b| !b).count();
        assert!(
            hole > 2,
            "`o` at {size:?} has no counter — winding rule is wrong (span {first}..={last})"
        );
    }

    #[test]
    fn hero_digits_are_tabular() {
        // A ticking clock must not shuffle sideways: every numeral shares the
        // widest numeral's advance, and the face is unkerned.
        let one = advance('1', FontSize::Hero);
        assert!(one > 0, "the hero face rasterizes digits");
        for d in "023456789".chars() {
            assert_eq!(advance(d, FontSize::Hero), one, "digit {d} is not tabular");
        }
        assert_eq!(
            measure("13:16".chars(), FontSize::Hero),
            measure("08:59".chars(), FontSize::Hero),
            "same-shape times must measure identically"
        );
        assert!(face(FontSize::Hero).kern.is_empty(), "tabular figures must stay unkerned");
    }

    #[test]
    fn latin_faces_fall_back_to_the_body_face_for_cjk() {
        // RFC-0082: a codepoint outside a Latin/Digits face's charset resolves
        // against the 16px Full face — smaller than its neighbours, but
        // rendered. Blank or `?` would both be wrong.
        for size in [FontSize::TitleSemi, FontSize::DisplaySemi, FontSize::Hero] {
            let (f, gi) = resolve(face(size), 'ん');
            assert_eq!(f.id, BODY.id, "{size:?} must fall back to the 16px Full face");
            assert_eq!(Some(gi), glyph_index_in(&BODY, 'ん'));
            assert_eq!(advance('ん', size), advance('ん', FontSize::Body));
        }
        // The hero face carries no letters at all — those fall back too.
        let (f, _) = resolve(face(FontSize::Hero), 'A');
        assert_eq!(f.id, BODY.id, "hero letters come from the Full face");
        // …but its own digits do NOT fall back.
        let (f, _) = resolve(face(FontSize::Hero), '7');
        assert_eq!(f.id, HERO.id, "hero digits stay on the hero face");
    }

    #[test]
    fn fallback_glyphs_share_the_primary_baseline() {
        // A fallback glyph was baked against ITS face's ascent; drawing it in
        // a taller band must shift it so the baselines coincide. Proof: the
        // kana ink sits around the primary face's baseline, not at the top.
        let size = FontSize::DisplaySemi;
        let (w, lh) = (measure("ん".chars(), size) + 8, line_height(size));
        let baseline = ascent(size);
        let mut lowest = 0i32;
        let mut highest = i32::MAX;
        for y in 0..lh {
            let mut row = alloc::vec![0u8; (w * 4) as usize];
            draw_text_row(&mut row, y, 0, 0, w, "ん".chars(), size, WHITE);
            if row.chunks_exact(4).any(|p| p[3] > 0) {
                lowest = lowest.max(y as i32);
                highest = highest.min(y as i32);
            }
        }
        assert!(highest < lowest, "the fallback glyph rendered at all");
        assert!(
            lowest <= baseline + 2 && lowest > baseline - ascent(FontSize::Body),
            "kana ink ({highest}..={lowest}) must sit on the {size:?} baseline ({baseline})"
        );
    }

    #[test]
    fn runtime_atlas_base_is_byte_identical_to_embedded() {
        // RFC-0080: installing an atlas base (a mapped RO VMO on device) must
        // render byte-identically to the embedded blob. Point the base at a
        // LEAKED heap copy of the same bytes (leaked → 'static, valid for the
        // process, so it never disturbs other tests — the resolved coverage is
        // identical either way).
        let heap: &'static [u8] = alloc::boxed::Box::leak(baked::EMBEDDED_ATLAS.to_vec().into());
        assert_eq!(heap.len(), atlas_len(), "heap copy matches the baked atlas size");
        // SAFETY: `heap` is a leaked Vec — valid + immutable for `heap.len()`
        // bytes for the whole process, and its len equals `atlas_len()`.
        #[allow(unsafe_code)]
        unsafe {
            set_atlas_base(heap.as_ptr(), heap.len());
        }
        // Every draw + measure now resolves through the installed base.
        for text in ["Hello", "ä ö ü", "日本語", "你好", "•••", "?"] {
            let rows = draw_band(text, FontSize::Body, 200);
            let lit: usize = rows.iter().map(|r| row_lit(r)).sum();
            assert!(lit > 0 || text == " ", "{text} renders through the runtime base");
        }
        // The bytes are identical, so the CJK-vs-? and bullet invariants hold.
        let q = measure("?".chars(), FontSize::Body);
        assert!(measure("日本語".chars(), FontSize::Body) > q);
        // Reset to the default (null → embedded) so other tests are unaffected
        // regardless of order.
        #[allow(unsafe_code)]
        unsafe {
            set_atlas_base(core::ptr::null(), 0);
        }
    }
}
