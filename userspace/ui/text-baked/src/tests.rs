// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests of the baked faces: the row draw, the fallback chain, the atlas
//! budget, the frozen metrics layouts are built from, the weights, tracking on
//! the painted row, and the `display` digits face.

use super::*;
use alloc::vec::Vec;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const NONE: Tracking = Tracking::NONE;

fn draw_band(text: &str, size: FontSize, w: u32) -> Vec<Vec<u8>> {
    let mut rows = Vec::new();
    for y in 0..line_height(size) + 4 {
        let mut row = alloc::vec![0u8; (w * 4) as usize];
        draw_text_row(&mut row, y, 2, 0, w, text.chars(), size, NONE, WHITE);
        rows.push(row);
    }
    rows
}

fn row_lit(row: &[u8]) -> usize {
    row.chunks_exact(4).filter(|p| p[3] > 0).count()
}

/// Total coverage of a run — the crude "how much ink" probe the weight and
/// size assertions below compare against.
fn ink(text: &str, size: FontSize) -> u32 {
    let w = measure(text.chars(), size, NONE) + 8;
    let mut sum = 0u32;
    for y in 0..line_height(size) {
        let mut row = alloc::vec![0u8; (w * 4) as usize];
        draw_text_row(&mut row, y, 0, 0, w, text.chars(), size, NONE, WHITE);
        sum += row.chunks_exact(4).map(|p| u32::from(p[3])).sum::<u32>();
    }
    sum
}

