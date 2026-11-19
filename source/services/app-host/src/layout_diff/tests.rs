// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The layout diff's host tests (split out of `layout_diff.rs`, the module-size ratchet): what
//! each kind of change names. The alignment's own cases are `rect_tests.rs`.

use super::*;
use crate::band_math::{Change, Rect as Named};
use nexus_layout_types::{FxPx, Material, Rect, Rgba8, VisualStyle};

/// The layout the tests diff on.
pub(super) const SIZE: (i32, i32) = (1920, 800);

pub(super) fn boxed(id: usize, y: i32, h: i32) -> LayoutBox {
    LayoutBox {
        node_id: id,
        rect: Rect::new(FxPx::new(0), FxPx::new(y), FxPx::new(100), FxPx::new(h)),
        visual: VisualStyle {
            background: Some(Rgba8 { r: 10, g: 10, b: 10, a: 200 }),
            ..VisualStyle::default()
        },
        ..LayoutBox::default()
    }
}

/// The rectangles `(x, y, w, h)` a change names (`None`: everything).
pub(super) fn named(mut p: Pending) -> Option<Vec<Named>> {
    match p.take() {
        None => Some(Vec::new()),
        Some(Change::Whole) => None,
        Some(Change::Rects(d)) => Some(d.rects().to_vec()),
    }
}

/// What the base repaints for the change from `old` to `new`, no motion layer holding anything.
pub(super) fn base(
    old: &[LayoutBox],
    new: &[LayoutBox],
    old_texts: &[TextRun],
    new_texts: &[TextRun],
) -> Option<Vec<Named>> {
    let none = |_: usize| false;
    named(changed_rows(old, new, (old_texts, new_texts), SIZE, (&none, &none)).base)
}

/// The menu case: the in-flow prefix is untouched, an overlay subtree appears at the END of
/// the box list — the damage is exactly the overlay's rectangle (± margin), never the frame.
#[test]
fn an_appended_overlay_damages_only_its_rectangle() {
    let old = vec![boxed(1, 0, 40), boxed(2, 40, 400)];
    let mut menu = boxed(3, 60, 175);
    menu.visual.material = Some(Material::Overlay);
    let new = vec![boxed(1, 0, 40), boxed(2, 40, 400), menu];
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 58, 102, 179)]), "the overlay + 2 px");
    // Closing it damages the same rectangle.
    assert_eq!(base(&new, &old, &[], &[]), Some(vec![(0, 58, 102, 179)]));
}

/// A box's shadow paints past its rect (`nexus_scene_raster::shadow_reach`): a button that
/// gains one damages what the shadow reaches too, or the pixels around it keep the old ones
/// (the board's buttons showed a shadow cut flat at their edges).
#[test]
fn a_shadow_damages_what_it_reaches() {
    let old = vec![boxed(1, 0, 40), boxed(2, 100, 30)];
    let mut lifted = boxed(2, 100, 30);
    lifted.visual.shadow = Some(nexus_layout_types::BoxShadow {
        color: Rgba8 { r: 0, g: 0, b: 0, a: 60 },
        offset_x: FxPx::new(0),
        offset_y: FxPx::new(2),
        blur_radius: FxPx::new(6),
        spread: FxPx::new(0),
    });
    let new = vec![boxed(1, 0, 40), lifted];
    let rects = base(&old, &new, &[], &[]).expect("a bound");
    let [(x, y, w, h)] = rects[..] else { panic!("one rectangle: {rects:?}") };
    assert!(y <= 100 - 4, "from the shadow's top (6 blur - 2 offset above): {rects:?}");
    assert!(y + h >= 130 + 8, "to the shadow's bottom (6 blur + 2 offset below): {rects:?}");
    assert!(x == 0 && w >= 100 + 6, "and its side: {rects:?}");
}

/// Identical lists = nothing (state moved, pixels did not).
#[test]
fn identical_layouts_name_nothing() {
    let a = vec![boxed(1, 0, 40), boxed(2, 40, 400)];
    assert_eq!(base(&a, &a.clone(), &[], &[]), Some(vec![]));
}

