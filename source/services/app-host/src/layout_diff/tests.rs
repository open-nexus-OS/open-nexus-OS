// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The layout diff's host tests (split out of `layout_diff.rs`, the module-size ratchet).

use super::*;
use nexus_layout_types::{FxPx, Material, Rect, Rgba8, VisualStyle};

fn boxed(id: usize, y: i32, h: i32) -> LayoutBox {
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

/// The menu case: the in-flow prefix is untouched, an overlay subtree
/// appears at the END of the box list — the span is exactly the overlay's
/// rows (± margin), never the whole frame.
#[test]
fn an_appended_overlay_damages_only_its_rows() {
    let old = vec![boxed(1, 0, 40), boxed(2, 40, 400)];
    let mut menu = boxed(3, 60, 175);
    menu.visual.material = Some(Material::Overlay);
    let new = vec![boxed(1, 0, 40), boxed(2, 40, 400), menu];
    let span = changed_row_span(&old, &new, &[], &[], 800);
    assert_eq!(span, Some((58, 237)), "overlay rows + 2px margin");
    // Closing it damages the same rows.
    let span = changed_row_span(&new, &old, &[], &[], 800);
    assert_eq!(span, Some((58, 237)));
}

/// A box's shadow paints past its rect (`nexus_scene_raster::shadow_reach`): a button that
/// gains one damages the rows the shadow reaches too, or the rows above and below keep the
/// old pixels (the board's buttons showed a shadow cut flat at their edges).
#[test]
fn a_shadow_damages_the_rows_it_reaches() {
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
    let span = changed_row_span(&old, &new, &[], &[], 800).expect("rows changed");
    assert!(span.0 <= 100 - 4, "from the shadow's top (6 blur - 2 offset above): {span:?}");
    assert!(span.1 >= 130 + 8, "to the shadow's bottom (6 blur + 2 offset below): {span:?}");
}

/// TASK-0354 P5: a modal opening over its scrim — a full-surface root and its sheet, held by
/// the motion layer it becomes — changes no base row; closing it neither. A change inside a
/// held box (a row of an open panel) names the layers' rows, not the base's; a box that
/// leaves a layer for the base names the base's.
#[test]
fn what_a_motion_layer_holds_never_damages_the_base() {
    let page = vec![boxed(1, 0, 40), boxed(2, 40, 760)];
    let mut open = page.clone();
    open.push(at(3, 0, 0, 1920, 800)); // the scrim root
    open.push(at(4, 800, 300, 320, 160)); // the sheet
    let none = |_: usize| false;
    let modal = |id: usize| (3..5).contains(&id);
    let opening = changed_rows(&page, &open, (&[], &[]), 800, (&none, &modal));
    assert_eq!(opening.base, Some((0, 0)), "no base row: the base never held the modal");
    assert_eq!(opening.layers, Some((0, 800)), "the layer's rows: its store paints them");
    let closing = changed_rows(&open, &page, (&[], &[]), 800, (&modal, &none));
    assert_eq!(closing.base, Some((0, 0)));
    // A row inside an open panel changes: the panel's store rows, never the base's.
    let panel = |id: usize| id == 2;
    let mut grown = page.clone();
    grown[1] = boxed(2, 40, 700);
    let inside = changed_rows(&page, &grown, (&[], &[]), 800, (&panel, &panel));
    assert_eq!(inside.base, Some((0, 0)));
    assert_eq!(inside.layers, Some((38, 800)), "the panel's rows, cut at the surface");
    // The panel's layer went while its box stays and changes: the base paints it.
    let left = changed_rows(&page, &grown, (&[], &[]), 800, (&panel, &none));
    assert_eq!(left.base, None, "38..800 is most of the frame: everything");
    assert_eq!(changed_rows(&page, &page, (&[], &[]), 800, (&none, &none)).layers, None);
}

/// Identical lists = empty span (state moved, pixels did not).
#[test]
fn identical_layouts_yield_an_empty_span() {
    let a = vec![boxed(1, 0, 40), boxed(2, 40, 400)];
    assert_eq!(changed_row_span(&a, &a.clone(), &[], &[], 800), Some((0, 0)));
}

/// A change spanning most of the frame degrades to FULL (`None`).
#[test]
fn a_near_full_change_degrades_to_full() {
    let old = vec![boxed(1, 0, 790)];
    let mut new = vec![boxed(1, 0, 790)];
    new[0].visual.background = Some(Rgba8 { r: 90, g: 10, b: 10, a: 200 });
    assert_eq!(changed_row_span(&old, &new, &[], &[], 800), None);
}

fn at(id: usize, x: i32, y: i32, w: i32, h: i32) -> LayoutBox {
    let mut b = boxed(id, y, h);
    b.rect = Rect::new(FxPx::new(x), FxPx::new(y), FxPx::new(w), FxPx::new(h));
    b
}

fn container(id: usize, x: i32, y: i32, w: i32, h: i32) -> LayoutBox {
    LayoutBox { visual: VisualStyle::default(), ..at(id, x, y, w, h) }
}

/// A flat band that grows damages only the rows it took — not the band.
#[test]
fn a_growing_flat_band_damages_only_the_rows_it_took() {
    let old = vec![boxed(1, 0, 100), boxed(2, 100, 700)];
    let new = vec![boxed(1, 0, 120), boxed(2, 120, 680)];
    assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((98, 122)));
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
    let span = changed_row_span(&frame(200), &frame(210), &[], &[], 800);
    assert_eq!(span, Some((198, 492)));
}

