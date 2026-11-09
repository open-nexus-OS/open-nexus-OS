// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0351 P16 — the per-row cost of each primitive the CPU painter draws, measured
//! on the host (`cargo test -p nexus-scene-raster --release --test row_cost_bench -- --ignored
//! --nocapture`). Numbers only: what one panel-wide row costs per shadow, glass fill, stroked
//! icon, filled symbol, animated box.

use nexus_layout::LayoutBox;
use nexus_layout_types::{
    BoxShadow, CornerRadius, FxPx, Material, PathPoint, PathShape, Rect, Rgba8, ShapeKind,
    VisualStyle,
};
use nexus_scene_raster::{paint_row, paint_row_picked_indexed, NodeAnim, RowCanvas};
use std::time::Instant;

const W: i32 = 1920;

fn boxed(node_id: usize, (x, y, w, h): (i32, i32, i32, i32), visual: VisualStyle) -> LayoutBox {
    LayoutBox {
        node_id,
        rect: Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h)),
        visual,
        ..LayoutBox::default()
    }
}

fn glass_panel(shadow: bool) -> LayoutBox {
    boxed(
        1,
        (600, 100, 720, 600),
        VisualStyle {
            background: Some(Rgba8::new(30, 30, 40, 160)),
            corner_radius: CornerRadius::uniform(FxPx::new(24)),
            material: Some(Material::Panel),
            shadow: shadow.then_some(BoxShadow {
                offset_x: FxPx::new(0),
                offset_y: FxPx::new(12),
                blur_radius: FxPx::new(24),
                spread: FxPx::new(0),
                color: Rgba8::new(0, 0, 0, 90),
            }),
            ..VisualStyle::default()
        },
    )
}

/// A line icon the size of a Lucide glyph: 6 polylines of 10 points each (a bell, a wifi arc).
fn stroke_icon(node_id: usize, x: i32, y: i32, size: i32) -> LayoutBox {
    let paths = (0..6)
        .map(|p| PathShape {
            points: (0..10)
                .map(|i| {
                    PathPoint::new((i * 100 + p * 7) as u16, ((i * 37 + p * 150) % 1000) as u16)
                })
                .collect(),
            closed: false,
        })
        .collect();
    boxed(
        node_id,
        (x, y, size, size),
        VisualStyle {
            background: Some(Rgba8::new(230, 230, 240, 255)),
            shape: ShapeKind::Stroke { paths, width_milli: 125 },
            ..VisualStyle::default()
        },
    )
}

/// A filled symbol of 40 points (a star, a chevron).
fn path_icon(node_id: usize, x: i32, y: i32, size: i32) -> LayoutBox {
    let points =
        (0..40).map(|i| PathPoint::new((i * 25) as u16, ((i * 173) % 1000) as u16)).collect();
    boxed(
        node_id,
        (x, y, size, size),
        VisualStyle {
            background: Some(Rgba8::new(230, 230, 240, 255)),
            shape: ShapeKind::Path(PathShape { points, closed: true }),
            ..VisualStyle::default()
        },
    )
}

fn card(node_id: usize, y: i32) -> LayoutBox {
    boxed(
        node_id,
        (640, y, 640, 56),
        VisualStyle {
            background: Some(Rgba8::new(255, 255, 255, 24)),
            corner_radius: CornerRadius::uniform(FxPx::new(12)),
            material: Some(Material::Card),
            ..VisualStyle::default()
        },
    )
}

fn rows_us(label: &str, rows: std::ops::Range<i32>, mut f: impl FnMut(&mut RowCanvas<'_>)) {
    let mut buf = vec![0u8; W as usize * 4];
    let n = (rows.end - rows.start) as u64;
    let t0 = Instant::now();
    for _ in 0..4 {
        for y in rows.clone() {
            let mut canvas = RowCanvas::new(&mut buf, y, W);
            f(&mut canvas);
        }
    }
    let per_row = t0.elapsed().as_nanos() as u64 / (4 * n);
    println!("{label:<44} {per_row:>7} ns/row  x{n} rows = {:>6} us", per_row * n / 1000);
}

#[test]
#[ignore]
fn row_cost_of_each_primitive() {
    let panel = glass_panel(false);
    rows_us("glass root 720px, no shadow", 100..700, |c| paint_row(c, &[panel.clone()]));
    let panel_sh = glass_panel(true);
    rows_us("glass root 720px + shadow xxl(24)", 76..736, |c| paint_row(c, &[panel_sh.clone()]));
    let icons: Vec<LayoutBox> =
        (0..8i32).map(|i| stroke_icon(10 + i as usize, 620 + i * 80, 120, 24)).collect();
    rows_us("8 stroke icons 24px (6x10 pts)", 120..144, |c| paint_row(c, &icons));
    let big: Vec<LayoutBox> =
        (0..4i32).map(|i| stroke_icon(20 + i as usize, 620 + i * 160, 120, 64)).collect();
    rows_us("4 stroke icons 64px (6x10 pts)", 120..184, |c| paint_row(c, &big));
    let syms: Vec<LayoutBox> =
        (0..8i32).map(|i| path_icon(30 + i as usize, 620 + i * 80, 120, 24)).collect();
    rows_us("8 path symbols 24px (40 pts)", 120..144, |c| paint_row(c, &syms));
    let cards: Vec<LayoutBox> = (0..8i32).map(|i| card(40 + i as usize, 120 + i * 64)).collect();
    rows_us("8 subtle glass cards 640px", 120..632, |c| paint_row(c, &cards));
    // The panel's animation frame: the same panel, translated and faded — the per-node path.
    let anim = NodeAnim { opacity: 180, dy: -12, ..NodeAnim::identity(1) };
    rows_us("glass root 720px ANIMATED (dy, opacity)", 100..700, |c| {
        paint_row_picked_indexed(c, &[panel.clone()], &[0], &[0], &[], &[anim.clone()])
    });
    let anim_sh = anim.clone();
    rows_us("glass root + shadow ANIMATED", 76..736, |c| {
        paint_row_picked_indexed(c, &[panel_sh.clone()], &[0], &[0], &[], &[anim_sh.clone()])
    });
    let ianim: Vec<NodeAnim> =
        (0..8).map(|i| NodeAnim { opacity: 180, dy: -12, ..NodeAnim::identity(10 + i) }).collect();
    let idx: Vec<i16> = (0..8).collect();
    let pick: Vec<u32> = (0..8).collect();
    rows_us("8 stroke icons 24px ANIMATED", 120..144, |c| {
        paint_row_picked_indexed(c, &icons, &pick, &idx, &[], &ianim)
    });
}
