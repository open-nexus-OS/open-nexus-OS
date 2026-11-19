// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The row painter's unit tests (moved out of `lib.rs`, module-size ratchet).

use super::*;
use nexus_layout_types::{CornerRadius, EdgeBorder, FxPx, PathPoint, PathShape, Rect, VisualStyle};

pub(crate) fn boxed(x: i32, y: i32, w: i32, h: i32, radius: i32, color: Rgba8) -> LayoutBox {
    LayoutBox {
        node_id: 1,
        rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
        visual: VisualStyle {
            background: Some(color),
            corner_radius: CornerRadius::uniform(FxPx::new(radius)),
            ..VisualStyle::default()
        },
        ..LayoutBox::default()
    }
}

/// The glass-stacking contract: a glass ROOT resets its rect to the pure
/// tint, NESTED glass blends src-over onto the parent's pixels. The old
/// unconditional replace made every inner glass rect erase its parent —
/// a whole page of stacked panes showed nothing but the window backdrop.
#[test]
fn nested_glass_blends_while_root_glass_replaces() {
    use nexus_layout_types::Material;
    // Root pane: opaque grey. Nested card: half-transparent white.
    let mut pane = boxed(0, 0, 20, 1, 0, Rgba8 { r: 80, g: 80, b: 80, a: 255 });
    pane.visual.material = Some(Material::Pane);
    let mut row = boxed(0, 0, 10, 1, 0, Rgba8 { r: 255, g: 255, b: 255, a: 128 });
    row.node_id = 2;
    row.visual.material = Some(Material::Card);
    row.glass_nested = true;

    let mut buf = [0u8; 20 * 4];
    let mut canvas = RowCanvas::new(&mut buf, 0, 20);
    paint_row(&mut canvas, &[pane.clone(), row.clone()]);
    // Nested: the row's white BLENDS with the pane grey — the pixel keeps
    // a parent contribution (grey shifted toward white, alpha stays up).
    let blended = buf[2];
    assert!(
        blended > 80 && buf[3] == 255,
        "nested glass must blend onto the pane: rgba={:?}",
        &buf[..4]
    );

    // A card never replaces, nested or not: it is a translucent layer, never a blur root
    // (RFC-0116), so unnested it still blends onto the pane.
    row.glass_nested = false;
    let mut buf2 = [0u8; 20 * 4];
    let mut canvas2 = RowCanvas::new(&mut buf2, 0, 20);
    paint_row(&mut canvas2, &[pane.clone(), row.clone()]);
    assert_eq!(buf2[3], 255, "an unnested card still blends: rgba={:?}", &buf2[..4]);

    // A blur ROOT replaces: alpha drops to the tint's own 128 — the compositor's backdrop
    // shows through it, so nothing the app drew under it may bake in.
    row.visual.material = Some(Material::Thin);
    let mut buf3 = [0u8; 20 * 4];
    let mut canvas3 = RowCanvas::new(&mut buf3, 0, 20);
    paint_row(&mut canvas3, &[pane, row]);
    assert_eq!(buf3[3], 128, "a blur root replaces (alpha = tint): rgba={:?}", &buf3[..4]);
}

