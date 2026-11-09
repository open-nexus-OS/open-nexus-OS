// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bakes the shared A8 glyph atlas from the vendored faces at build
//! time — Inter for Latin, the PINNED Noto Sans CJK faces for kana / hangul /
//! han per the font-library.md fallback contract (RFC-0075 Phase 8d). The
//! faces are the design's text styles (RFC-0116): one (size, weight) face per
//! style default and per weight override the apps use, nothing else. Every
//! face declares a CHARSET budget (RFC-0082): `Full` (ASCII + Latin EXTRAS +
//! the CJK wide tail) only at 13 and 14 px — the full hangul block at display
//! sizes would be hundreds of megabytes — while every other face is `Latin`
//! or `Digits` and falls back to the 14 px `Full` face for what it lacks. No
//! runtime font parsing anywhere; missing CJK faces fail the build with the
//! fetch hint.
//!
//! Two rasterizers, deliberately: Regular 400 goes through `fontdue` (the
//! variable font's default instance), so the 13 px `Full` face stays
//! BYTE-IDENTICAL to every earlier build. The other weights (300, 500, 600,
//! 700) do not exist as instances `fontdue` can reach (it has no
//! variation-axis API), so they are instanced on `ttf-parser`'s `wght` axis
//! and filled by the scanline rasterizer in `build/raster.rs`.
//!
//! Each face is written out as a complete `crate::ladder::Face` record
//! (`S14W400`, …): what is baked and what the reader walks come from this one
//! table, and `FontSize` in `src/ladder.rs` only names the records.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: consumed by the `src/` unit tests (`src/tests.rs`, `src/ladder.rs`)

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

/// Latin EXTRAS (umlauts/ß, calculator math symbols, the breadcrumb
/// separator). MUST stay sorted ascending and < 256 total glyph indices with
/// ASCII (kern indices are u8).
const EXTRAS: [u32; 14] = [
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
    // Text that does not fit its box is CUT by the row painter. Without this
    // glyph the cut was invisible and a label just stopped mid-word.
    0x2026, // …
    0x2039, // ‹
    // The `Breadcrumbs` widget's separator. Without it the path trail printed
    // a placeholder box between every crumb.
    0x203A, // ›
    0x2212, // −
];

/// The codepoints a `Digits` face actually rasterizes: what the `display` and
/// `clock` styles show — the lock-screen clock and the calculator display. Everything
/// else in the Latin index space is emitted as an EMPTY glyph so the dense
/// index scheme still holds and the reader falls back to the 14 px `Full`
/// face.
const DIGITS: [u32; 21] = [
    0x20, // space
    0x25, // %
    0x2B, // +
    0x2C, // ,
    0x2D, // -  (the clock's "--:--" placeholder before walltime anchors)
    0x2E, // .
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, // 0-9
    0x3A, // :
    0x3D, // =
    0xD7, // ×
    0xF7, // ÷
    // The calculator's minus sign.
    0x2212,
];

/// Which codepoints a face carries real coverage for. The budget rule from
/// RFC-0082: `Full` is forbidden above 16 px.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charset {
    Full,
    Latin,
    Digits,
}

/// One baked face: its px size, its `wght` axis coordinate (400 goes through
/// `fontdue`, every other weight through the instanced path) and its charset.
struct FaceSpec {
    px: u16,
    weight: u16,
    charset: Charset,
}

impl FaceSpec {
    /// The generated record's name — spelled like the `FontSize` variant
    /// that names it (`S14W400`).
    fn name(&self) -> String {
        format!("S{}W{}", self.px, self.weight)
    }
}

const fn spec(px: u16, weight: u16, charset: Charset) -> FaceSpec {
    FaceSpec { px, weight, charset }
}

/// The faces: every RFC-0116 text style at its default weight, plus the
/// weight overrides the apps use. Order is the atlas concatenation order; the
/// 13 px `Full` face MUST stay first and unchanged so its coverage bytes keep
/// offset 0.
const FACES: &[FaceSpec] = &[
    spec(13, 400, Charset::Full),    // subheadline (CJK)
    spec(14, 400, Charset::Full),    // body (CJK) — the fallback face
    spec(10, 500, Charset::Latin),   // caption2
    spec(10, 700, Charset::Latin),   // caption2 + bold
    spec(11, 500, Charset::Latin),   // caption
    spec(11, 600, Charset::Latin),   // overline, caption + semibold
    spec(12, 400, Charset::Latin),   // footnote
    spec(12, 500, Charset::Latin),   // footnote + medium
    spec(12, 600, Charset::Latin),   // footnote + semibold
    spec(13, 500, Charset::Latin),   // subheadline + medium
    spec(13, 600, Charset::Latin),   // subheadline + semibold
    spec(14, 500, Charset::Latin),   // body + medium (the md control label)
    spec(14, 600, Charset::Latin),   // body + semibold
    spec(15, 600, Charset::Latin),   // headline
    spec(17, 600, Charset::Latin),   // title3
    spec(20, 400, Charset::Latin),   // title2 + normal
    spec(20, 500, Charset::Latin),   // title2 + medium
    spec(20, 600, Charset::Latin),   // title2
    spec(24, 700, Charset::Latin),   // title1
    spec(30, 700, Charset::Latin),   // largeTitle
    spec(64, 300, Charset::Digits),  // display: the calculator, the phone's lock clock
    spec(128, 300, Charset::Digits), // clock: the lock screen's time (RFC-0116 `clock`)
];

/// The face a `Latin`/`Digits` face falls back to for a codepoint it lacks
/// (CJK in a title, a letter on the display face): `body`, 14 px `Full`.
const FALLBACK: (u16, u16) = (14, 400);

