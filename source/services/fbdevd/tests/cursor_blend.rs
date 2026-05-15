// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for cursor blending in the framebuffer backend.

use fbdevd::blend_cursor_row;

#[test]
fn blend_cursor_row_replaces_opaque_pixels() {
    // Create a white row (like the checkerboard white square)
    let mut row = vec![0xffu8; 40 * 4]; // 40 pixels, all white

    // Create a 32x32 dark cursor bitmap (dark blue #1a1a2e)
    let cw = 32u32;
    let ch = 32u32;
    let mut bitmap = vec![0u8; (cw * ch * 4) as usize];
    for y in 0..ch {
        for x in 0..cw {
            let idx = ((y * cw + x) * 4) as usize;
            bitmap[idx] = 0x2e; // B
            bitmap[idx + 1] = 0x1a; // G
            bitmap[idx + 2] = 0x1a; // R
            bitmap[idx + 3] = 0xff; // A (fully opaque)
        }
    }

    // Blend at position (10, 0) — row 0 should get cursor pixels starting at col 10
    blend_cursor_row(&mut row, 0, &bitmap, cw, ch, 10, 0);

    // Pixels before column 10 should still be white (cursor starts at x=10)
    assert_eq!(row[0], 0xff, "pixel 0 B should be white");

    // Pixels at column 10-41 should be dark blue (cursor, 32px wide)
    let idx = 10 * 4;
    assert_eq!(row[idx], 0x2e, "cursor pixel B should be 0x2e");
    assert_eq!(row[idx + 1], 0x1a, "cursor pixel G should be 0x1a");
    assert_eq!(row[idx + 2], 0x1a, "cursor pixel R should be 0x1a");
    assert_eq!(row[idx + 3], 0xff, "cursor pixel A should be opaque");

    // Pixel at column 9 (just before cursor) should still be white
    let before_idx = 9 * 4;
    assert_eq!(row[before_idx], 0xff, "pixel before cursor should be white");
}

#[test]
fn blend_cursor_row_skips_transparent_pixels() {
    let mut row = vec![0x10u8; 10 * 4]; // gray row

    // Cursor with transparent pixels
    let bitmap = vec![
        0x00, 0x00, 0xff, 0x80, // half-transparent red
        0x00, 0xff, 0x00, 0x00, // fully transparent green
    ];

    blend_cursor_row(&mut row, 0, &bitmap, 2, 1, 0, 0);

    // First pixel: half-transparent red (B=0,G=0,R=0xff,A=0x80) blended on gray (0x10)
    // B: (0*128 + 16*127)/255 ≈ 7
    assert_eq!(row[0], 7); // B of half-transparent red blended on gray
                           // R: (255*128 + 16*127)/255 ≈ 135
    assert_eq!(row[2], 135); // R of half-transparent red blended on gray
                             // Second pixel: unchanged (fully transparent, A=0, skipped)
    assert_eq!(row[4], 0x10); // B of second pixel unchanged
}

#[test]
fn blend_cursor_row_ignores_out_of_bounds() {
    let mut row = vec![0xffu8; 5 * 4];
    let bitmap = vec![0x00u8; 10 * 10 * 4]; // empty bitmap

    // Cursor at position (-10, -10) — entirely out of bounds
    blend_cursor_row(&mut row, 0, &bitmap, 10, 10, -10, -10);

    // Row should be unchanged
    assert!(row.iter().all(|&b| b == 0xff), "row should be unchanged");
}

#[test]
fn blend_cursor_row_at_different_row() {
    let mut row0 = vec![0xffu8; 10 * 4];
    let mut row5 = vec![0xffu8; 10 * 4];
    let bitmap = vec![0x2e, 0x1a, 0x1a, 0xff]; // single dark pixel

    blend_cursor_row(&mut row0, 0, &bitmap, 1, 10, 0, 5); // cursor at y=5
    blend_cursor_row(&mut row5, 5, &bitmap, 1, 10, 0, 5); // cursor at y=5

    // Row 0 should be unchanged (cursor is at row 5)
    assert!(row0.iter().all(|&b| b == 0xff), "row 0 unchanged");

    // Row 5 should have the cursor pixel
    assert_eq!(row5[0], 0x2e, "row 5 has cursor at col 0");
}