/// A compositor region's shadow is windowd's: a blur root (and a fully opaque pane root)
/// paints no shadow of its own — painting it doubled the compositor's and cut it flat at the
/// box's top and bottom rows. A card is no region: its shadow stays the painter's.
#[test]
fn a_region_roots_shadow_is_the_compositors() {
    use nexus_layout_types::{BoxShadow, Material};
    let shadowed = |material: Material, opacity: Option<u32>| {
        let mut b = boxed(10, 0, 10, 1, 0, Rgba8 { r: 255, g: 255, b: 255, a: 255 });
        b.visual.material = Some(material);
        b.visual.opacity = opacity.map(nexus_layout_types::Fraction);
        b.visual.shadow = Some(BoxShadow {
            offset_x: FxPx::new(0),
            offset_y: FxPx::new(0),
            blur_radius: FxPx::new(8),
            spread: FxPx::new(0),
            color: Rgba8 { r: 0, g: 0, b: 0, a: 200 },
        });
        let mut buf = [0u8; 30 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 0, 30);
        paint_row(&mut canvas, &[b]);
        buf[8 * 4 + 3] // a pixel left of the box: shadow or nothing
    };
    assert_eq!(shadowed(Material::Panel, None), 0, "a blur root's shadow is windowd's");
    assert_eq!(shadowed(Material::Overlay, None), 0, "a frosted overlay's shadow is windowd's");
    assert_eq!(shadowed(Material::Pane, None), 0, "an opaque root's shadow is windowd's");
    assert!(shadowed(Material::Card, None) > 0, "a card keeps its painted shadow");
    assert!(shadowed(Material::Pane, Some(128)) > 0, "a faded pane is no region");
}

/// A faded box (its effective opacity below 255) paints its fill at that fraction: a white
/// box at half opacity over nothing leaves half-alpha white, the same box opaque full white.
/// An overlay is frosted as a root — its rect reset to the translucent tint, the compositor's
/// blur behind it — and opaque where it is nested (a floating action bar on a pane): its glass
/// at full strength, nothing of the pane showing through.
#[test]
fn a_root_overlay_is_its_tint_and_a_nested_one_is_opaque() {
    let tint = Rgba8 { r: 252, g: 252, b: 254, a: 209 };
    let mut pane = boxed(0, 0, 8, 1, 0, Rgba8 { r: 40, g: 40, b: 40, a: 255 });
    pane.visual.material = Some(Material::Pane);
    let mut overlay = boxed(0, 0, 4, 1, 0, tint);
    overlay.node_id = 2;
    overlay.visual.material = Some(Material::Overlay);
    let paint = |nested: bool| {
        let mut o = overlay.clone();
        o.glass_nested = nested;
        let mut buf = [0u8; 8 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 0, 8);
        paint_row(&mut canvas, &[pane.clone(), o]);
        [buf[0], buf[3]]
    };
    assert_eq!(paint(false)[1], 209, "a root overlay leaves its tint for the compositor");
    assert_eq!(paint(true), [254, 255], "a nested overlay covers the pane, opaque");
}

#[test]
fn a_faded_box_paints_its_fill_at_its_opacity() {
    let paint = |opacity: u8| {
        let mut b = boxed(0, 0, 4, 1, 0, Rgba8 { r: 255, g: 255, b: 255, a: 255 });
        b.opacity = opacity;
        let mut buf = [0u8; 4 * 4];
        let mut canvas = RowCanvas::new(&mut buf, 0, 4);
        paint_row(&mut canvas, &[b]);
        [buf[4], buf[7]]
    };
    assert_eq!(paint(255), [255, 255]);
    let [c, a] = paint(128);
    assert!((127..=129).contains(&c) && (127..=129).contains(&a), "half: c={c} a={a}");
}

#[test]
fn a_shadow_reaches_past_its_box_by_its_reach_and_no_further() {
    let mut b = boxed(10, 10, 20, 10, 4, Rgba8 { r: 255, g: 255, b: 255, a: 255 });
    b.visual.shadow = Some(nexus_layout_types::BoxShadow {
        color: Rgba8 { r: 0, g: 0, b: 0, a: 200 },
        offset_x: FxPx::new(0),
        offset_y: FxPx::new(2),
        blur_radius: FxPx::new(4),
        spread: FxPx::new(0),
    });
    let (l, t, r, bo) = shadow_reach(&b);
    assert_eq!((l, t, r, bo), (4, 2, 4, 6), "blur 4 around, moved 2 down");
    // A row inside the reach below the box carries shadow; one past the reach carries none.
    let row_alpha = |y: i32| {
        let mut row = [0u8; 40 * 4];
        let mut canvas = RowCanvas::new(&mut row, y, 40);
        paint_box_row(&mut canvas, &b);
        row.chunks(4).map(|p| p[3]).max().unwrap_or(0)
    };
    assert!(row_alpha(10 + 10 + 1) > 0, "the shadow shows below the box");
    assert_eq!(row_alpha(10 + 10 + bo), 0, "nothing past the reach");
    // A compositor region paints no shadow, so it reaches nowhere.
    b.visual.material = Some(nexus_layout_types::Material::Panel);
    assert_eq!(shadow_reach(&b), (0, 0, 0, 0));
}