/// A change most of the frame tall is ONE rectangle the row painter paints — never everything
/// (the full redraw invalidates every band and paints every motion layer again).
#[test]
fn a_near_full_change_is_one_rectangle_never_everything() {
    let old = vec![boxed(1, 0, 790)];
    let mut new = vec![boxed(1, 0, 790)];
    new[0].visual.background = Some(Rgba8 { r: 90, g: 10, b: 10, a: 200 });
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 0, 102, 792)]));
}

pub(super) fn at(id: usize, x: i32, y: i32, w: i32, h: i32) -> LayoutBox {
    let mut b = boxed(id, y, h);
    b.rect = Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h));
    b
}

pub(super) fn container(id: usize, x: i32, y: i32, w: i32, h: i32) -> LayoutBox {
    LayoutBox { visual: VisualStyle::default(), ..at(id, x, y, w, h) }
}

/// A flat band that grows damages only the rows it took, in its columns — not the band.
#[test]
fn a_growing_flat_band_damages_only_the_rows_it_took() {
    let old = vec![boxed(1, 0, 100), boxed(2, 100, 700)];
    let new = vec![boxed(1, 0, 120), boxed(2, 120, 680)];
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 98, 102, 24)]));
}

/// The screenshot tool's selection (RFC-0095): four scrim bands around a
/// framed hole. Dragging the hole 10 px down repaints the hole's rows (old
/// ∪ new, + margin) — the bands above and below are the same scrim.
#[test]
fn a_dragged_selection_damages_the_hole_rows_not_the_frame() {
    let frame = |top: i32| {
        let mut hole = at(5, 120, top, 400, 280);
        hole.visual = VisualStyle::default();
        hole.visual.border = nexus_layout_types::EdgeBorder::all(
            FxPx::new(2),
            Rgba8 { r: 255, g: 255, b: 255, a: 255 },
        );
        vec![
            container(1, 0, 0, 1280, 800),
            at(2, 0, 0, 1280, top),
            container(3, 0, top, 1280, 280),
            at(4, 0, top, 120, 280),
            hole,
            at(6, 520, top, 760, 280),
            at(7, 0, top + 280, 1280, 520 - top),
        ]
    };
    let rects = base(&frame(200), &frame(210), &[], &[]).expect("a bound");
    let rows =
        rects.iter().fold((i32::MAX, 0), |s, r| (s.0.min(r.1 as i32), s.1.max((r.1 + r.3) as i32)));
    assert_eq!(rows, (198, 492), "the hole's rows, old and new: {rects:?}");
    // The hole's frame where it was and where it is, the rows the bands took and left.
    for part in [(120, 200, 400, 290), (0, 200, 1280, 10), (0, 480, 1280, 10)] {
        assert!(covers(&rects, part), "{part:?} is not named in {rects:?}");
    }
}

/// Whether every pixel of `r` lies in one rectangle of `rects`.
pub(super) fn covers(rects: &[Named], r: Named) -> bool {
    rects
        .iter()
        .any(|q| q.0 <= r.0 && q.1 <= r.1 && q.0 + q.2 >= r.0 + r.2 && q.1 + q.3 >= r.1 + r.3)
}

/// A layout container that moves paints nothing by itself.
#[test]
fn a_moved_container_damages_nothing_by_itself() {
    let old = vec![container(1, 0, 0, 100, 40), boxed(2, 100, 30)];
    let new = vec![container(1, 0, 10, 100, 40), boxed(2, 100, 30)];
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![]));
}

/// A flat box that moves sideways changes both rectangles.
#[test]
fn a_sideways_move_damages_both_rects() {
    let old = vec![at(1, 0, 100, 50, 40)];
    let new = vec![at(1, 30, 110, 50, 40)];
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 98, 82, 54)]));
}

