// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bakes the shared A8 glyph atlases (13px/16px) from the vendored
//! faces at build time — Inter for Latin, the PINNED Noto Sans CJK faces
//! for kana / hangul / han per the font-library.md fallback contract
//! (RFC-0075 Phase 8d). Charset = dense ASCII + Latin EXTRAS + a bounded
//! WIDE tail: full kana + CJK punctuation (Noto JP), compat jamo + the FULL
//! hangul syllable block (Noto KR — typing composes arbitrary syllables),
//! and the EXTRACTED han set actually used by the repo (i18n catalogs +
//! the IME engines' output tables + OSK labels; Noto SC). No runtime font
//! parsing anywhere; missing CJK faces fail the build with the fetch hint.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: consumed by `src/lib.rs` unit tests

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::Path;

const UI_FONT: &str = "../../../resources/fonts/inter/docs/font-files/InterVariable.ttf";
const NOTO_JP: &str = "../../../resources/fonts/noto/NotoSansCJKjp-Regular.otf";
const NOTO_KR: &str = "../../../resources/fonts/noto/NotoSansCJKkr-Regular.otf";
const NOTO_SC: &str = "../../../resources/fonts/noto/NotoSansCJKsc-Regular.otf";

/// Latin EXTRAS (umlauts/ß + calculator math symbols). MUST stay sorted
/// ascending and < 256 total glyph indices with ASCII (kern indices are u8).
const EXTRAS: [u32; 11] = [
    0x00B1, // ±
    0x00C4, // Ä
    0x00D6, // Ö
    0x00D7, // ×
    0x00DC, // Ü
    0x00DF, // ß
    0x00E4, // ä
    0x00F6, // ö
    0x00F7, // ÷
    0x00FC, // ü
    0x2212, // −
];

fn load(path: &str, hint: &str) -> fontdue::Font {
    let bytes = fs::read(path).unwrap_or_else(|err| {
        panic!("missing font {path}: {err}\n  hint: {hint}");
    });
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .unwrap_or_else(|err| panic!("parse font {path}: {err:?}"))
}

/// The four vendored faces, resolved per codepoint (script → face).
struct Faces {
    inter: fontdue::Font,
    jp: fontdue::Font,
    kr: fontdue::Font,
    sc: fontdue::Font,
}

impl Faces {
    /// Script-aware face pick (font-library.md fallback chain). Han goes to
    /// SC (simplified — the shipped zh line); kana + CJK punctuation to JP;
    /// jamo + syllables to KR; everything else stays Inter.
    fn for_code(&self, c: u32) -> &fontdue::Font {
        match c {
            // Contiguous JP span (punctuation + kana); jamo + hangul; han.
            0x3001..=0x30FF => &self.jp,
            0x3131..=0x318E | 0xAC00..=0xD7A3 => &self.kr,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF => &self.sc,
            _ => &self.inter,
        }
    }
}

/// The WIDE tail: every non-Latin codepoint the platform actually renders,
/// sorted + deduped. Bounded by construction (fixed ranges + extracted han).
fn wide_charset() -> Vec<u32> {
    let mut set: BTreeSet<u32> = BTreeSet::new();
    // Secure-field bullet (Inter) + CJK punctuation subset + kana.
    for c in [0x2022u32, 0x3001, 0x3002, 0x300C, 0x300D, 0x30FB] {
        set.insert(c);
    }
    set.extend(0x3041..=0x3096); // hiragana
    set.extend(0x30A0..=0x30FF); // katakana + ー
    set.extend(0x3131..=0x3163); // compat jamo
    set.extend(0xAC00..=0xD7A3); // FULL hangul block (typing composes any)
                                 // Engine outputs (lexicon kanji/han; kana already in-range).
    for ch in ime_core::engine_output_chars() {
        set.insert(ch as u32);
    }
    // OSK labels (jamo etc. — in-range already, but keep the SSOT honest).
    for layout in [
        keymaps::LayoutId::Us,
        keymaps::LayoutId::De,
        keymaps::LayoutId::Jp,
        keymaps::LayoutId::Kr,
        keymaps::LayoutId::Zh,
    ] {
        for row in 0..keymaps::OSK_ROWS {
            for key in keymaps::osk_rows(layout, row) {
                for ch in key.label.chars().chain(key.key.chars()) {
                    set.insert(ch as u32);
                }
            }
        }
    }
    // Every codepoint in every app i18n catalog (the actual UI text).
    let apps = Path::new("../../apps");
    if let Ok(entries) = fs::read_dir(apps) {
        let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for dir in dirs {
            let i18n = dir.join("i18n");
            let Ok(files) = fs::read_dir(&i18n) else { continue };
            let mut files: Vec<_> = files.flatten().map(|e| e.path()).collect();
            files.sort();
            for file in files {
                if file.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                println!("cargo:rerun-if-changed={}", file.display());
                if let Ok(text) = fs::read_to_string(&file) {
                    for ch in text.chars() {
                        set.insert(ch as u32);
                    }
                }
            }
        }
    }
    // Drop what the ASCII+EXTRAS span already covers (and non-chars).
    set.retain(|&c| {
        !(32..=126).contains(&c) && !EXTRAS.contains(&c) && char::from_u32(c).is_some()
    });
    set.into_iter().collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={UI_FONT}");
    for f in [NOTO_JP, NOTO_KR, NOTO_SC] {
        println!("cargo:rerun-if-changed={f}");
    }
    let out_dir = std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let out_dir = Path::new(&out_dir);
    let hint = "run scripts/fetch-fonts.sh (pinned Noto Sans CJK, font-library.md)";
    let faces = Faces {
        inter: load(UI_FONT, "git submodule update --init resources/fonts/inter"),
        jp: load(NOTO_JP, hint),
        kr: load(NOTO_KR, hint),
        sc: load(NOTO_SC, hint),
    };
    let wide = wide_charset();
    let mut generated = File::create(out_dir.join("baked_fonts.rs"))?;
    emit_glyph_atlas(&mut generated, out_dir, &faces, &wide, 13.0, "FONT13")?;
    emit_glyph_atlas(&mut generated, out_dir, &faces, &wide, 16.0, "FONT16")?;
    Ok(())
}