#[test]
fn rounded_corners_clip_the_corner_pixels() {
    let b = boxed(0, 0, 20, 20, 8, Rgba8 { r: 255, g: 0, b: 0, a: 255 });
    let mut row = [0u8; 20 * 4];
    let mut canvas = RowCanvas::new(&mut row, 0, 20);
    paint_box_row(&mut canvas, &b);
    assert_eq!(row[0], 0, "corner pixel stays empty");
    assert_ne!(row[10 * 4 + 2], 0, "centre pixel painted red");
}

#[test]
fn circle_row_is_narrower_near_the_pole() {
    let mut b = boxed(0, 0, 32, 32, 0, Rgba8 { r: 0, g: 255, b: 0, a: 255 });
    b.visual.shape = ShapeKind::Circle;
    let painted = |y: i32| {
        let mut row = [0u8; 32 * 4];
        let mut canvas = RowCanvas::new(&mut row, y, 32);
        paint_box_row(&mut canvas, &b);
        row.chunks_exact(4).filter(|px| px[1] != 0).count()
    };
    assert!(painted(2) < painted(16), "pole rows narrower than the equator");
}

#[test]
fn uniform_border_ring_follows_the_corner_radius() {
    use nexus_layout_types::{EdgeBorder, FxPx};
    let mut b = boxed(0, 0, 24, 24, 8, Rgba8 { r: 10, g: 10, b: 10, a: 255 });
    b.visual.border = EdgeBorder::all(FxPx::new(2), Rgba8 { r: 0, g: 0, b: 255, a: 255 });
    // Top row (y=0): the ring must NOT paint the extreme corner pixel
    // (a square frame would) but MUST paint near the rounded arc.
    let mut row = [0u8; 24 * 4];
    let mut canvas = RowCanvas::new(&mut row, 0, 24);
    paint_box_row(&mut canvas, &b);
    assert_eq!(row[0], 0, "corner pixel outside the rounded ring stays empty");
    assert_eq!(row[12 * 4], 255, "top-centre pixel is border blue");
    // Mid row: ring = left/right edges only; the centre is fill, not border.
    let mut mid = [0u8; 24 * 4];
    let mut canvas = RowCanvas::new(&mut mid, 12, 24);
    paint_box_row(&mut canvas, &b);
    assert_eq!(mid[0], 255, "left edge is border blue");
    assert_ne!(mid[12 * 4], 255, "centre is the fill, not the border");
}

#[test]
fn src_over_blends_translucent_glass() {
    let b = boxed(0, 0, 4, 4, 0, Rgba8 { r: 255, g: 255, b: 255, a: 128 });
    let mut row = [0x40u8; 4 * 4];
    for px in row.chunks_exact_mut(4) {
        px[3] = 0xff;
    }
    let mut canvas = RowCanvas::new(&mut row, 1, 4);
    paint_box_row(&mut canvas, &b);
    assert!(row[0] > 0x40 && row[0] < 0xff, "50% white over grey = mid blend");
}

// --- rotation, base opacity, the rounded clip, the split axis (TASK-0354 P10/P13) ---

const INK: Rgba8 = Rgba8 { r: 255, g: 255, b: 255, a: 255 };

fn pp(x: u16, y: u16) -> PathPoint {
    PathPoint::new(x, y)
}

