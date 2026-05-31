// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cursor blending for windowd compositor: BGRA alpha-composite per scanline.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

pub(crate) fn blend_cursor_row(
    row: &mut [u8],
    ry: u32,
    cb: &[u8],
    cw: u32,
    ch: u32,
    cx: i32,
    cy: i32,
) {
    let cr = ry as i32 - cy;
    if cr < 0 || cr >= ch as i32 {
        return;
    }
    for col in 0..(row.len() / 4) {
        let cc = col as i32 - cx;
        if cc < 0 || cc >= cw as i32 {
            continue;
        }
        let si = ((cr as u32 * cw + cc as u32) * 4) as usize;
        let di = col * 4;
        if si + 4 > cb.len() {
            continue;
        }
        let a = cb[si + 3];
        if a == 0 {
            continue;
        }
        if a == 255 {
            row[di..di + 4].copy_from_slice(&cb[si..si + 4]);
            continue;
        }
        let ia = 255u32.saturating_sub(u32::from(a));
        let a = u32::from(a);
        for ch in 0..3 {
            row[di + ch] =
                ((u32::from(cb[si + ch]) * a + u32::from(row[di + ch]) * ia) / 255) as u8;
        }
        row[di + 3] = 255;
    }
}
