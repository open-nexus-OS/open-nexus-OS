// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use nexus_shape::{GlyphRun, PixelSize, ShapeContext, ShapeError};

fn fonts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("resources")
        .join("fonts")
        .join("inter")
}

// ---------------------------------------------------------------------------
// Font loading
// ---------------------------------------------------------------------------

#[test]
fn test_load_fonts_from_directory() {
    let dir = fonts_dir();
    if !dir.exists() {
        // Skip if font files aren't available
        return;
    }
    let ctx = ShapeContext::new(&dir);
    match ctx {
        Ok(ctx) => assert!(ctx.font_count() > 0),
        Err(ShapeError::NoFonts) => {} // acceptable if dir has no ttf files
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn test_empty_directory_is_error() {
    let tmp = std::env::temp_dir().join("nexus-shape-test-empty");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let err = ShapeContext::new(&tmp).unwrap_err();
    assert!(matches!(err, ShapeError::NoFonts));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// Shaping
// ---------------------------------------------------------------------------

#[test]
fn test_shape_latin_ltr() {
    let dir = fonts_dir();
    if !dir.exists() {
        return;
    }
    let ctx = match ShapeContext::new(&dir) {
        Ok(c) => c,
        Err(ShapeError::NoFonts) => return,
        Err(e) => panic!("{e}"),
    };
    let run = ctx.shape("Hello", PixelSize(16), rustybuzz::Direction::LeftToRight).unwrap();
    assert!(!run.glyphs.is_empty());
    assert!(!run.cluster_map.is_empty());
    assert_eq!(run.glyphs.len(), run.cluster_map.len());
}

#[test]
fn test_shape_empty_text() {
    let dir = fonts_dir();
    if !dir.exists() {
        return;
    }
    let ctx = match ShapeContext::new(&dir) {
        Ok(c) => c,
        Err(ShapeError::NoFonts) => return,
        Err(e) => panic!("{e}"),
    };
    let run = ctx.shape("", PixelSize(16), rustybuzz::Direction::LeftToRight).unwrap();
    assert!(run.glyphs.is_empty());
}

#[test]
fn test_shape_determinism() {
    let dir = fonts_dir();
    if !dir.exists() {
        return;
    }
    let ctx = match ShapeContext::new(&dir) {
        Ok(c) => c,
        Err(ShapeError::NoFonts) => return,
        Err(e) => panic!("{e}"),
    };
    let r1 = ctx.shape("Test", PixelSize(16), rustybuzz::Direction::LeftToRight).unwrap();
    let r2 = ctx.shape("Test", PixelSize(16), rustybuzz::Direction::LeftToRight).unwrap();
    assert_eq!(r1.width, r2.width);
    assert_eq!(r1.height, r2.height);
    assert_eq!(r1.glyphs.len(), r2.glyphs.len());
    for (g1, g2) in r1.glyphs.iter().zip(r2.glyphs.iter()) {
        assert_eq!(g1.glyph_index, g2.glyph_index);
        assert_eq!(g1.advance, g2.advance);
    }
}

#[test]
fn test_shape_rtl_arabic() {
    let dir = fonts_dir();
    if !dir.exists() {
        return;
    }
    let ctx = match ShapeContext::new(&dir) {
        Ok(c) => c,
        Err(ShapeError::NoFonts) => return,
        Err(e) => panic!("{e}"),
    };
    // Arabic "Marhaba" (hello)
    let run = ctx.shape("مرحبا", PixelSize(16), rustybuzz::Direction::RightToLeft).unwrap();
    // Should produce some glyphs (may be minimal if font lacks Arabic)
    assert!(!run.glyphs.is_empty());
}

// ---------------------------------------------------------------------------
// Newtype correctness
// ---------------------------------------------------------------------------

#[test]
fn test_newtype_font_id() {
    let a = nexus_shape::FontId(1);
    let b = nexus_shape::FontId(1);
    let c = nexus_shape::FontId(2);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn test_newtype_glyph_index() {
    let a = nexus_shape::GlyphIndex(42);
    let b = nexus_shape::GlyphIndex(42);
    assert_eq!(a, b);
}

#[test]
fn test_newtype_pixel_size() {
    let s = nexus_shape::PixelSize(16);
    assert_eq!(s.to_string(), "16px");
}

// ---------------------------------------------------------------------------
// Invalid font handling
// ---------------------------------------------------------------------------

#[test]
fn test_reject_invalid_font_data() {
    let tmp = std::env::temp_dir().join("nexus-shape-test-bad-font");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Write a file with .ttf extension but garbage content
    std::fs::write(tmp.join("fake.ttf"), b"not a real font").unwrap();
    let err = ShapeContext::new(&tmp).unwrap_err();
    assert!(matches!(err, ShapeError::InvalidFont { .. }));
    let _ = std::fs::remove_dir_all(&tmp);
}
