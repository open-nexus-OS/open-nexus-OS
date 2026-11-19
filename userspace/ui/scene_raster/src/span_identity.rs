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

/// The drop shadow as it was: both distance fields on every pixel of its reach.
fn shadow_ref(
    canvas: &mut RowCanvas<'_>,
    shadow: &nexus_layout_types::BoxShadow,
    corner: i32,
    (x, y, w, h): (i32, i32, i32, i32),
) {
    use super::shapes::sqrt_f32;
    let blur = shadow.blur_radius.0.max(1) as f32;
    let sx = x + shadow.offset_x.0 - shadow.spread.0;
    let sy = y + shadow.offset_y.0 - shadow.spread.0;
    let (sw, sh) = (w + 2 * shadow.spread.0, h + 2 * shadow.spread.0);
    let reach = shadow.blur_radius.0;
    if sw <= 0 || sh <= 0 || canvas.y < sy - reach || canvas.y >= sy + sh + reach {
        return;
    }
    let radius = corner.max(0).min(sw / 2).min(sh / 2) as f32;
    let (cx, cy) = (sx as f32 + sw as f32 / 2.0, sy as f32 + sh as f32 / 2.0);
    let (hw, hh) = (sw as f32 / 2.0 - radius, sh as f32 / 2.0 - radius);
    let (kcx, kcy) = (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
    let (khw, khh) = (w as f32 / 2.0 - radius, h as f32 / 2.0 - radius);
    let py = canvas.y as f32 + 0.5;
    let kdy = (py - kcy).abs() - khh;
    let kdyc = kdy.max(0.0);
    let dy = (py - cy).abs() - hh;
    let dyc = dy.max(0.0);
    for px in (sx - reach).max(0)..(sx + sw + reach).min(canvas.width) {
        let pxf = px as f32 + 0.5;
        let dx = (pxf - cx).abs() - hw;
        let dxc = if dx > 0.0 { dx } else { 0.0 };
        let inside = if dx.max(dy) < 0.0 { dx.max(dy) } else { 0.0 };
        let t = 0.5 - (sqrt_f32(dxc * dxc + dyc * dyc) + inside - radius) / blur;
        if t <= 0.0 {
            continue;
        }
        let mut f = if t >= 1.0 { 1.0 } else { t };
        let kdx = (pxf - kcx).abs() - khw;
        let kdxc = if kdx > 0.0 { kdx } else { 0.0 };
        let kin = if kdx.max(kdy) < 0.0 { kdx.max(kdy) } else { 0.0 };
        let kdist = sqrt_f32(kdxc * kdxc + kdyc * kdyc) + kin - radius;
        if kdist <= 0.0 {
            continue;
        }
        if kdist < 1.0 {
            f *= kdist;
        }
        let a = (shadow.color.a as f32 * f) as u8;
        if a != 0 {
            canvas.blend(px, Rgba8 { a, ..shadow.color });
        }
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
    let sizes = [(1, 1), (2, 3), (5, 20), (17, 9), (40, 105), (101, 64), (333, 40), (72, 60)];
    let radii = [0, 1, 2, 7, 16, 30, 50];
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

/// A small deterministic generator (xorshift): the same cases on every run.
struct Cases(u32);

impl Cases {
    fn next(&mut self, below: u32) -> i32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 % below) as i32
    }
}

/// Capsules, circles and wide radii at random — the shapes whose every row is a corner row,
/// where only the columns an arc crosses are asked (`arc_band`): fill, glass reset, ring, shine
/// and drop shadow against the pixel paths, every row of each, byte for byte. The widest radii
/// lie past the narrowing's limit and take the whole corner.
#[test]
fn an_arc_asks_only_the_columns_it_crosses_and_paints_the_same() {
    use nexus_layout_types::{BoxShadow, CornerRadius, EdgeBorder, FxPx, VisualStyle};
    const WIDE: i32 = 1400;
    fn wide<'a>(buf: &'a mut [u8], y: i32, scroll: (i32, Option<(i32, i32)>)) -> RowCanvas<'a> {
        let mut c = RowCanvas::new(buf, y, WIDE);
        (c.shift_x, c.clip_x) = scroll;
        c
    }
    let base: Vec<u8> = (0..WIDE * 4).map(|i| ((i * 37 + 11) % 251) as u8).collect();
    let mut cases = Cases(0x9e37_79b9);
    let mut rows = 0u32;
    for case in 0..600 {
        let big = case % 20 == 0;
        let (w, h) = if big {
            (1 + cases.next(1300), 1 + cases.next(1300))
        } else {
            (1 + cases.next(200), 1 + cases.next(120))
        };
        let (x, y) = (cases.next(40) - 6, cases.next(9) - 4);
        let radius = match cases.next(4) {
            0 => h / 2,
            1 => w.min(h) / 2 + cases.next(3),
            2 => cases.next(1 + w.max(h) as u32),
            _ => 9_999,
        };
        let col = colours()[cases.next(4) as usize];
        let scroll = if cases.next(3) == 0 {
            (cases.next(9), Some((cases.next(30), 900)))
        } else {
            (0, None)
        };
        let (width, inset) = (1 + cases.next(3), cases.next(2));
        let shadow = BoxShadow {
            offset_x: FxPx::new(cases.next(7) - 3),
            offset_y: FxPx::new(cases.next(9) - 2),
            blur_radius: FxPx::new(cases.next(25)),
            spread: FxPx::new(cases.next(5) - 1),
            color: col,
        };
        let boxed = nexus_layout::LayoutBox {
            visual: VisualStyle {
                corner_radius: CornerRadius::uniform(FxPx::new(radius)),
                ..VisualStyle::default()
            },
            ..nexus_layout::LayoutBox::default()
        };
        let rect = (x, y, w, h);
        let step = if big { 1 + h / 97 } else { 1 };
        for row in (y - 30..y + h + 30).step_by(step as usize) {
            let (mut a, mut b) = (base.clone(), base.clone());
            fill_ref(&mut wide(&mut a, row, scroll), rect, radius, col);
            wide(&mut b, row, scroll).fill_round_rect_row(x, y, w, h, radius, col);
            assert_eq!(a, b, "fill {rect:?} r={radius} {col:?} row={row} {scroll:?}");

            let (mut a, mut b) = (base.clone(), base.clone());
            replace_ref(&mut wide(&mut a, row, scroll), rect, radius, col);
            wide(&mut b, row, scroll).fill_round_rect_row_replace(x, y, w, h, radius, col);
            assert_eq!(a, b, "replace {rect:?} r={radius} {col:?} row={row} {scroll:?}");

            let (mut a, mut b) = (base.clone(), base.clone());
            ring_ref(&mut wide(&mut a, row, scroll), rect, radius, width, col);
            wide(&mut b, row, scroll).stroke_round_rect_row(x, y, w, h, radius, width, col);
            assert_eq!(a, b, "ring {rect:?} r={radius} w={width} {col:?} row={row} {scroll:?}");

            let border = if inset == 0 {
                EdgeBorder::none()
            } else {
                EdgeBorder::all(FxPx::new(inset), col)
            };
            let (mut a, mut b) = (base.clone(), base.clone());
            shine_ref(&mut wide(&mut a, row, scroll), rect, radius, inset, col);
            super::borders::paint_inset_highlight_row(
                &mut wide(&mut b, row, scroll),
                rect,
                radius,
                &border,
                col,
            );
            assert_eq!(a, b, "shine {rect:?} r={radius} inset={inset} row={row} {scroll:?}");

            let (mut a, mut b) = (base.clone(), base.clone());
            shadow_ref(&mut wide(&mut a, row, scroll), &shadow, radius, rect);
            super::paint_shadow_row(&mut wide(&mut b, row, scroll), &shadow, &boxed, x, y, w, h);
            assert_eq!(a, b, "shadow {rect:?} r={radius} {shadow:?} row={row} {scroll:?}");
            rows += 1;
        }
    }
    assert!(rows > 60_000, "the cases ran ({rows} rows)");
}

