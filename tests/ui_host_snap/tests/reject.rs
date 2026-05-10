// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Negative host snapshot proofs for fail-closed TASK-0054 behavior.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 15 reject integration tests
//! TEST_SCOPE: Renderer validation, golden update gating, and fixture path rejects.
//! TEST_SCENARIOS: Oversize, invalid stride/dimensions, overflow, invalid rect/damage, path policy.
//! DEPENDENCIES: `ui_host_snap`, `ui_renderer`
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ui_host_snap::{
    artifact_root, golden_root, resolve_under_root, temp_artifact_path, update_hex_golden,
    GoldenMode, SnapshotError,
};
use ui_renderer::{
    Damage, DamageRectCount, FixtureFont, Frame, Image, PixelBgra, Point, Rect, RenderError,
    SurfaceHeight, SurfaceWidth, MAX_FRAME_HEIGHT, MAX_FRAME_WIDTH, MAX_GLYPHS, MAX_IMAGE_HEIGHT,
    MAX_IMAGE_WIDTH,
};

fn collect_rust_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn test_reject_oversized_frame_before_allocation() {
    let err = Frame::new_checked(MAX_FRAME_WIDTH + 1, 1);
    assert_eq!(err, Err(RenderError::FrameTooLarge));

    let err = Frame::new_checked(1, MAX_FRAME_HEIGHT + 1);
    assert_eq!(err, Err(RenderError::FrameTooLarge));
}

#[test]
fn test_reject_oversized_image_before_allocation() {
    let err = Image::from_bgra_checked(MAX_IMAGE_WIDTH + 1, 1, 4, Vec::new());
    assert_eq!(err, Err(RenderError::ImageTooLarge));

    let err = Image::from_bgra_checked(1, MAX_IMAGE_HEIGHT + 1, 4, Vec::new());
    assert_eq!(err, Err(RenderError::ImageTooLarge));
}

#[test]
fn test_reject_invalid_stride() {
    let frame_err = Frame::from_bgra_buffer_checked(2, 2, 8, Vec::new());
    assert_eq!(frame_err, Err(RenderError::InvalidStride));

    let image_err = Image::from_bgra_checked(2, 2, 4, Vec::new());
    assert_eq!(image_err, Err(RenderError::InvalidStride));
}

#[test]
fn test_reject_buffer_length_mismatch_but_accepts_exact_lengths() {
    let frame_ok = Frame::from_bgra_buffer_checked(2, 2, 64, vec![0; 128]);
    assert!(frame_ok.is_ok());
    let frame_err = Frame::from_bgra_buffer_checked(2, 2, 64, vec![0; 127]);
    assert_eq!(frame_err, Err(RenderError::InvalidStride));

    let image_ok = Image::from_bgra_checked(2, 2, 12, vec![0; 24]);
    assert!(image_ok.is_ok());
    let image_err = Image::from_bgra_checked(2, 2, 12, vec![0; 23]);
    assert_eq!(image_err, Err(RenderError::InvalidStride));
}

#[test]
fn test_reject_arithmetic_overflow() {
    let err = Frame::new_checked(u32::MAX, 1);
    assert_eq!(err, Err(RenderError::ArithmeticOverflow));
}

#[test]
fn test_reject_invalid_rect_or_damage_overflow() -> Result<(), Box<dyn Error>> {
    assert_eq!(Rect::new(0, 0, 0, 1), Err(RenderError::InvalidRect));
    assert_eq!(DamageRectCount::new(0), Err(RenderError::DamageOverflow));
    let width = SurfaceWidth::new(4)?;
    let height = SurfaceHeight::new(4)?;
    let mut damage = Damage::for_frame(width, height, DamageRectCount::new(1)?)?;
    damage.add(Rect::new(0, 0, 1, 1)?)?;
    damage.add(Rect::new(3, 3, 1, 1)?)?;
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 4, 4)?]);
    Ok(())
}

#[test]
fn test_reject_golden_update_without_env() {
    let _mode = GoldenMode::from_env();
    let err = update_hex_golden(
        &golden_root(),
        Path::new("blocked.bgra.hex"),
        "00\n",
        GoldenMode::CompareOnly,
    );
    assert_eq!(err, Err(SnapshotError::GoldenUpdateDisabled));
}

