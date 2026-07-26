// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bakes the shared A8 glyph atlases from the vendored faces at build
//! time — Inter for Latin, the PINNED Noto Sans CJK faces for kana / hangul /
//! han per the font-library.md fallback contract (RFC-0075 Phase 8d). Since
//! RFC-0082 the bake is a (size, weight) LADDER, and every face declares a
//! CHARSET budget: `Full` (ASCII + Latin EXTRAS + the CJK wide tail) is allowed
//! at 13/16 px only — the full hangul block at display sizes would be hundreds
//! of megabytes — while larger faces are `Latin` or `Digits`. No runtime font
//! parsing anywhere; missing CJK faces fail the build with the fetch hint.
//!
//! Two rasterizers, deliberately: Regular 400 keeps going through `fontdue`
//! (the variable font's default instance) so the pre-RFC-0082 13/16 px
//! coverage stays BYTE-IDENTICAL and no existing layout shifts. Light 300 and
//! SemiBold 600 do not exist as instances `fontdue` can reach (it has no
//! variation-axis API), so they are instanced with `ttf-parser`'s `wght` axis
//! and filled by the scanline rasterizer in `raster` below.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: consumed by `src/lib.rs` unit tests

#[path = "build/raster.rs"]
mod raster;
use raster::{rasterize_instanced, GlyphRaster};

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

/// The codepoints a `Digits` face actually rasterizes (RFC-0082): hero
/// numerals are for clocks, not for prose. Everything else in the ASCII index
/// space is emitted as an EMPTY glyph so the dense index scheme still holds
/// and the reader can fall back to the 16 px `Full` face.
const DIGITS: [u32; 14] = [
    0x20, // space
    0x2D, // -  (the clock's "--:--" placeholder before walltime anchors —
    //           without it the placeholder silently drops to the 16px face)
    0x2E, // .
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, // 0-9
    0x3A, // :
];

/// Which codepoints a face carries real coverage for. The budget rule from
/// RFC-0082: `Full` is forbidden above 16 px.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Full,
    Latin,
    Digits,
}

/// One baked face. `weight` is the `wght` axis coordinate; 400 goes through
/// `fontdue`, everything else through the instanced path.
struct FaceSpec {
    name: &'static str,
    px: f32,
    weight: u16,
    charset: Charset,
}

/// The ladder (RFC-0082 Phase 0). Order is the atlas concatenation order;
/// `FONT13`/`FONT16` MUST stay first and unchanged so their coverage bytes
/// keep their historical offsets.
const FACES: &[FaceSpec] = &[
    FaceSpec { name: "FONT13", px: 13.0, weight: 400, charset: Charset::Full },
    FaceSpec { name: "FONT16", px: 16.0, weight: 400, charset: Charset::Full },
    FaceSpec { name: "FONT13_SEMI", px: 13.0, weight: 600, charset: Charset::Latin },
    FaceSpec { name: "FONT16_SEMI", px: 16.0, weight: 600, charset: Charset::Latin },
    FaceSpec { name: "FONT21", px: 21.0, weight: 400, charset: Charset::Latin },
    FaceSpec { name: "FONT21_SEMI", px: 21.0, weight: 600, charset: Charset::Latin },
    FaceSpec { name: "FONT36_SEMI", px: 36.0, weight: 600, charset: Charset::Latin },
    FaceSpec { name: "FONT120_LIGHT", px: 120.0, weight: 300, charset: Charset::Digits },
];