/// What a painter writes under scissor `clip` and shift `shift`, by its old contract: the full
/// walk without a scissor, kept only where `blend`/`set` let a pixel through — every other
/// column stays what the row held.
fn scissored(
    base: &[u8],
    row: i32,
    (shift, clip): (i32, Option<(i32, i32)>),
    paint: &dyn Fn(&mut RowCanvas<'_>),
) -> Vec<u8> {
    let mut full = base.to_vec();
    paint(&mut canvas(&mut full, row, (shift, None)));
    let mut out = base.to_vec();
    let (c0, c1) = clip.unwrap_or((i32::MIN, i32::MAX));
    for s in c0.max(0)..c1.min(W) {
        let i = s as usize * 4;
        out[i..i + 4].copy_from_slice(&full[i..i + 4]);
    }
    out
}

/// TASK-0367 P3: every painter walks only the columns the scissor lets through — under a narrow
/// window, one at an edge, an empty one, one past the row, and with a shift, each writes exactly
/// what the full walk wrote there and nothing else, byte for byte (fill, glass reset, ring,
/// shine, drop shadow, ellipse, triangle, contour and sprite).
#[test]
fn every_painter_cut_to_the_scissor_paints_what_the_full_walk_painted() {
    use nexus_layout_types::{BoxShadow, CornerRadius, FxPx, PathPoint, PathShape, ShapeKind};
    let clips = [
        None,
        Some((0, W)),
        Some((3, 290)),
        Some((40, 41)),
        Some((100, 160)),
        Some((200, 120)),
        Some((-20, 30)),
        Some((300, 900)),
    ];
    let col = colours()[2];
    let shadow = BoxShadow {
        offset_x: FxPx::new(2),
        offset_y: FxPx::new(4),
        blur_radius: FxPx::new(18),
        spread: FxPx::new(1),
        color: col,
    };
    let sprite: &'static [u8] =
        (0..16 * 9 * 4).map(|i| ((i * 53 + 7) % 256) as u8).collect::<Vec<u8>>().leak();
    let star = PathShape {
        points: [
            (500, 0),
            (620, 380),
            (1000, 380),
            (690, 610),
            (810, 1000),
            (500, 760),
            (190, 1000),
        ]
        .iter()
        .map(|&(x_milli, y_milli)| PathPoint { x_milli, y_milli })
        .collect(),
        closed: true,
    };
    let mut rows = 0u32;
    for &(x, y, w, h, radius) in &[(6, 3, 333, 40, 16), (-9, 0, 120, 64, 60), (150, 5, 40, 40, 20)]
    {
        let boxed = nexus_layout::LayoutBox {
            visual: nexus_layout_types::VisualStyle {
                corner_radius: CornerRadius::uniform(FxPx::new(radius)),
                ..nexus_layout_types::VisualStyle::default()
            },
            ..nexus_layout::LayoutBox::default()
        };
        let mut raster = super::tests::boxed(x, y, w, h, 0, Rgba8::new(0, 0, 0, 1));
        raster.visual.shape = ShapeKind::Raster { w: 16, h: 9, rgba: sprite };
        let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, h as f32);
        let painters: [(&str, &dyn Fn(&mut RowCanvas<'_>)); 9] = [
            ("fill", &|c| c.fill_round_rect_row(x, y, w, h, radius, col)),
            ("replace", &|c| c.fill_round_rect_row_replace(x, y, w, h, radius, col)),
            ("ring", &|c| c.stroke_round_rect_row(x, y, w, h, radius, 2, col)),
            ("shine", &|c| {
                let none = nexus_layout_types::EdgeBorder::none();
                super::borders::paint_inset_highlight_row(c, (x, y, w, h), radius, &none, col)
            }),
            ("shadow", &|c| super::paint_shadow_row(c, &shadow, &boxed, x, y, w, h)),
            ("ellipse", &|c| c.fill_ellipse_row((xf, yf, wf, hf), col)),
            ("triangle", &|c| {
                let pts = [(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)];
                c.fill_polygon_row(3, |i| pts[i], col)
            }),
            ("contour", &|c| c.fill_contour_row(&star, xf, yf, wf, hf, col)),
            ("sprite", &|c| super::paint_box_row(c, &raster)),
        ];
        for (name, paint) in painters {
            for clip in clips {
                for shift in [0, 7, -5] {
                    for row in y - 24..y + h + 24 {
                        let want = scissored(&base_row(), row, (shift, clip), paint);
                        let mut got = base_row();
                        paint(&mut canvas(&mut got, row, (shift, clip)));
                        assert_eq!(
                            got, want,
                            "{name} {x},{y} {w}x{h} row={row} {clip:?} shift={shift}"
                        );
                        rows += 1;
                    }
                }
            }
        }
    }
    assert!(rows > 20_000, "the sweep ran ({rows} rows)");
}

/// The window a painter walks is never wider than what the scissor, the row and the buffer let
/// through — and it is everything they let through.
#[test]
fn visible_is_never_wider_than_the_scissor() {
    let mut buf = base_row();
    for clip in
        [None, Some((3, 290)), Some((40, 41)), Some((200, 120)), Some((-20, 30)), Some((300, 900))]
    {
        for shift in [0, 7, -5] {
            let c = canvas(&mut buf, 0, (shift, clip));
            let (a, b) = c.visible(-1000, 1000);
            let (c0, c1) = clip.unwrap_or((i32::MIN, i32::MAX));
            let lands = |m: i32| {
                let s = m - shift;
                s >= c0.max(0) && s < c1.min(W)
            };
            for m in -1000..1000 {
                assert_eq!((a..b).contains(&m), lands(m), "x={m} {clip:?} shift={shift}");
            }
            let (a, b) = c.visible(50, 60);
            assert!(a >= 50 && b <= 60.max(a), "inside the asked span");
        }
    }
}