/// A chevron pointing down, stroked — the design's disclosure glyph.
fn chevron() -> ShapeKind {
    ShapeKind::Stroke {
        paths: vec![PathShape::line(&[pp(250, 375), pp(500, 625), pp(750, 375)])],
        width_milli: 83,
    }
}

/// A filled arrow pointing up (no symmetry a half turn would hide).
fn arrow() -> PathShape {
    PathShape::polygon(&[
        pp(500, 100),
        pp(900, 700),
        pp(600, 700),
        pp(600, 900),
        pp(400, 900),
        pp(400, 700),
        pp(100, 700),
    ])
}

fn glyph(shape: ShapeKind, (x, y, w, h): (i32, i32, i32, i32), deg: i16) -> LayoutBox {
    let mut b = boxed(x, y, w, h, 0, INK);
    b.visual.shape = shape;
    b.visual.rotate_deg = deg;
    b
}

/// `boxes` painted through the row loop with `anims` over a `w` x `h` surface of `fill` bytes.
fn surface(boxes: &[LayoutBox], anims: &[NodeAnim], w: i32, h: i32, fill: u8) -> Vec<u8> {
    let pick: Vec<u32> = (0..boxes.len() as u32).collect();
    let mut out = vec![fill; (w * h * 4) as usize];
    for (y, row) in out.chunks_exact_mut(w as usize * 4).enumerate() {
        let mut canvas = RowCanvas::new(row, y as i32, w);
        paint_row_picked_animated(&mut canvas, boxes, &pick, &[], anims);
    }
    out
}

/// The blue channel of each row of `rect` in a `w`-wide surface, as hex — a readable golden.
fn hex_rows(buf: &[u8], w: i32, (x, y, bw, bh): (i32, i32, i32, i32)) -> Vec<String> {
    (y..y + bh)
        .map(|row| {
            (x..x + bw).map(|col| format!("{:02x}", buf[((row * w + col) * 4) as usize])).collect()
        })
        .collect()
}

/// No rotation is the paint of today: each shape through the box painter equals its primitive
/// called as `paint_box_row_at` always called it — at rest, after a whole turn, and under a
/// motion that cancels the rest rotation.
#[test]
fn a_rotation_of_0_is_byte_identical() {
    let (w, h) = (28, 20);
    let rect = (3, 2, 20, 14);
    let (xf, yf, wf, hf) = (3.0, 2.0, 20.0, 14.0);
    let shapes = [
        chevron(),
        ShapeKind::Path(arrow()),
        ShapeKind::Vector(vec![arrow(), PathShape::polygon(&[pp(0, 0), pp(300, 0), pp(0, 300)])]),
        ShapeKind::TriangleUp,
        ShapeKind::TriangleDown,
        ShapeKind::Circle,
        ShapeKind::Rect,
    ];
    for shape in shapes {
        let mut today = vec![9u8; (w * h * 4) as usize];
        for (y, row) in today.chunks_exact_mut(w as usize * 4).enumerate() {
            let mut c = RowCanvas::new(row, y as i32, w);
            if c.y < 2 || c.y >= 16 {
                continue;
            }
            match &shape {
                ShapeKind::Rect => c.fill_round_rect_row(3, 2, 20, 14, 0, INK),
                ShapeKind::Circle => c.fill_ellipse_row((xf, yf, wf, hf), INK),
                ShapeKind::TriangleUp => {
                    let pts = [(xf + wf / 2.0, yf), (xf + wf, yf + hf), (xf, yf + hf)];
                    c.fill_polygon_row(3, |i| pts[i], INK);
                }
                ShapeKind::TriangleDown => {
                    let pts = [(xf, yf), (xf + wf, yf), (xf + wf / 2.0, yf + hf)];
                    c.fill_polygon_row(3, |i| pts[i], INK);
                }
                ShapeKind::Path(ps) => c.fill_contour_row(ps, xf, yf, wf, hf, INK),
                ShapeKind::Vector(contours) => {
                    for ps in contours {
                        c.fill_contour_row(ps, xf, yf, wf, hf, INK);
                    }
                }
                ShapeKind::Stroke { paths, width_milli } => {
                    c.stroke_paths_row(paths, (xf, yf, wf, hf), *width_milli, INK)
                }
                ShapeKind::Raster { .. } => {}
            }
        }
        assert!(today.iter().any(|v| *v != 9), "{shape:?} paints");
        let rest = surface(&[glyph(shape.clone(), rect, 0)], &[], w, h, 9);
        assert_eq!(rest, today, "{shape:?} at rest");
        let whole = surface(&[glyph(shape.clone(), rect, 360)], &[], w, h, 9);
        assert_eq!(whole, today, "{shape:?} after a whole turn");
        let back = NodeAnim { rot_deg: -90, ..NodeAnim::identity(1) };
        assert!(!back.is_identity());
        let cancelled = surface(&[glyph(shape.clone(), rect, 90)], &[back], w, h, 9);
        assert_eq!(cancelled, today, "{shape:?} with the motion cancelling the rest");
    }
}