/// A layout container that moves paints nothing by itself.
#[test]
fn a_moved_container_damages_nothing_by_itself() {
    let old = vec![container(1, 0, 0, 100, 40), boxed(2, 100, 30)];
    let new = vec![container(1, 0, 10, 100, 40), boxed(2, 100, 30)];
    assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((0, 0)));
}

/// A flat box that moves sideways changes both rects' rows.
#[test]
fn a_sideways_move_damages_both_rects() {
    let old = vec![at(1, 0, 100, 50, 40)];
    let new = vec![at(1, 30, 110, 50, 40)];
    assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((98, 152)));
}

/// A structure change (a node added inside the window) keeps the union.
#[test]
fn a_structure_change_keeps_the_union() {
    let old = vec![boxed(1, 0, 40), boxed(2, 100, 50), boxed(9, 700, 10)];
    let new = vec![boxed(1, 0, 40), boxed(2, 100, 60), boxed(3, 160, 10), boxed(9, 700, 10)];
    assert_eq!(changed_row_span(&old, &new, &[], &[], 800), Some((98, 172)));
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

/// The present loop's span merge: a pending full repaint wins, spans union.
#[test]
fn merge_span_unions_and_full_wins() {
    let (mut dirty, mut rows) = (false, None);
    merge_span(&mut dirty, &mut rows, (10, 20));
    merge_span(&mut dirty, &mut rows, (5, 12));
    assert_eq!((dirty, rows), (true, Some((5, 20))));
    let (mut dirty, mut rows) = (true, None);
    merge_span(&mut dirty, &mut rows, (10, 20));
    assert_eq!((dirty, rows), (true, None));
}

/// A pure TEXT change on retained boxes damages the text's box rows.
#[test]
fn a_text_change_damages_its_box_rows() {
    let boxes = vec![boxed(1, 0, 40), boxed(2, 100, 30)];
    let old_t = vec![run(2, "Aus")];
    let new_t = vec![run(2, "An")];
    let span = changed_row_span(&boxes, &boxes.clone(), &old_t, &new_t, 800);
    assert_eq!(span, Some((98, 132)));
}

/// A text run for `node_id` carrying `content`.
fn run(node_id: usize, content: &str) -> TextRun {
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
/// appears at the END, held by a motion layer — the base's change is the button's rows. One
/// window over both painted every row of the desktop for each panel opened and closed
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
    let opened = changed_rows(&old, &new, (&[], &[]), 800, (&none, &held));
    assert_eq!(opened.base, Some((0, 26)), "the button's rows and the margin");
    assert_eq!(last_window(), [3, 3, 0, 2], "the base's boxes alone: the button is the window");
    assert_eq!(opened.layers, Some((0, 800)), "the layer's own rows");
    // Closing it: the button goes back, the layer's subtree leaves the scene.
    let closed = changed_rows(&new, &old, (&[], &[]), 800, (&held, &none));
    assert_eq!(closed.base, Some((0, 26)));
    // The panel alone (the button kept its look): no base row at all.
    let mut same = desk(boxed(1, 0, 24));
    same.extend([boxed(4, 0, 800), boxed(5, 60, 540)]);
    assert_eq!(changed_rows(&old, &same, (&[], &[]), 800, (&none, &held)).base, Some((0, 0)));
    // Not held (the app paints the overlay): its rows are the base's — everything.
    assert_eq!(changed_rows(&old, &new, (&[], &[]), 800, (&none, &none)).base, None);
}