#[test]
fn test_golden_update_writes_only_under_safe_root() -> Result<(), Box<dyn Error>> {
    let root = artifact_root()?.join("golden-update");
    fs::create_dir_all(&root)?;
    update_hex_golden(
        &root,
        Path::new("safe.bgra.hex"),
        "aa\n",
        GoldenMode::Update,
    )?;
    assert_eq!(fs::read_to_string(root.join("safe.bgra.hex"))?, "aa\n");
    Ok(())
}

#[test]
fn test_reject_fixture_path_traversal() {
    let err = resolve_under_root(&golden_root(), Path::new("../escape.bgra.hex"));
    assert_eq!(err, Err(SnapshotError::FixturePathRejected));
}

#[test]
fn test_reject_absolute_golden_write_path() {
    let err = update_hex_golden(
        &golden_root(),
        Path::new("/tmp/escape.bgra.hex"),
        "00\n",
        GoldenMode::Update,
    );
    assert_eq!(err, Err(SnapshotError::FixturePathRejected));
}

#[test]
fn test_reject_artifact_path_traversal() {
    let err = temp_artifact_path("../escape.png");
    assert!(err.is_err());
}

#[test]
fn test_reject_invalid_source_image_dimensions() {
    let err = Image::from_bgra_checked(0, 1, 4, Vec::new());
    assert_eq!(err, Err(RenderError::InvalidDimensions));
}

#[test]
fn test_reject_text_unsupported_glyph_and_glyph_run_too_large() -> Result<(), Box<dyn Error>> {
    let font = FixtureFont::load_default()?;
    let mut frame = Frame::new_checked(16, 8)?;
    let mut damage = Damage::for_frame(frame.width(), frame.height(), DamageRectCount::new(4)?)?;
    let err = frame.draw_text(
        Point::new(0, 0),
        "x",
        &font,
        PixelBgra::new(1, 2, 3, 4),
        &mut damage,
    );
    assert_eq!(err, Err(RenderError::Unsupported));

    let too_long = "h".repeat(MAX_GLYPHS + 1);
    let err = frame.draw_text(
        Point::new(0, 0),
        &too_long,
        &font,
        PixelBgra::new(1, 2, 3, 4),
        &mut damage,
    );
    assert_eq!(err, Err(RenderError::GlyphRunTooLarge));
    Ok(())
}

#[test]
fn test_reject_malformed_fixture_font_inputs() {
    let duplicate = "WIDTH 1\nHEIGHT 1\nGLYPH a\n1\nEND\nGLYPH a\n1\nEND\n";
    assert_eq!(
        FixtureFont::parse(duplicate),
        Err(RenderError::FixtureFontRejected)
    );

    let bad_row_width = "WIDTH 2\nHEIGHT 1\nGLYPH a\n1\nEND\n";
    assert_eq!(
        FixtureFont::parse(bad_row_width),
        Err(RenderError::FixtureFontRejected)
    );

    let unterminated = "WIDTH 1\nHEIGHT 1\nGLYPH a\n1\n";
    assert_eq!(
        FixtureFont::parse(unterminated),
        Err(RenderError::FixtureFontRejected)
    );

    let row_without_glyph = "WIDTH 1\nHEIGHT 1\n1\n";
    assert_eq!(
        FixtureFont::parse(row_without_glyph),
        Err(RenderError::FixtureFontRejected)
    );
}

#[test]
fn test_reject_fake_proof_marker_strings_absent_from_host_sources() -> Result<(), Box<dyn Error>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        manifest.join("src"),
        manifest
            .join("..")
            .join("..")
            .join("userspace")
            .join("ui")
            .join("renderer")
            .join("src"),
    ];
    let forbidden = [
        format!("SELFTEST{}", ":"),
        String::from("present ok"),
        String::from("windowd:"),
        String::from("launcher: first frame ok"),
    ];

    let mut files = Vec::new();
    for root in roots {
        collect_rust_files(&root, &mut files)?;
    }
    for file in files {
        let text = fs::read_to_string(&file)?;
        for marker in &forbidden {
            assert!(
                !text.contains(marker),
                "{} contains fake proof marker {marker}",
                file.display()
            );
        }
    }
    Ok(())
}
