// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Positive host snapshot behavior proofs for TASK-0054 renderer primitives.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 positive renderer behavior tests
//! TEST_SCOPE: Public `ui_renderer` behavior through host snapshot fixtures.
//! TEST_SCENARIOS: clear, clipped rect, rounded rect, blit, fixture-font text, stride, damage.
//! DEPENDENCIES: `ui_host_snap`, `ui_renderer`
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use std::error::Error;

use ui_host_snap::make_damage;
use ui_renderer::{
    Damage, DamageRectCount, FixtureFont, Frame, Image, PixelBgra, Point, Rect, SurfaceHeight,
    SurfaceWidth,
};

fn assert_mask(
    frame: &Frame,
    rows: &[&str],
    on: PixelBgra,
    off: PixelBgra,
) -> Result<(), Box<dyn Error>> {
    for (y, row) in rows.iter().enumerate() {
        for (x, bit) in row.chars().enumerate() {
            let expected = if bit == '1' { on } else { off };
            assert_eq!(frame.pixel(u32::try_from(x)?, u32::try_from(y)?)?, expected);
        }
    }
    Ok(())
}

#[test]
fn clear_fills_expected_bgra_pixels() -> Result<(), Box<dyn Error>> {
    let mut frame = Frame::new_checked(4, 3)?;
    let mut damage = make_damage(&frame, 4)?;
    let color = PixelBgra::from_rgba(0x33, 0x22, 0x11, 0xff);
    frame.clear(color, &mut damage)?;
    for y in 0..3 {
        for x in 0..4 {
            assert_eq!(frame.pixel(x, y)?, color);
        }
    }
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 4, 3)?]);
    Ok(())
}

#[test]
fn rect_clips_and_damages_expected_region() -> Result<(), Box<dyn Error>> {
    let mut frame = Frame::new_checked(4, 4)?;
    let mut damage = make_damage(&frame, 4)?;
    let black = PixelBgra::new(0, 0, 0, 0xff);
    let green = PixelBgra::from_rgba(0, 0xff, 0, 0xff);
    frame.clear(black, &mut damage)?;
    let mut damage = make_damage(&frame, 4)?;
    frame.draw_rect(Rect::new(-1, 1, 3, 2)?, green, &mut damage)?;
    for y in 0..4 {
        for x in 0..4 {
            let expected = if x <= 1 && (1..=2).contains(&y) { green } else { black };
            assert_eq!(frame.pixel(x, y)?, expected);
        }
    }
    assert_eq!(damage.rects(), &[Rect::new(0, 1, 2, 2)?]);
    Ok(())
}

#[test]
fn rounded_rect_has_documented_deterministic_coverage() -> Result<(), Box<dyn Error>> {
    let mut frame = Frame::new_checked(5, 5)?;
    let mut damage = make_damage(&frame, 4)?;
    let white = PixelBgra::from_rgba(0xff, 0xff, 0xff, 0xff);
    frame.draw_rounded_rect(Rect::new(0, 0, 5, 5)?, 2, white, &mut damage)?;
    assert_mask(
        &frame,
        &["00100", "01110", "11111", "01110", "00100"],
        white,
        PixelBgra::new(0, 0, 0, 0),
    )?;
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 5, 5)?]);
    Ok(())
}

#[test]
fn blit_copies_expected_in_memory_image_pixels() -> Result<(), Box<dyn Error>> {
    let src = Image::from_bgra_checked(
        2,
        2,
        8,
        vec![
            1, 2, 3, 4, 5, 6, 7, 8, //
            9, 10, 11, 12, 13, 14, 15, 16,
        ],
    )?;
    let mut frame = Frame::new_checked(4, 4)?;
    let mut damage = make_damage(&frame, 4)?;
    frame.blit(Point::new(1, 1), &src, &mut damage)?;
    assert_eq!(frame.pixel(1, 1)?, PixelBgra::new(1, 2, 3, 4));
    assert_eq!(frame.pixel(2, 1)?, PixelBgra::new(5, 6, 7, 8));
    assert_eq!(frame.pixel(1, 2)?, PixelBgra::new(9, 10, 11, 12));
    assert_eq!(frame.pixel(2, 2)?, PixelBgra::new(13, 14, 15, 16));
    assert_eq!(damage.rects(), &[Rect::new(1, 1, 2, 2)?]);
    Ok(())
}