/// A half turn is the glyph flipped on both axes — at rest and as a motion's rotation.
#[test]
fn a_chevron_at_180_is_its_flipped_self() {
    let (w, h) = (32, 32);
    let rect = (4, 4, 24, 24);
    for shape in [chevron(), ShapeKind::Path(arrow()), ShapeKind::TriangleUp] {
        let up = surface(&[glyph(shape.clone(), rect, 0)], &[], w, h, 0);
        let down = surface(&[glyph(shape.clone(), rect, 180)], &[], w, h, 0);
        assert_ne!(up, down, "{shape:?} turned");
        // The box is centred on the surface: its flip is the surface's.
        for (px, flipped) in down.chunks_exact(4).zip(up.chunks_exact(4).rev()) {
            for (a, b) in px.iter().zip(flipped) {
                assert!(a.abs_diff(*b) <= 2, "{shape:?}: {a} against {b}");
            }
        }
        let motion = NodeAnim { rot_deg: 180, ..NodeAnim::identity(1) };
        let moved = surface(&[glyph(shape.clone(), rect, 0)], &[motion], w, h, 0);
        assert_eq!(moved, down, "{shape:?}: a motion's turn is the rest's");
    }
    // A triangle pointing up, half turned, is the one pointing down.
    let turned = surface(&[glyph(ShapeKind::TriangleUp, rect, 180)], &[], w, h, 0);
    let down = surface(&[glyph(ShapeKind::TriangleDown, rect, 0)], &[], w, h, 0);
    for (a, b) in turned.iter().zip(&down) {
        assert!(a.abs_diff(*b) <= 2, "{a} against {b}");
    }
}

/// The rotation at a quarter and an eighth turn: clockwise, about the box centre — the samples
/// say where the ink is, the goldens pin every byte.
#[test]
fn a_rotation_samples_at_45_and_90() {
    let (w, rect) = (12, (0, 0, 12, 12));
    let at = |buf: &[u8], x: i32, y: i32| buf[((y * w + x) * 4) as usize];
    // A triangle pointing up, a quarter turn clockwise: it points right.
    let quarter = surface(&[glyph(ShapeKind::TriangleUp, rect, 90)], &[], w, 12, 0);
    assert_eq!((at(&quarter, 1, 6), at(&quarter, 1, 1)), (255, 255), "its base is the left edge");
    assert_eq!((at(&quarter, 10, 1), at(&quarter, 10, 10)), (0, 0), "its tip is at the right");
    assert!(at(&quarter, 10, 5) > 0 && at(&quarter, 10, 6) > 0, "the tip, mid right");
    // A level line through the centre, an eighth turn clockwise: the falling diagonal.
    let line = ShapeKind::Stroke {
        paths: vec![PathShape::line(&[pp(100, 500), pp(900, 500)])],
        width_milli: 170,
    };
    let eighth = surface(&[glyph(line, rect, 45)], &[], w, 12, 0);
    for k in 3..9 {
        assert_eq!(at(&eighth, k, k), 255, "on the falling diagonal at {k}");
    }
    assert_eq!((at(&eighth, 2, 9), at(&eighth, 9, 2)), (0, 0), "not on the rising one");
    assert_eq!(
        hex_rows(&quarter, w, rect),
        GOLDEN_TRIANGLE_90,
        "{:#?}",
        hex_rows(&quarter, w, rect)
    );
    assert_eq!(hex_rows(&eighth, w, rect), GOLDEN_LINE_45, "{:#?}", hex_rows(&eighth, w, rect));
}