/// The leftmost and rightmost lit column of a run drawn with `tracking`.
fn ink_span(text: &str, size: FontSize, tracking: Tracking) -> (usize, usize) {
    let w = measure(text.chars(), size, NONE) + 16;
    let (mut lo, mut hi) = (usize::MAX, 0usize);
    for y in 0..line_height(size) {
        let mut row = alloc::vec![0u8; (w * 4) as usize];
        draw_text_row(&mut row, y, 0, 0, w, text.chars(), size, tracking, WHITE);
        for (x, p) in row.chunks_exact(4).enumerate() {
            if p[3] > 0 {
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
    }
    (lo, hi)
}

#[test]
fn draws_inside_band_only() {
    let rows = draw_band("Hello", FontSize::S14W400, 100);
    assert_eq!(row_lit(&rows[0]), 0, "above the band (top=2) stays untouched");
    assert_eq!(row_lit(&rows[1]), 0);
    let lit: usize = rows.iter().map(|r| row_lit(r)).sum();
    assert!(lit > 40, "a 5-glyph run lights a substantial pixel count, got {lit}");
}

#[test]
fn measure_is_monotonic_and_positive() {
    let a = measure("hi".chars(), FontSize::S13W400, NONE);
    let b = measure("hi there".chars(), FontSize::S13W400, NONE);
    assert!(a > 0);
    assert!(b > a, "longer text measures wider ({a} vs {b})");
}

#[test]
fn clip_end_is_respected() {
    let size = FontSize::S14W400;
    for tracking in [NONE, Tracking::em(60), Tracking::em(-20)] {
        for y in 0..line_height(size) {
            let mut row = alloc::vec![0u8; 100 * 4];
            draw_text_row(&mut row, y, 0, 0, 20, "wwwwwwwwww".chars(), size, tracking, WHITE);
            for (x, px) in row.chunks_exact(4).enumerate() {
                assert!(x < 20 || px[3] == 0, "pixel {x} written past clip_end_x=20");
            }
        }
    }
}

#[test]
fn non_ascii_falls_back_without_panic() {
    let w = measure("a—b".chars(), FontSize::S14W400, NONE); // em-dash → '?'
    assert!(w > measure("ab".chars(), FontSize::S14W400, NONE));
    let rows = draw_band("—…—", FontSize::S14W400, 80);
    assert!(rows.iter().map(|r| row_lit(r)).sum::<usize>() > 0);
}

#[test]
fn wide_tail_serves_cjk_glyphs() {
    // RFC-0075 Phase 8d: kana / hangul / han resolve to REAL glyphs — wider
    // than the `?` fallback — in both Full faces.
    for size in [FontSize::S13W400, FontSize::S14W400] {
        let q = measure("?".chars(), size, NONE);
        for text in ["ん", "日本語", "한", "你好"] {
            let w = measure(text.chars(), size, NONE);
            assert!(w > q, "{size:?}: {text} must not be the ? fallback (w={w}, q={q})");
        }
        // The secure-field bullet resolves (else passwords mask as `?`).
        let f = face(size);
        assert_ne!(glyph_index_in(f, '•'), glyph_index_in(f, '?'));
        // A codepoint OUTSIDE the baked set still falls back to `?`.
        assert_eq!(measure("\u{1F600}".chars(), size, NONE), q);
    }
    // The EXTRAS tail resolves on a Latin face.
    assert!(measure("ä".chars(), FontSize::S12W400, NONE) > 0);
}

/// Every face must actually RASTERISE. A face that bakes empty lays out
/// identically and renders nothing, so every layout-level and scene-text
/// test stays green while the screen goes blank — exactly the gap that once
/// let a blank display reach the device.
#[test]
fn every_face_has_ink() {
    for size in FontSize::ALL {
        assert!(ink("0123456789", size) > 0, "{size:?} rasterises no digits");
        assert!(advance('7', size, NONE) > 0, "{size:?} has no advance");
        assert!(ink("AC", size) > 0, "{size:?} rasterises no letters (nor falls back)");
    }
}

#[test]
fn atlas_stays_within_the_budget() {
    // The atlas is ONE shared RO VMO mapped into every app-host (RFC-0080) —
    // growth here is arena pressure everywhere. Two Full faces (13, 14 px)
    // carry almost all of it; 5 MiB is the ceiling a new face has to argue
    // against.
    assert!(
        atlas_len() < 5 * 1024 * 1024,
        "baked atlas is {} bytes — over the 5 MiB budget (RFC-0082)",
        atlas_len()
    );
    assert_eq!(face(FontSize::S13W400).cov_off, 0, "13px Full must stay first in the atlas");
}

#[test]
fn latin_metrics_are_frozen() {
    // The real "no existing layout shifts" invariant. Face LENGTHS move
    // legitimately whenever the CJK charset grows (an i18n string, a calendar
    // name), so pinning those would be a tripwire that fires on the wrong
    // thing. What must never move is what layouts are built from: the
    // measured width of Latin text and the line boxes of the two Full faces.
    let s13 = FontSize::S13W400;
    assert_eq!(measure("Handgloves 0123".chars(), s13, NONE), 107);
    assert_eq!(measure("Sonntag, 26. Juli".chars(), s13, NONE), 104);
    assert_eq!((line_height(s13), ascent(s13)), (16, 13));
    // The 14 px `body` face as first baked (RFC-0116).
    let s14 = FontSize::S14W400;
    assert_eq!(measure("Handgloves 0123".chars(), s14, NONE), 115);
    assert_eq!(measure("Sonntag, 26. Juli".chars(), s14, NONE), 111);
    assert_eq!((line_height(s14), ascent(s14)), (17, 14));
}

#[test]
fn heavier_weights_carry_more_ink() {
    // The whole point of instancing the `wght` axis: a heavier face must
    // actually be heavier, not a differently-named copy — Medium over
    // Regular, SemiBold over Medium, Bold over Medium, at every size that
    // bakes more than one weight.
    for px in [10, 11, 12, 13, 14, 20] {
        let faces: Vec<FontSize> = FontSize::ALL.into_iter().filter(|s| s.px() == px).collect();
        assert!(faces.len() > 1, "{px} px bakes one weight only");
        for pair in faces.windows(2) {
            let (light, heavy) = (ink("Handgloves", pair[0]), ink("Handgloves", pair[1]));
            assert!(
                heavy > light,
                "{:?} ({heavy}) must be heavier than {:?} ({light})",
                pair[1],
                pair[0]
            );
        }
    }
}

#[test]
fn counters_are_not_filled_nonzero_winding() {
    // The instanced rasterizer fills with the NONZERO winding rule. If it
    // used even-odd — or ignored contour direction — the bowl of `o` would be
    // solid. Probe the middle of the glyph for lit · dark · lit.
    for (size, ch) in [(FontSize::S30W700, 'o'), (FontSize::S64W300, '0')] {
        let text = [ch];
        let w = measure(text.iter().copied(), size, NONE) + 4;
        let mid = (ascent(size) - size.px() / 3) as u32;
        let mut row = alloc::vec![0u8; (w * 4) as usize];
        draw_text_row(&mut row, mid, 0, 0, w, text.iter().copied(), size, NONE, WHITE);
        let lit: Vec<bool> = row.chunks_exact(4).map(|p| p[3] > 0).collect();
        let first = lit.iter().position(|&b| b).expect("left stem");
        let last = lit.iter().rposition(|&b| b).expect("right stem");
        let hole = lit[first..=last].iter().filter(|&&b| !b).count();
        assert!(hole > 2, "{ch:?} at {size:?} has no counter (span {first}..={last})");
    }
}

#[test]
fn display_digits_are_tabular() {
    // A ticking clock must not shuffle sideways: every numeral shares the
    // widest numeral's advance, and the face is unkerned — the `display` face
    // and the lock screen's `clock` face alike.
    for size in [FontSize::S64W300, FontSize::S128W300] {
        let one = advance('1', size, NONE);
        assert!(one > 0, "{size:?} rasterizes digits");
        for d in "023456789".chars() {
            assert_eq!(advance(d, size, NONE), one, "{size:?}: digit {d} is not tabular");
        }
        assert_eq!(
            measure("13:16".chars(), size, NONE),
            measure("08:59".chars(), size, NONE),
            "{size:?}: same-shape times must measure identically"
        );
        assert!(face(size).kern.is_empty(), "{size:?}: tabular figures must stay unkerned");
    }
}

#[test]
fn the_display_face_draws_every_digit_glyph() {
    // The clock and the calculator display: every glyph of the digits
    // charset is the display face's OWN, with ink — none falls back to the
    // 14 px face (which would paint a tiny glyph in a 64 px line).
    for size in [FontSize::S64W300, FontSize::S128W300] {
        let display = face(size);
        for ch in "0123456789:,.\u{2212}-+\u{D7}\u{F7}%=".chars() {
            let (f, _) = resolve(display, ch);
            assert_eq!(f.id, display.id, "{size:?}: {ch:?} falls back off the face");
            assert!(ink(&alloc::format!("{ch}"), size) > 0, "{size:?}: {ch:?} has no ink");
        }
        let (f, _) = resolve(display, ' ');
        assert_eq!(f.id, display.id, "{size:?}: the space is the face's own");
        assert!(advance(' ', size, NONE) > 0);
        // Letters are not in the charset: they come from the 14 px face.
        let (f, _) = resolve(display, 'A');
        assert_eq!(f.id, face(FontSize::S14W400).id);
    }
}

#[test]
fn latin_faces_fall_back_to_the_body_face_for_cjk() {
    // A codepoint outside a Latin/Digits face's charset resolves against the
    // 14 px Full face — smaller than its neighbours, but rendered. Blank or
    // `?` would both be wrong.
    let body = face(FontSize::S14W400);
    for size in [FontSize::S11W500, FontSize::S20W600, FontSize::S30W700, FontSize::S64W300] {
        let (f, gi) = resolve(face(size), 'ん');
        assert_eq!(f.id, body.id, "{size:?} must fall back to the 14px Full face");
        assert_eq!(Some(gi), glyph_index_in(body, 'ん'));
        assert_eq!(advance('ん', size, NONE), advance('ん', FontSize::S14W400, NONE));
    }
    // The 13 px Full face carries its own CJK.
    let (f, _) = resolve(face(FontSize::S13W400), 'ん');
    assert_eq!(f.id, face(FontSize::S13W400).id);
}

#[test]
fn fallback_glyphs_share_the_primary_baseline() {
    // A fallback glyph was baked against ITS face's ascent; drawing it in a
    // taller band must shift it so the baselines coincide. Proof: the kana
    // ink sits around the primary face's baseline, not at the top.
    let size = FontSize::S30W700;
    let (w, lh) = (measure("ん".chars(), size, NONE) + 8, line_height(size));
    let baseline = ascent(size);
    let mut lowest = 0i32;
    let mut highest = i32::MAX;
    for y in 0..lh {
        let mut row = alloc::vec![0u8; (w * 4) as usize];
        draw_text_row(&mut row, y, 0, 0, w, "ん".chars(), size, NONE, WHITE);
        if row.chunks_exact(4).any(|p| p[3] > 0) {
            lowest = lowest.max(y as i32);
            highest = highest.min(y as i32);
        }
    }
    assert!(highest < lowest, "the fallback glyph rendered at all");
    assert!(
        lowest <= baseline + 2 && lowest > baseline - ascent(FontSize::S14W400),
        "kana ink ({highest}..={lowest}) must sit on the {size:?} baseline ({baseline})"
    );
}

#[test]
fn a_tracked_run_paints_where_it_measures() {
    // `largeTitle` −0.02 em at 30 px: glyph i starts round(i × −0.6) px
    // earlier. The first glyph does not move; the tenth starts
    // round(9 × −0.6) = −5 px off — the painter places glyphs with the very
    // arithmetic `measure` counts them with.
    let (size, run) = (FontSize::S30W700, "HHHHHHHHHH");
    let plain = ink_span(run, size, NONE);
    let tight = ink_span(run, size, Tracking::em(-20));
    assert_eq!(tight.0, plain.0, "the first glyph keeps its place");
    assert_eq!(tight.1 + 5, plain.1, "the last glyph moves by the run's tracking");
    // `overline` +0.06 em at 11 px: the tenth glyph starts round(9 × 0.66) = 6 px later.
    let (size, run) = (FontSize::S11W600, "NNNNNNNNNN");
    let plain = ink_span(run, size, NONE);
    let wide = ink_span(run, size, Tracking::em(60));
    assert_eq!((wide.0, wide.1), (plain.0, plain.1 + 6));
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
        let rows = draw_band(text, FontSize::S14W400, 200);
        let lit: usize = rows.iter().map(|r| row_lit(r)).sum();
        assert!(lit > 0, "{text} renders through the runtime base");
    }
    // The bytes are identical, so the CJK-vs-? invariant holds.
    let q = measure("?".chars(), FontSize::S14W400, NONE);
    assert!(measure("日本語".chars(), FontSize::S14W400, NONE) > q);
    // Reset to the default (null → embedded) so other tests are unaffected
    // regardless of order.
    #[allow(unsafe_code)]
    unsafe {
        set_atlas_base(core::ptr::null(), 0);
    }
}

/// Tabular figures (RFC-0116 M-3): every digit of a face takes one width — the widest digit's —
/// so `1111` and `8888` measure the same and a changing number never shifts; letters keep their
/// proportional advances, and no digit is narrower than its own glyph.
#[test]
fn tabular_digits_take_one_width() {
    let tab = NONE.tabular();
    for size in [FontSize::S13W400, FontSize::S14W400, FontSize::S30W700, FontSize::S64W300] {
        let one = measure(core::iter::once('1'), size, tab);
        for d in '0'..='9' {
            let w = measure(core::iter::once(d), size, tab);
            assert_eq!(w, one, "{size:?}: digit {d:?} is not one width");
            assert!(w >= measure(core::iter::once(d), size, NONE), "{size:?}: {d:?} shrank");
        }
        assert_eq!(measure("1111".chars(), size, tab), measure("8888".chars(), size, tab));
        assert_eq!(measure("Hello".chars(), size, tab), measure("Hello".chars(), size, NONE));
        // Letter-spacing still applies on top of the cell.
        let spaced = Tracking::em(60).tabular();
        assert!(measure("1234".chars(), size, spaced) > measure("1234".chars(), size, tab));
    }
}

/// A tabular `1` is drawn in the middle of its cell: the ink of `1` in a two-digit run sits
/// where the ink of `8` sits, give or take the glyphs' own widths.
#[test]
fn tabular_digits_are_centred_in_their_cell() {
    let size = FontSize::S30W700;
    let tab = NONE.tabular();
    let w = 200;
    let ink = |text: &str| {
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for y in 0..line_height(size) {
            let mut row = alloc::vec![0u8; (w * 4) as usize];
            draw_text_row(&mut row, y, 0, 0, w, text.chars(), size, tab, WHITE);
            for (x, px) in row.chunks_exact(4).enumerate() {
                if px[3] > 0 {
                    lo = lo.min(x as u32);
                    hi = hi.max(x as u32);
                }
            }
        }
        (lo, hi)
    };
    let (one_lo, one_hi) = ink("1");
    let (eight_lo, eight_hi) = ink("8");
    let centre = |lo: u32, hi: u32| (lo + hi) as i32 / 2;
    assert!(
        (centre(one_lo, one_hi) - centre(eight_lo, eight_hi)).abs() <= 2,
        "a tabular 1 sits off-centre: 1 at {one_lo}..{one_hi}, 8 at {eight_lo}..{eight_hi}"
    );
}
