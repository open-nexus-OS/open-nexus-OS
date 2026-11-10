// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The row painter's unit tests (moved out of `lib.rs`, module-size ratchet).

use super::*;
use nexus_layout_types::{CornerRadius, FxPx, Rect, VisualStyle};

fn boxed(x: i32, y: i32, w: i32, h: i32, radius: i32, color: Rgba8) -> LayoutBox {
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