#[test]
fn blit_clips_at_destination_edge_and_ignores_source_padding() -> Result<(), Box<dyn Error>> {
    let src = Image::from_bgra_checked(
        2,
        2,
        12,
        vec![
            21, 22, 23, 24, 31, 32, 33, 34, 0xaa, 0xbb, 0xcc, 0xdd, //
            41, 42, 43, 44, 51, 52, 53, 54, 0xee, 0xff, 0x11, 0x22,
        ],
    )?;
    let mut frame = Frame::new_checked(4, 4)?;
    let mut damage = make_damage(&frame, 4)?;
    frame.blit(Point::new(3, 3), &src, &mut damage)?;

    assert_eq!(frame.pixel(3, 3)?, PixelBgra::new(21, 22, 23, 24));
    assert_eq!(frame.pixel(2, 3)?, PixelBgra::new(0, 0, 0, 0));
    assert_eq!(frame.pixel(3, 2)?, PixelBgra::new(0, 0, 0, 0));
    assert_eq!(damage.rects(), &[Rect::new(3, 3, 1, 1)?]);
    Ok(())
}

#[test]
fn fixture_font_text_renders_hello_world_deterministically() -> Result<(), Box<dyn Error>> {
    let font = FixtureFont::load_default()?;
    let mut frame = Frame::new_checked(80, 10)?;
    let mut damage = make_damage(&frame, 4)?;
    let white = PixelBgra::from_rgba(0xff, 0xff, 0xff, 0xff);
    frame.draw_text(Point::new(0, 0), "hello world", &font, white, &mut damage)?;

    assert_mask(
        &frame,
        &[
            "10000000000011000011000000000000000000000000000000000011000000001",
            "10000001110001000001000001110000000010001001110010110001000000001",
            "10110010001001000001000010001000000010001010001011001001000001101",
            "11001011111001000001000010001000000010001010001010000001000010011",
            "10001010000001000001000010001000000010101010001010000001000010001",
            "10001010001001000001000010001000000010101010001010000001000010001",
            "10001001110011100011100001110000000001010001110010000011100001111",
        ],
        white,
        PixelBgra::new(0, 0, 0, 0),
    )?;
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 65, 7)?]);
    Ok(())
}

#[test]
fn stride_is_64_byte_aligned_and_buffer_length_exact() -> Result<(), Box<dyn Error>> {
    let frame = Frame::new_checked(17, 3)?;
    assert_eq!(frame.stride().get() % 64, 0);
    assert!(frame.stride().get() >= 17 * 4);
    assert_eq!(frame.buffer().len(), frame.stride().get() as usize * 3);
    Ok(())
}

#[test]
fn damage_coalescing_and_overflow_follow_documented_rule() -> Result<(), Box<dyn Error>> {
    let width = SurfaceWidth::new(8)?;
    let height = SurfaceHeight::new(8)?;
    let mut damage = Damage::for_frame(width, height, DamageRectCount::new(2)?)?;
    damage.add(Rect::new(0, 0, 2, 2)?)?;
    damage.add(Rect::new(2, 0, 2, 2)?)?;
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 4, 2)?]);
    damage.add(Rect::new(0, 5, 1, 1)?)?;
    assert_eq!(damage.rects().len(), 2);
    damage.add(Rect::new(7, 7, 1, 1)?)?;
    assert_eq!(damage.rects(), &[Rect::new(0, 0, 8, 8)?]);
    Ok(())
}