/// One face set at one size: coverage blob (`<name>.a8`) + per-glyph
/// placement + metrics + sparse Latin kerning + the sorted WIDE tail.
/// Layout per glyph: `(cov_offset, w, h, left_bearing, top_from_band_top,
/// advance_px)`. All glyphs share Inter's baseline (top is computed against
/// Inter's ascent so mixed-script runs align).
fn emit_glyph_atlas(
    generated: &mut File,
    out_dir: &Path,
    faces: &Faces,
    wide: &[u32],
    px: f32,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let latin: Vec<u32> = (32u32..=126).chain(EXTRAS.iter().copied()).collect();
    let charset: Vec<u32> = latin.iter().copied().chain(wide.iter().copied()).collect();
    let lm = faces
        .inter
        .horizontal_line_metrics(px)
        .ok_or_else(|| format!("{name}: no horizontal line metrics"))?;
    let ascent = lm.ascent.round() as i32;
    let line_h = (lm.ascent - lm.descent).ceil() as u32;

    let mut cov: Vec<u8> = Vec::new();
    let mut glyphs = String::new();
    let mut advance_sum = 0u32;
    let mut advance_max = 0u32;
    for &code in &charset {
        let ch = char::from_u32(code).ok_or("charset codepoint")?;
        let font = faces.for_code(code);
        let (m, bitmap) = font.rasterize(ch, px);
        let off = cov.len() as u32;
        cov.extend_from_slice(&bitmap);
        let top = ascent - (m.ymin + m.height as i32);
        let adv = m.advance_width.round().max(1.0) as u32;
        // Latin-only average keeps the wrap heuristic stable (CJK advances
        // would skew it and reflow every existing golden).
        if (32..=126).contains(&code) || EXTRAS.contains(&code) {
            advance_sum += adv;
            advance_max = advance_max.max(adv);
        }
        glyphs
            .push_str(&format!("({off}, {}, {}, {}, {}, {adv}), ", m.width, m.height, m.xmin, top));
    }
    let n_latin = latin.len() as u32;
    let cov_path = out_dir.join(format!("{}.a8", name.to_lowercase()));
    fs::write(&cov_path, &cov)?;
    writeln!(generated, "pub const {name}_ASCENT: i32 = {ascent};")?;
    writeln!(generated, "pub const {name}_LINE_H: u32 = {line_h};")?;
    writeln!(generated, "pub const {name}_AVG_ADVANCE: u32 = {};", advance_sum / n_latin)?;
    // Part of the baked font metrics API surface; not every consumer reads it.
    writeln!(generated, "#[allow(dead_code)]")?;
    writeln!(generated, "pub const {name}_MAX_ADVANCE: u32 = {advance_max};")?;
    writeln!(
        generated,
        "pub const {name}_COV: &[u8] = include_bytes!(r#\"{}\"#);",
        cov_path.display()
    )?;
    writeln!(
        generated,
        "pub const {name}_GLYPHS: &[(u32, u16, u16, i16, i16, u16)] = &[{glyphs}];"
    )?;
    // The sparse EXTRAS tail (codepoints past ASCII, glyph indices 95..106).
    let extras_list: String = EXTRAS.iter().fold(String::new(), |mut acc, c| {
        let _ = write!(acc, "{c}, ");
        acc
    });
    writeln!(generated, "pub const {name}_EXTRAS: &[u32; {}] = &[{extras_list}];", EXTRAS.len())?;
    // The sorted WIDE tail (glyph indices 106.. — kana/jamo/hangul/han).
    let wide_list: String = wide.iter().fold(String::new(), |mut acc, c| {
        let _ = write!(acc, "{c}, ");
        acc
    });
    writeln!(generated, "pub const {name}_WIDE: &[u32] = &[{wide_list}];")?;
    // Sparse kerning over the LATIN span only (Inter pairs; CJK is unkerned
    // by design and its indices exceed the u8 table anyway).
    let mut kern = String::new();
    for (li, &l) in latin.iter().enumerate() {
        for (ri, &r) in latin.iter().enumerate() {
            let (lc, rc) = (char::from_u32(l).unwrap_or(' '), char::from_u32(r).unwrap_or(' '));
            if let Some(k) = faces.inter.horizontal_kern(lc, rc, px) {
                let kpx = k.round() as i32;
                if kpx != 0 {
                    kern.push_str(&format!("({li}, {ri}, {kpx}), "));
                }
            }
        }
    }
    writeln!(generated, "pub const {name}_KERN: &[(u8, u8, i8)] = &[{kern}];")?;
    Ok(())
}