const GOLDEN_TRIANGLE_90: [&str; 12] = [
    "bf4000000000000000000000",
    "ffffbf400000000000000000",
    "ffffffffbf40000000000000",
    "ffffffffffffbf4000000000",
    "ffffffffffffffffbf400000",
    "ffffffffffffffffffffbf40",
    "ffffffffffffffffffffbf40",
    "ffffffffffffffffbf400000",
    "ffffffffffffbf4000000000",
    "ffffffffbf40000000000000",
    "ffffbf400000000000000000",
    "bf4000000000000000000000",
];
const GOLDEN_LINE_45: [&str; 12] = [
    "000000000000000000000000",
    "000068190000000000000000",
    "0068ffcf1b00000000000000",
    "0019cfffcf1b000000000000",
    "00001bcfffcf1b0000000000",
    "0000001bcfffcf1b00000000",
    "000000001bcfffcf1b000000",
    "00000000001bcfffcf1b0000",
    "0000000000001bcfffcf1900",
    "000000000000001bcfff6800",
    "000000000000000019680000",
    "000000000000000000000000",
];

/// A turned glyph is cut to its box: at every angle, at rest and under a motion, on a shifted
/// and scissored canvas — no byte outside the box's rows and columns changes.
#[test]
fn rotated_ink_stays_inside_its_box() {
    let (w, h) = (50, 28);
    let (x, y, bw, bh) = (10, 8, 30, 12);
    let bar = ShapeKind::Stroke {
        paths: vec![PathShape::line(&[pp(0, 500), pp(1000, 500)])],
        width_milli: 300,
    };
    let shapes = [bar, ShapeKind::Path(arrow()), ShapeKind::TriangleUp, ShapeKind::TriangleDown];
    let outside = |buf: &[u8], x0: i32, what: &str| {
        for (i, px) in buf.chunks_exact(4).enumerate() {
            let (col, row) = (i as i32 % w, i as i32 / w);
            let inside = col >= x0 && col < x0 + bw && row >= y && row < y + bh;
            assert!(inside || px == [0, 0, 0, 0], "{what}: ink at ({col}, {row})");
        }
    };
    let mut inked = 0;
    for shape in shapes {
        for deg in (15..360).step_by(15) {
            let rest = surface(&[glyph(shape.clone(), (x, y, bw, bh), deg)], &[], w, h, 0);
            outside(&rest, x, "at rest");
            inked += usize::from(rest.iter().any(|v| *v != 0));
            let motion = NodeAnim { rot_deg: deg, ..NodeAnim::identity(1) };
            let moved = surface(&[glyph(shape.clone(), (x, y, bw, bh), 0)], &[motion], w, h, 0);
            assert_eq!(moved, rest, "a motion's turn is cut like the rest's");
            // The same box on a canvas shifted by 4 columns: it shows 4 columns to the left.
            let mut shifted = vec![0u8; (w * h * 4) as usize];
            for (row_y, row) in shifted.chunks_exact_mut(w as usize * 4).enumerate() {
                let mut canvas = RowCanvas::new(row, row_y as i32, w);
                canvas.shift_x = 4;
                paint_row(&mut canvas, &[glyph(shape.clone(), (x, y, bw, bh), deg)]);
            }
            outside(&shifted, x - 4, "shifted");
        }
    }
    assert_eq!(inked, 4 * 23, "every turn paints");
}