/// A structure change (a node added beside a changed one) names both.
#[test]
fn a_structure_change_names_what_came_and_what_changed() {
    let old = vec![boxed(1, 0, 40), boxed(2, 100, 50), boxed(9, 700, 10)];
    let new = vec![boxed(1, 0, 40), boxed(2, 100, 60), boxed(3, 160, 10), boxed(9, 700, 10)];
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 98, 102, 74)]));
}

/// Glass regions are re-declared only when a glass root moved.
#[test]
fn glass_roots_change_only_with_a_glass_root() {
    let mut panel = boxed(3, 600, 120);
    panel.visual.material = Some(Material::Panel);
    let old = vec![boxed(1, 0, 100), panel.clone()];
    let moved_band = vec![boxed(1, 0, 120), panel.clone()];
    assert!(!glass_roots_changed(&old, &moved_band));
    let mut moved_panel = panel;
    moved_panel.rect.y = FxPx::new(580);
    assert!(glass_roots_changed(&old, &[boxed(1, 0, 100), moved_panel]));
}

/// A pure TEXT change on retained boxes damages the text's box.
#[test]
fn a_text_change_damages_its_box() {
    let boxes = vec![boxed(1, 0, 40), boxed(2, 100, 30)];
    let old_t = vec![run(2, "Aus")];
    let new_t = vec![run(2, "An")];
    assert_eq!(base(&boxes, &boxes.clone(), &old_t, &new_t), Some(vec![(0, 98, 102, 34)]));
}

/// A text run for `node_id` carrying `content`.
pub(super) fn run(node_id: usize, content: &str) -> TextRun {
    TextRun {
        node: node_id,
        content: alloc::string::String::from(content),
        font: nexus_text_baked::FontSize::S14W400,
        color: [0, 0, 0, 255],
        weight: nexus_layout_types::FontWeight::Normal,
        tracking: nexus_text_baked::Tracking::NONE,
        line: 20,
        align: nexus_layout_types::TextAlign::Left,
    }
}

/// TASK-0354 P8: a top-bar button changes its look (near the START of the list) and its panel
/// appears at the END, held by a motion layer — the base's change is the button's rectangle.
/// One window over both painted every row of the desktop for each panel opened and closed
/// (`usb-visible--2026-11-18T15-58-24`: `base_paints=whole`, 75 ms).
#[test]
fn a_change_at_the_start_and_a_held_overlay_at_the_end_damage_the_start_alone() {
    let lit = |mut b: LayoutBox| {
        b.visual.background = Some(Rgba8 { r: 90, g: 90, b: 90, a: 255 });
        b
    };
    let desk = |button: LayoutBox| vec![button, boxed(2, 40, 300), boxed(3, 340, 300)];
    let (old, mut new) = (desk(boxed(1, 0, 24)), desk(lit(boxed(1, 0, 24))));
    new.extend([boxed(4, 0, 800), boxed(5, 60, 540)]); // the scrim and the panel
    let held = |id: usize| id >= 4;
    let none = |_: usize| false;
    let opened = changed_rows(&old, &new, (&[], &[]), SIZE, (&none, &held));
    assert_eq!(named(opened.base), Some(vec![(0, 0, 102, 26)]), "the button and the margin");
    assert_eq!(opened.window, [3, 3, 0, 2], "the base's boxes alone: the button is the window");
    assert_eq!(opened.layers, Some((0, 800)), "the layer's own rows");
    // Closing it: the button goes back, the layer's subtree leaves the scene.
    let closed = changed_rows(&new, &old, (&[], &[]), SIZE, (&held, &none));
    assert_eq!(named(closed.base), Some(vec![(0, 0, 102, 26)]));
    // The panel alone (the button kept its look): no base pixel at all.
    let mut same = desk(boxed(1, 0, 24));
    same.extend([boxed(4, 0, 800), boxed(5, 60, 540)]);
    assert_eq!(
        named(changed_rows(&old, &same, (&[], &[]), SIZE, (&none, &held)).base),
        Some(vec![])
    );
    // Not held (the app paints the overlay): its rectangle is the base's — one, not everything.
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 0, 102, 800)]));
}