// -------------------------------------------------------------------- fonts

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
    /// Raw bytes of the variable UI face — re-parsed per weight because
    /// `set_variation` mutates the face.
    inter_bytes: Vec<u8>,
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

    /// The UI face instanced at `weight` on the `wght` axis.
    fn instanced(&self, weight: u16) -> ttf_parser::Face<'_> {
        let mut face = ttf_parser::Face::parse(&self.inter_bytes, 0)
            .unwrap_or_else(|err| panic!("ttf-parser cannot parse {UI_FONT}: {err:?}"));
        assert!(
            face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(weight)).is_some(),
            "{UI_FONT} has no `wght` axis — the ladder needs a variable UI face"
        );
        face
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
    // Calendar names the app-host's clock can emit (TASK-0305). These live in
    // HOST Rust, not in any app's i18n catalog, so the catalog sweep below
    // would never see them and every CJK date would render as `?`.
    for ch in "月火水木金土日曜星期".chars() {
        set.insert(ch as u32);
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
        inter_bytes: fs::read(UI_FONT)?,
    };
    let wide = wide_charset();
    let mut generated = File::create(out_dir.join("baked_fonts.rs"))?;
    // Bake each face's coverage and concatenate into ONE atlas blob (ladder
    // order) so the runtime can back it with a single shared RO VMO
    // (RFC-0080). Each face resolves `cov` as `atlas[offset .. offset+len]`.
    let mut atlas: Vec<u8> = Vec::new();
    for spec in FACES {
        assert!(
            !(spec.charset == Charset::Full && spec.px > 16.0),
            "{}: a Full charset above 16px blows the shared atlas (RFC-0082 budget)",
            spec.name
        );
        let cov = emit_glyph_atlas(&mut generated, &faces, &wide, spec, atlas.len())?;
        atlas.extend_from_slice(&cov);
    }
    let atlas_path = out_dir.join("atlas.a8");
    fs::write(&atlas_path, &atlas)?;
    writeln!(generated, "pub const ATLAS_LEN: usize = {};", atlas.len())?;
    // The embedded blob is the DEFAULT backing (feature `embedded-atlas`, on
    // for windowd/host/tests + the VMO owner). Consumers that map a shared
    // atlas VMO build with it OFF, so the blob never enters their image.
    if std::env::var_os("CARGO_FEATURE_EMBEDDED_ATLAS").is_some() {
        writeln!(
            generated,
            "pub const EMBEDDED_ATLAS: &[u8] = include_bytes!(r#\"{}\"#);",
            atlas_path.display()
        )?;
    }
    Ok(())
}