/// A box whose scene opacity is 0 paints nothing at rest; a motion's base opacity paints it as
/// the box it would be at that opacity — fill, border and all.
#[test]
fn a_base_opacity_paints_a_box_whose_rest_is_hidden() {
    let (w, h) = (12, 6);
    let mut hidden = boxed(2, 1, 8, 4, 2, Rgba8 { r: 240, g: 120, b: 30, a: 200 });
    hidden.visual.border = EdgeBorder::all(FxPx::new(1), Rgba8 { r: 10, g: 20, b: 200, a: 255 });
    hidden.opacity = 0;
    assert!(surface(&[hidden.clone()], &[], w, h, 37).iter().all(|v| *v == 37), "hidden at rest");
    for value in [255u8, 128, 1] {
        let mut shown = hidden.clone();
        shown.opacity = value;
        let exit = NodeAnim { base_opacity: Some(value), ..NodeAnim::identity(1) };
        assert!(!exit.is_identity());
        let painted = surface(&[hidden.clone()], &[exit], w, h, 37);
        assert_eq!(painted, surface(&[shown], &[], w, h, 37), "as if its opacity were {value}");
        assert!(value == 1 || painted.iter().any(|v| *v != 37), "it paints at {value}");
    }
    // The other way: a visible box painted as if hidden.
    let mut shown = hidden.clone();
    shown.opacity = 255;
    let gone = NodeAnim { base_opacity: Some(0), ..NodeAnim::identity(1) };
    assert!(surface(&[shown], &[gone], w, h, 37).iter().all(|v| *v == 37));
}

/// A capsule squeezed along one axis stays a capsule: a split axis keeps the authored radius
/// (the fill clamps it), where a uniform scale scales it with the box.
#[test]
fn a_split_axis_keeps_its_capsule_radius() {
    let split = NodeAnim { scale_pct: 50, scale_y_pct: Some(100), ..NodeAnim::identity(1) };
    assert_eq!(split.radius_pct(), 100);
    let squeezed = NodeAnim { scale_pct: 150, scale_y_pct: Some(40), ..NodeAnim::identity(1) };
    assert_eq!(squeezed.radius_pct(), 100);
    assert_eq!(NodeAnim { scale_pct: 50, ..NodeAnim::identity(1) }.radius_pct(), 50);
    // A 40x20 capsule at half its width is the 20x20 circle, not a square with 5 px corners.
    let (w, h) = (60, 20);
    let thumb = boxed(10, 0, 40, 20, 10, INK);
    let painted = surface(&[thumb], &[split], w, h, 0);
    assert_eq!(painted, surface(&[boxed(20, 0, 20, 20, 10, INK)], &[], w, h, 0));
    assert_ne!(painted, surface(&[boxed(20, 0, 20, 20, 5, INK)], &[], w, h, 0));
}

const TRACK: RoundClip = RoundClip { x: 10, y: 4, w: 60, h: 12, radius: 6 };
const FILL: Rgba8 = Rgba8 { r: 240, g: 120, b: 30, a: 200 };

