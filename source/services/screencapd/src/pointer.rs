// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pointer in a capture (RFC-0095) — pure, host-tested. windowd writes the sprite
//! it shows (premultiplied BGRA, at most 64×64) behind the frozen frame; when the user keeps
//! the pointer, each row the encoder pulls is blended with the sprite's row at that height —
//! the same `src + dst · (255 − a) / 255` the compositor's software path applies, so the
//! saved pointer matches the one on screen.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs

use crate::plan::Pointer;

/// Blends `pointer`'s sprite into `row`: display row `display_y` of a crop whose first column
/// is display column `crop_x` (`row` is BGRA, `row.len() / 4` pixels). Rows and columns the
/// sprite does not cover, and a sprite shorter than its size claims, are left untouched.
pub fn blend_row(row: &mut [u8], display_y: u32, crop_x: u32, pointer: &Pointer, sprite: &[u8]) {
    let (sw, sh) = (pointer.w as usize, pointer.h as usize);
    if sw == 0 || sh == 0 || sprite.len() < sw * sh * 4 {
        return;
    }
    let top = i64::from(pointer.y) - i64::from(pointer.hot_y);
    let left = i64::from(pointer.x) - i64::from(pointer.hot_x);
    let sy = i64::from(display_y) - top;
    if sy < 0 || sy >= sh as i64 {
        return;
    }
    let row_w = (row.len() / 4) as i64;
    for sx in 0..sw {
        let dx = left + sx as i64 - i64::from(crop_x);
        if dx < 0 || dx >= row_w {
            continue;
        }
        let s = (sy as usize * sw + sx) * 4;
        let a = u32::from(sprite[s + 3]);
        if a == 0 {
            continue;
        }
        let d = dx as usize * 4;
        for c in 0..3 {
            let under = u32::from(row[d + c]) * (255 - a) / 255;
            row[d + c] = (u32::from(sprite[s + c]) + under).min(255) as u8;
        }
    }
}