/// One face at one size/weight: coverage blob + per-glyph placement + metrics,
/// sparse Latin kerning, and the sorted WIDE tail.
///
/// Layout per glyph: `(cov_offset, w, h, left_bearing, top_from_band_top,
/// advance_px)`. All glyphs share Inter's Regular baseline (top is computed
/// against it) so mixed-script AND mixed-face runs align.
fn emit_glyph_atlas(
    generated: &mut File,
    faces: &Faces,
    wide: &[u32],
    spec: &FaceSpec,
    cov_offset: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (name, px) = (spec.name, spec.px);
    let latin: Vec<u32> = (32u32..=126).chain(EXTRAS.iter().copied()).collect();
    // A `Latin`/`Digits` face carries no wide tail; the reader falls back to
    // the 16 px `Full` face for those codepoints (RFC-0082).
    let face_wide: &[u32] = if spec.charset == Charset::Full { wide } else { &[] };
    let charset: Vec<u32> = latin.iter().copied().chain(face_wide.iter().copied()).collect();
    // Line metrics always come from Inter Regular at this size: `wght` does
    // not move Inter's vertical metrics, and one source keeps every face's
    // baseline on the same rule.
    let lm = faces
        .inter
        .horizontal_line_metrics(px)
        .ok_or_else(|| format!("{name}: no horizontal line metrics"))?;
    let ascent = lm.ascent.round() as i32;
    let line_h = (lm.ascent - lm.descent).ceil() as u32;

    let instanced = (spec.weight != 400).then(|| faces.instanced(spec.weight));

    // Rasterize first, so the Digits face can equalize its numeral advances
    // (tabular figures — a ticking clock must not shuffle sideways).
    let mut rasters: Vec<GlyphRaster> = Vec::with_capacity(charset.len());
    for &code in &charset {
        let ch = char::from_u32(code).ok_or("charset codepoint")?;
        let real = match spec.charset {
            Charset::Full | Charset::Latin => true,
            Charset::Digits => DIGITS.contains(&code),
        };
        if !real {
            rasters.push(GlyphRaster::absent());
            continue;
        }
        rasters.push(match &instanced {
            Some(face) => rasterize_instanced(face, ch, px),
            None => {
                let (m, bitmap) = faces.for_code(code).rasterize(ch, px);
                GlyphRaster {
                    width: m.width,
                    height: m.height,
                    xmin: m.xmin,
                    ymin: m.ymin,
                    advance: m.advance_width,
                    bitmap,
                }
            }
        });
    }
    if spec.charset == Charset::Digits {
        equalize_digit_advances(&charset, &mut rasters);
    }

    let mut cov: Vec<u8> = Vec::new();
    let mut glyphs = String::new();
    let mut advance_sum = 0u32;
    let mut advance_n = 0u32;
    let mut advance_max = 0u32;
    for (&code, r) in charset.iter().zip(&rasters) {
        let off = cov.len() as u32;
        cov.extend_from_slice(&r.bitmap);
        let top = ascent - (r.ymin + r.height as i32);
        // An absent glyph keeps advance 0 — that is exactly how the reader
        // recognizes "not in this face's charset" and falls back.
        let adv =
            if r.width == 0 && r.advance <= 0.0 { 0 } else { r.advance.round().max(1.0) as u32 };
        // Latin-only average keeps the wrap heuristic stable (CJK advances
        // would skew it and reflow every existing golden).
        if ((32..=126).contains(&code) || EXTRAS.contains(&code)) && adv > 0 {
            advance_sum += adv;
            advance_n += 1;
            advance_max = advance_max.max(adv);
        }
        let _ = write!(glyphs, "({off}, {}, {}, {}, {top}, {adv}), ", r.width, r.height, r.xmin);
    }
    writeln!(generated, "pub const {name}_ASCENT: i32 = {ascent};")?;
    writeln!(generated, "pub const {name}_LINE_H: u32 = {line_h};")?;
    writeln!(generated, "pub const {name}_AVG_ADVANCE: u32 = {};", advance_sum / advance_n.max(1))?;
    // Part of the baked font metrics API surface; not every consumer reads it.
    writeln!(generated, "#[allow(dead_code)]")?;
    writeln!(generated, "pub const {name}_MAX_ADVANCE: u32 = {advance_max};")?;
    // This face's coverage lives at `atlas[COV_OFFSET .. COV_OFFSET + COV_LEN]`
    // (RFC-0080 — the runtime resolves it from the shared VMO or the embedded
    // blob). Per-glyph `off` stays 0-based WITHIN this face's slice.
    writeln!(generated, "pub const {name}_COV_OFFSET: usize = {cov_offset};")?;
    writeln!(generated, "pub const {name}_COV_LEN: usize = {};", cov.len())?;
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
    let wide_list: String = face_wide.iter().fold(String::new(), |mut acc, c| {
        let _ = write!(acc, "{c}, ");
        acc
    });
    writeln!(generated, "pub const {name}_WIDE: &[u32] = &[{wide_list}];")?;
    // Sparse kerning over the LATIN span only (Inter pairs; CJK is unkerned
    // by design and its indices exceed the u8 table anyway). Kerning comes
    // from the Regular instance for every weight — `wght` shifts pair values
    // by well under a pixel at these sizes, and one table keeps wrap and
    // paint on the same numbers. A Digits face stays UNKERNED (tabular).
    let mut kern = String::new();
    if spec.charset != Charset::Digits {
        for (li, &l) in latin.iter().enumerate() {
            for (ri, &r) in latin.iter().enumerate() {
                let (lc, rc) = (char::from_u32(l).unwrap_or(' '), char::from_u32(r).unwrap_or(' '));
                if let Some(k) = faces.inter.horizontal_kern(lc, rc, px) {
                    let kpx = k.round() as i32;
                    if kpx != 0 {
                        let _ = write!(kern, "({li}, {ri}, {kpx}), ");
                    }
                }
            }
        }
    }
    writeln!(generated, "pub const {name}_KERN: &[(u8, u8, i8)] = &[{kern}];")?;
    Ok(cov)
}

/// Tabular figures: give every numeral the widest numeral's advance and
/// center its bitmap in that cell. `:` and `.` keep their natural width —
/// that is what `font-variant-numeric: tabular-nums` does, and it is all a
/// ticking clock needs to stop shuffling.
fn equalize_digit_advances(charset: &[u32], rasters: &mut [GlyphRaster]) {
    let widest = charset
        .iter()
        .zip(rasters.iter())
        .filter(|(&c, _)| (0x30..=0x39).contains(&c))
        .map(|(_, r)| r.advance)
        .fold(0f32, f32::max);
    for (&code, r) in charset.iter().zip(rasters.iter_mut()) {
        if !(0x30..=0x39).contains(&code) {
            continue;
        }
        let slack = widest - r.advance;
        if slack > 0.0 {
            r.xmin += (slack / 2.0).round() as i32;
            r.advance = widest;
        }
    }
}