/// A part under its track's rounded clip ends exactly as the track's own capsule fill does: the
/// same bytes at the rounded end, at both ends, for a part taller than the track too.
#[test]
fn a_clipped_child_ends_byte_equal_to_its_capsule() {
    let (w, h) = (80, 20);
    let row_len = w as usize * 4;
    let capsule = surface(&[boxed(10, 4, 60, 12, 6, FILL)], &[], w, h, 37);
    for clip in [TRACK, RoundClip { radius: 99, ..TRACK }] {
        let under = NodeAnim { clip: Some(clip), ..NodeAnim::identity(1) };
        assert!(!under.is_identity());
        // A part over the left end, hanging out of the track to the left.
        let left = surface(&[boxed(0, 4, 30, 12, 0, FILL)], &[under], w, h, 37);
        // A part over the right end, taller than the track and hanging out to the right.
        let right = surface(&[boxed(50, 2, 30, 16, 0, FILL)], &[under], w, h, 37);
        for (y, ((l, r), cap)) in left
            .chunks_exact(row_len)
            .zip(right.chunks_exact(row_len))
            .zip(capsule.chunks_exact(row_len))
            .enumerate()
        {
            assert_eq!(l[..30 * 4], cap[..30 * 4], "the left end, row {y}");
            assert!(l[30 * 4..].iter().all(|v| *v == 37), "nothing past the part, row {y}");
            assert_eq!(r[50 * 4..], cap[50 * 4..], "the right end, row {y}");
            assert!(r[..50 * 4].iter().all(|v| *v == 37), "nothing before the part, row {y}");
        }
    }
    // The end is soft: the capsule's first row has partly covered pixels the part shares.
    let first = &capsule[4 * row_len..5 * row_len];
    assert!(first.chunks_exact(4).any(|px| px[0] != 37 && px[0] != capsule[10 * row_len + 160]));
}

/// Outside its clip a part paints nothing: beside it, above it, in the cut-off corner — a fill,
/// a glass reset and a stroked glyph alike.
#[test]
fn a_part_outside_its_clip_paints_nothing() {
    let (w, h) = (80, 20);
    let under = NodeAnim { clip: Some(TRACK), ..NodeAnim::identity(1) };
    let untouched = |buf: &[u8]| buf.iter().all(|v| *v == 37);
    assert!(untouched(&surface(&[boxed(0, 4, 9, 12, 0, FILL)], &[under], w, h, 37)), "beside");
    assert!(untouched(&surface(&[boxed(20, 0, 10, 4, 0, FILL)], &[under], w, h, 37)), "above");
    assert!(untouched(&surface(&[boxed(20, 16, 10, 4, 0, FILL)], &[under], w, h, 37)), "below");
    assert!(untouched(&surface(&[boxed(10, 4, 1, 1, 0, FILL)], &[under], w, h, 37)), "the corner");
    let empty = NodeAnim { clip: Some(RoundClip { w: 0, ..TRACK }), ..NodeAnim::identity(1) };
    assert!(untouched(&surface(&[boxed(0, 0, 80, 20, 0, FILL)], &[empty], w, h, 37)), "no clip");
    // Parts far larger than the clip: every changed pixel lies in the clip's rect, the cut-off
    // corner pixel stays, the middle is painted.
    let mut glass = boxed(0, 0, 80, 20, 0, FILL);
    glass.visual.material = Some(Material::Pane);
    let bar = ShapeKind::Stroke {
        paths: vec![PathShape::line(&[pp(0, 500), pp(1000, 500)])],
        width_milli: 900,
    };
    for part in [boxed(0, 0, 80, 20, 0, FILL), glass, glyph(bar, (0, 0, 80, 20), 0)] {
        let painted = surface(&[part.clone()], &[under], w, h, 37);
        for (i, px) in painted.chunks_exact(4).enumerate() {
            let (col, row) = (i as i32 % w, i as i32 / w);
            let inside = (10..70).contains(&col) && (4..16).contains(&row);
            assert!(inside || px == [37; 4], "{:?}: ink at ({col}, {row})", part.visual.shape);
        }
        let at = |col: usize, row: usize| &painted[(row * 80 + col) * 4..][..4];
        assert_eq!(at(10, 4), [37; 4], "the cut-off corner");
        assert_eq!(at(69, 15), [37; 4], "the cut-off corner, far end");
        assert_ne!(at(40, 10), [37; 4], "the middle");
        // The glass reset and the fill end as the track's own fill of that kind ends.
        let mut track = boxed(10, 4, 60, 12, 6, FILL);
        track.visual.material = part.visual.material;
        if part.visual.shape == ShapeKind::Rect {
            assert_eq!(painted, surface(&[track], &[], w, h, 37), "{:?}", part.visual.material);
        }
    }
}
