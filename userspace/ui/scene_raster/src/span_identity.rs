// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The span paths paint exactly what the pixel paths painted (TASK-0337): a box's interior laid
//! as one span, a border ring computed only where it is not zero, the straight top and bottom
//! edges of a ring or a shine as one coverage a row — each checked byte for byte against the
//! pixel-at-a-time painting it replaced, over sizes, radii, ring widths, alphas, rows, a scroll
//! shift and a scissor.

use super::shapes::round_rect_coverage;
use super::RowCanvas;
use nexus_layout_types::Rgba8;

const W: i32 = 360;

/// A row that is not uniform, so a blend that differs shows.
fn base_row() -> Vec<u8> {
    (0..W * 4).map(|i| ((i * 37 + 11) % 251) as u8).collect()
}

fn canvas<'a>(buf: &'a mut [u8], y: i32, scroll: (i32, Option<(i32, i32)>)) -> RowCanvas<'a> {
    let mut c = RowCanvas::new(buf, y, W);
    (c.shift_x, c.clip_x) = scroll;
    c
}

/// The fill as it was: every pixel through `blend` / `blend_covered`.
fn fill_ref(c: &mut RowCanvas<'_>, (x, y, w, h): (i32, i32, i32, i32), radius: i32, col: Rgba8) {
    if w <= 0 || h <= 0 || c.y < y || c.y >= y + h {
        return;
    }
    let r = radius.max(0).min(w / 2).min(h / 2);
    let in_corner_rows = c.y < y + r || c.y >= y + h - r;
    let rect = (x as f32, y as f32, w as f32, h as f32);
    let py = c.y as f32 + 0.5;
    for xx in x..x + w {
        if !in_corner_rows || (xx >= x + r && xx < x + w - r) {
            c.blend(xx, col);
        } else {
            c.blend_covered(xx, col, round_rect_coverage(xx as f32 + 0.5, py, rect, r as f32));
        }
    }
}

/// The glass reset as it was: every pixel through `set_covered`.
fn replace_ref(c: &mut RowCanvas<'_>, (x, y, w, h): (i32, i32, i32, i32), radius: i32, col: Rgba8) {
    if w <= 0 || h <= 0 || c.y < y || c.y >= y + h {
        return;
    }
    let r = radius.max(0).min(w / 2).min(h / 2);
    let in_corner_rows = c.y < y + r || c.y >= y + h - r;
    let rect = (x as f32, y as f32, w as f32, h as f32);
    let py = c.y as f32 + 0.5;
    for xx in x..x + w {
        let cov = if !in_corner_rows || (xx >= x + r && xx < x + w - r) {
            1.0
        } else {
            round_rect_coverage(xx as f32 + 0.5, py, rect, r as f32)
        };
        c.set_covered(xx, col, cov);
    }
}

/// The ring as it was: two distance fields on every pixel of the row.
fn ring_ref(
    c: &mut RowCanvas<'_>,
    (x, y, w, h): (i32, i32, i32, i32),
    radius: i32,
    width: i32,
    col: Rgba8,
) {
    if w <= 0 || h <= 0 || c.y < y || c.y >= y + h {
        return;
    }
    let outer = (x as f32, y as f32, w as f32, h as f32);
    let inner =
        ((x + width) as f32, (y + width) as f32, (w - 2 * width) as f32, (h - 2 * width) as f32);
    let (r_out, r_in) = (radius.max(0) as f32, (radius - width).max(0) as f32);
    let py = c.y as f32 + 0.5;
    for xx in x..x + w {
        let px = xx as f32 + 0.5;
        let cov =
            round_rect_coverage(px, py, outer, r_out) - round_rect_coverage(px, py, inner, r_in);
        c.blend_covered(xx, col, cov);
    }
}

/// The shine as it was (`paint_inset_highlight_row` before its span).
fn shine_ref(
    c: &mut RowCanvas<'_>,
    (x, y, w, h): (i32, i32, i32, i32),
    radius: i32,
    inset: i32,
    col: Rgba8,
) {
    let (ix, iy, iw, ih) = (x + inset, y + inset, w - 2 * inset, h - 2 * inset);
    if iw <= 0 || ih <= 0 {
        return;
    }
    let r = (radius - inset).max(0).min(iw / 2).min(ih / 2);
    if c.y < iy || c.y > iy + r {
        return;
    }
    let inner = (ix as f32, iy as f32, iw as f32, ih as f32);
    let py = c.y as f32 + 0.5;
    for xx in ix..ix + iw {
        let px = xx as f32 + 0.5;
        let band = round_rect_coverage(px, py, inner, r as f32)
            - round_rect_coverage(px, py - 1.0, inner, r as f32);
        c.blend_covered(xx, col, band);
    }
}

fn colours() -> [Rgba8; 4] {
    [
        Rgba8::new(10, 200, 30, 0),
        Rgba8::new(250, 1, 99, 1),
        Rgba8::new(72, 74, 84, 122),
        Rgba8::new(52, 54, 62, 255),
    ]
}

fn scrolls() -> [(i32, Option<(i32, i32)>); 2] {
    [(0, None), (7, Some((3, 290)))]
}

#[test]
fn every_span_path_paints_what_the_pixel_path_painted() {
    let sizes = [(1, 1), (2, 3), (5, 20), (17, 9), (40, 105), (101, 64), (333, 40)];
    let radii = [0, 1, 2, 7, 16, 50];
    let mut checked = 0u32;
    for &(w, h) in &sizes {
        let rect = (6, 3, w, h);
        for &radius in &radii {
            for col in colours() {
                for scroll in scrolls() {
                    for row in rect.1 - 1..rect.1 + h + 1 {
                        let (mut a, mut b) = (base_row(), base_row());
                        fill_ref(&mut canvas(&mut a, row, scroll), rect, radius, col);
                        let (x, y, w, h) = rect;
                        canvas(&mut b, row, scroll).fill_round_rect_row(x, y, w, h, radius, col);
                        assert_eq!(a, b, "fill {rect:?} r={radius} {col:?} row={row} {scroll:?}");

                        let (mut a, mut b) = (base_row(), base_row());
                        replace_ref(&mut canvas(&mut a, row, scroll), rect, radius, col);
                        canvas(&mut b, row, scroll)
                            .fill_round_rect_row_replace(x, y, w, h, radius, col);
                        assert_eq!(a, b, "replace {rect:?} r={radius} {col:?} row={row}");

                        for width in [1, 2, 3] {
                            let (mut a, mut b) = (base_row(), base_row());
                            ring_ref(&mut canvas(&mut a, row, scroll), rect, radius, width, col);
                            canvas(&mut b, row, scroll)
                                .stroke_round_rect_row(x, y, w, h, radius, width, col);
                            assert_eq!(
                                a, b,
                                "ring {rect:?} r={radius} w={width} {col:?} row={row}"
                            );
                        }
                        for inset in [0, 1] {
                            let border = nexus_layout_types::EdgeBorder::all(
                                nexus_layout_types::FxPx::new(inset),
                                col,
                            );
                            let border = if inset == 0 {
                                nexus_layout_types::EdgeBorder::none()
                            } else {
                                border
                            };
                            let (mut a, mut b) = (base_row(), base_row());
                            shine_ref(&mut canvas(&mut a, row, scroll), rect, radius, inset, col);
                            super::borders::paint_inset_highlight_row(
                                &mut canvas(&mut b, row, scroll),
                                rect,
                                radius,
                                &border,
                                col,
                            );
                            assert_eq!(a, b, "shine {rect:?} r={radius} inset={inset} row={row}");
                        }
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 4_000, "the grid ran ({checked} rows)");
}
