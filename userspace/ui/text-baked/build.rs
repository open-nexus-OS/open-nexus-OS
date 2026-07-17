// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bakes the shared A8 glyph atlases (13px/16px, ASCII 32..=126 +
//! sparse kerning) from the vendored UI face at build time — PROMOTED from
//! windowd's build.rs (RFC-0067 P5 discipline: one text SSOT, windowd and
//! the app-host are clients). No runtime font parsing anywhere.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: consumed by `src/lib.rs` unit tests

use std::fs::{self, File};
use std::io::Write as _;
use std::path::Path;

const UI_FONT: &str = "../../../resources/fonts/inter/docs/font-files/InterVariable.ttf";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={UI_FONT}");
    let out_dir = std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let out_dir = Path::new(&out_dir);
    let font_bytes = fs::read(UI_FONT)?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|err| std::io::Error::other(format!("parse UI font: {err:?}")))?;
    let mut generated = File::create(out_dir.join("baked_fonts.rs"))?;
    emit_glyph_atlas(&mut generated, out_dir, &font, 13.0, "FONT13")?;
    emit_glyph_atlas(&mut generated, out_dir, &font, 16.0, "FONT16")?;
    Ok(())
}

/// One face: coverage blob (`<name>.a8`) + per-glyph placement + metrics +
/// sparse kerning. Layout per glyph:
/// `(cov_offset, w, h, left_bearing, top_from_band_top, advance_px)`.
fn emit_glyph_atlas(
    generated: &mut File,
    out_dir: &Path,
    font: &fontdue::Font,
    px: f32,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Sparse charset: dense ASCII (32..=126) + the non-contiguous extras the
    // UI actually renders (German umlauts/ß + the calculator math symbols).
    // MUST stay sorted ascending (the runtime lookup binary-searches the
    // tail) and < 256 entries (kern indices are u8).
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
    let charset: Vec<u32> = (32u32..=126).chain(EXTRAS.iter().copied()).collect();
    let lm = font
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
        let (m, bitmap) = font.rasterize(ch, px);
        let off = cov.len() as u32;
        cov.extend_from_slice(&bitmap);
        let top = ascent - (m.ymin + m.height as i32);
        let adv = m.advance_width.round().max(1.0) as u32;
        advance_sum += adv;
        advance_max = advance_max.max(adv);
        glyphs
            .push_str(&format!("({off}, {}, {}, {}, {}, {adv}), ", m.width, m.height, m.xmin, top));
    }
    let n = charset.len() as u32;
    let cov_path = out_dir.join(format!("{}.a8", name.to_lowercase()));
    fs::write(&cov_path, &cov)?;
    writeln!(generated, "pub const {name}_ASCENT: i32 = {ascent};")?;
    writeln!(generated, "pub const {name}_LINE_H: u32 = {line_h};")?;
    writeln!(generated, "pub const {name}_AVG_ADVANCE: u32 = {};", advance_sum / n)?;
    writeln!(generated, "pub const {name}_MAX_ADVANCE: u32 = {advance_max};")?;
    writeln!(
        generated,
        "pub const {name}_COV: &[u8] = include_bytes!(r#\"{}\"#);",
        cov_path.display()
    )?;
    writeln!(
        generated,
        "pub const {name}_GLYPHS: &[(u32, u16, u16, i16, i16, u16); {n}] = &[{glyphs}];"
    )?;
    // The sparse EXTRAS tail (codepoints past ASCII, in glyph-index order
    // 95.. — the runtime maps ASCII by offset and looks these up).
    let extras_list: String = EXTRAS.iter().map(|c| format!("{c}, ")).collect();
    writeln!(generated, "pub const {name}_EXTRAS: &[u32; {}] = &[{extras_list}];", EXTRAS.len())?;
    // Sparse kerning: only pairs whose kern rounds to a non-zero pixel count
    // at this size (most round to 0 at 13–16 px). Indices are glyph indices.
    let mut kern = String::new();
    for (li, &l) in charset.iter().enumerate() {
        for (ri, &r) in charset.iter().enumerate() {
            let (lc, rc) = (char::from_u32(l).unwrap_or(' '), char::from_u32(r).unwrap_or(' '));
            if let Some(k) = font.horizontal_kern(lc, rc, px) {
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