/// The table's rules, checked before anything is rasterized.
fn validate() {
    let first = &FACES[0];
    assert!(
        (first.px, first.weight) == (13, 400) && first.charset == Charset::Full,
        "the 13 px Full face must stay first: its coverage keeps offset 0"
    );
    assert!(FACES.len() <= usize::from(u8::MAX), "face ids are u8");
    for (i, face) in FACES.iter().enumerate() {
        let name = face.name();
        assert!(
            !(face.charset == Charset::Full && face.px > 16),
            "{name}: a Full charset above 16px blows the shared atlas (RFC-0082 budget)"
        );
        assert!(matches!(face.weight, 300 | 400 | 500 | 600 | 700), "{name}: no such weight");
        assert!(
            FACES[..i].iter().all(|f| (f.px, f.weight) != (face.px, face.weight)),
            "{name} is baked twice"
        );
    }
    assert!(
        FACES.iter().any(|f| (f.px, f.weight) == FALLBACK && f.charset == Charset::Full),
        "the fallback face must be baked with the Full charset"
    );
}

/// The runtime `Weight` variant a `wght` coordinate reads as.
fn weight_variant(weight: u16) -> &'static str {
    match weight {
        300 => "Light",
        400 => "Regular",
        500 => "Medium",
        600 => "SemiBold",
        700 => "Bold",
        other => panic!("no Weight variant for wght {other}"),
    }
}

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

/// `&[a, b, c, ]` source text for a list of codepoints.
fn list(codes: &[u32]) -> String {
    codes.iter().fold(String::new(), |mut acc, c| {
        let _ = write!(acc, "{c}, ");
        acc
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    validate();
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
    // The sparse EXTRAS tail (codepoints past ASCII, glyph indices 95..109),
    // one list every face shares.
    writeln!(generated, "pub const EXTRAS: &[u32] = &[{}];", list(&EXTRAS))?;
    writeln!(generated, "pub const FACE_COUNT: usize = {};", FACES.len())?;
    // Bake each face's coverage and concatenate into ONE atlas blob (table
    // order) so the runtime can back it with a single shared RO VMO
    // (RFC-0080). Each face resolves its coverage as `atlas[off .. off+len]`.
    let mut atlas: Vec<u8> = Vec::new();
    for (id, spec) in FACES.iter().enumerate() {
        let cov = emit_face(&mut generated, &faces, &wide, spec, id, atlas.len())?;
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

/// One face at one size/weight: the coverage blob (returned) plus its
/// generated record — per-glyph placement, line metrics, sparse Latin
/// kerning, the sorted WIDE tail and the fallback link.
///
/// Layout per glyph: `(cov_offset, w, h, left_bearing, top_from_band_top,
/// advance_px)`. All glyphs share Inter's Regular baseline (top is computed
/// against it) so mixed-script AND mixed-face runs align.
fn emit_face(
    generated: &mut File,
    faces: &Faces,
    wide: &[u32],
    spec: &FaceSpec,
    id: usize,
    cov_offset: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let name = spec.name();
    let px = f32::from(spec.px);
    let latin: Vec<u32> = (32u32..=126).chain(EXTRAS.iter().copied()).collect();
    // A `Latin`/`Digits` face carries no wide tail; the reader falls back to
    // the 14 px `Full` face for those codepoints.
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
        }
        let _ = write!(glyphs, "({off}, {}, {}, {}, {top}, {adv}), ", r.width, r.height, r.xmin);
    }
    writeln!(
        generated,
        "pub const {name}_GLYPHS: &[(u32, u16, u16, i16, i16, u16)] = &[{glyphs}];"
    )?;
    // The sorted WIDE tail (glyph indices 109.. — kana/jamo/hangul/han).
    writeln!(generated, "pub const {name}_WIDE: &[u32] = &[{}];", list(face_wide))?;
    // Sparse kerning over the LATIN span only (CJK is unkerned by design and
    // its indices exceed the u8 table anyway). Kerning comes from the Regular
    // instance for every weight — `wght` shifts pair values by well under a
    // pixel at these sizes, and one table keeps wrap and paint on the same
    // numbers. A Digits face stays UNKERNED (tabular).
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
    // The record the reader walks. Its coverage lives at
    // `atlas[cov_off .. cov_off + cov_len]` (RFC-0080 — resolved from the
    // shared VMO or the embedded blob); per-glyph `off` is 0-based WITHIN it.
    let fallback = if spec.charset == Charset::Full {
        String::from("None")
    } else {
        format!("Some(&S{}W{})", FALLBACK.0, FALLBACK.1)
    };
    writeln!(
        generated,
        "pub const {name}: crate::ladder::Face = crate::ladder::Face {{ id: {id}, px: {}, \
         weight: crate::ladder::Weight::{}, cov_off: {cov_offset}, cov_len: {}, \
         glyphs: {name}_GLYPHS, extras: EXTRAS, wide: {name}_WIDE, kern: {name}_KERN, \
         line_h: {line_h}, avg_advance: {}, ascent: {ascent}, fallback: {fallback} }};",
        spec.px,
        weight_variant(spec.weight),
        cov.len(),
        advance_sum / advance_n.max(1),
    )?;
    Ok(cov)
}

/// Tabular figures: give every numeral the widest numeral's advance and
/// center its bitmap in that cell. Punctuation and operators keep their
/// natural width — that is what `font-variant-numeric: tabular-nums` does,
/// and it is all a ticking clock needs to stop shuffling.
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
