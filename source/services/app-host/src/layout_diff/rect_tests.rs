// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The layout diff's alignment on whole layouts (TASK-0354 P8): changes far apart stay apart,
//! a box that paints nothing names nothing, a bound costs damage and never correctness — and
//! the pixel properties the old window's size used to hide (an ancestor's fade, a glass root
//! over a text run).

use super::tests::{at, base, boxed, container, covers, named, run, SIZE};
use super::*;
use nexus_layout_types::{Material, Rgba8};

/// The desktop of the board's opens (`whole(130/135/42/46)`): 42 boxes of bar and wallpaper,
/// the bar button, 41 boxes of desktop under it, 46 boxes of dock.
fn desktop() -> Vec<LayoutBox> {
    let mut boxes = Vec::new();
    let mut push = |x: i32, y: i32, w: i32, h: i32| {
        let id = boxes.len() + 1;
        boxes.push(at(id, x, y, w, h));
    };
    for k in 0..42 {
        push(k * 20, 0, 18, 24); // the bar's items left of the button
    }
    push(900, 0, 60, 24); // the button, index 42
    for k in 0..41 {
        push(40 + (k % 8) * 220, 80 + (k / 8) * 150, 96, 96); // icons
    }
    for k in 0..46 {
        push(200 + k * 30, 990, 28, 60); // the dock
    }
    boxes
}

/// Gives the boxes their pre-order ids again after an insert or a removal.
fn renumber(mut boxes: Vec<LayoutBox>) -> Vec<LayoutBox> {
    for (i, b) in boxes.iter_mut().enumerate() {
        b.node_id = i + 1;
    }
    boxes
}

/// The board's bar tap: the button changes its look at index 42 and five scaffold boxes (the
/// panel's hosts — they paint nothing; the panel itself is a motion layer's) come in at index
/// 84. One window from the first change to the last named all 42 boxes between them, 800 rows
/// of the desktop; aligned, the button is the damage.
#[test]
fn a_bar_button_and_scaffold_boxes_far_below_damage_the_button_alone() {
    let old = desktop();
    let mut new = old.clone();
    new[42].visual.background = Some(Rgba8 { r: 90, g: 90, b: 90, a: 255 });
    for k in 0..5 {
        new.insert(84 + k, container(0, 0, 44, 1920, 1036));
    }
    let new = renumber(new);
    let none = |_: usize| false;
    let changed = changed_rows(&old, &new, (&[], &[]), (1920, 1080), (&none, &none));
    assert_eq!(changed.window, [130, 135, 42, 46], "the board's window");
    let rects = named(changed.base).expect("a bound");
    assert_eq!(rects, vec![(898, 0, 64, 26)], "the button and its margin");
    let px: u64 = rects.iter().map(|r| u64::from(r.2) * u64::from(r.3)).sum();
    assert!(px * 100 < 1920 * 1080, "{px} px is under 1 % of the surface");
    // Closing: the scaffold goes, the button turns back — the same rectangle.
    let closed = changed_rows(&new, &old, (&[], &[]), (1920, 1080), (&none, &none));
    assert_eq!(named(closed.base), Some(vec![(898, 0, 64, 26)]));
}

/// A block that comes in at the end of the page (a sheet the app paints) is its rectangle:
/// its columns too, not its rows across the surface.
#[test]
fn an_appended_block_damages_its_rectangle() {
    let old = vec![at(1, 0, 0, 1920, 40), at(2, 0, 40, 1920, 760)];
    let mut new = old.clone();
    new.extend([at(3, 800, 300, 320, 160), at(4, 816, 316, 288, 40)]);
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(798, 298, 324, 164)]));
    assert_eq!(base(&new, &old, &[], &[]), Some(vec![(798, 298, 324, 164)]));
}

/// Layout containers that come or go between boxes that stay change no pixel.
#[test]
fn a_box_that_paints_nothing_damages_nothing_when_it_comes_or_goes() {
    let old = vec![boxed(1, 0, 40), boxed(2, 40, 300), boxed(3, 340, 300)];
    let mut new = old.clone();
    new.insert(1, container(0, 0, 0, 1920, 800));
    new.insert(3, container(0, 0, 340, 1920, 300));
    let new = renumber(new);
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![]), "they came");
    assert_eq!(base(&new, &old, &[], &[]), Some(vec![]), "they went");
    // One that paints is named, alone.
    let mut painted = old.clone();
    painted.insert(1, at(0, 500, 10, 40, 20));
    assert_eq!(base(&old, &renumber(painted), &[], &[]), Some(vec![(498, 8, 44, 24)]));
}

/// `n` boxes in a row at `y`, no two alike.
fn row(n: usize, y: i32) -> Vec<LayoutBox> {
    (0..n as i32).map(|k| at(0, k * 9, y, 8, 8)).collect()
}

/// Past its budget the alignment calls nothing equal any more: boxes that ARE equal behind two
/// long runs of different ones are named like every other — the damage grows, and no bound is
/// taken from a comparison that never ran. Within the budget the same boxes are matched.
#[test]
fn test_reject_a_bound_from_an_alignment_past_its_budget() {
    let kept: Vec<LayoutBox> = (0..10).map(|k| at(0, 1000 + k * 40, 600, 30, 30)).collect();
    let layouts = |n: usize| {
        let (mut old, mut new) = (vec![boxed(0, 0, 4)], vec![boxed(0, 0, 4)]);
        old.extend(row(n, 100));
        new.extend(row(n + 1, 200));
        old.extend(kept.clone());
        new.extend(kept.clone());
        old.push(at(0, 0, 700, 5, 5));
        new.push(at(0, 0, 720, 5, 5)); // the lists end differently: no common suffix
        (renumber(old), renumber(new))
    };
    let kept_rect = |k: u32| (1000 + k * 40, 600, 30, 30);
    // Ten changed boxes: the kept ones are found again and name nothing.
    let (old, new) = layouts(10);
    let rects = base(&old, &new, &[], &[]).expect("a bound");
    assert!(!(0..10).any(|k| covers(&rects, kept_rect(k))), "matched: {rects:?}");
    // Two hundred: two blocks without an equal pair spend the budget.
    let (old, new) = layouts(200);
    let rects = base(&old, &new, &[], &[]).expect("still a bound: rectangles, larger");
    assert!(rects.len() <= crate::band_math::DAMAGE_RECTS);
    for k in 0..10 {
        assert!(covers(&rects, kept_rect(k)), "kept box {k} is not named in {rects:?}");
    }
    assert!(covers(&rects, (0, 100, 8, 8)) && covers(&rects, (200 * 9, 200, 8, 8)));
}

/// An ancestor's fade changes a child's EFFECTIVE opacity and leaves its `visual` as it was:
/// the child's pixels changed, so it is named (a disabled group's controls kept their full
/// colour otherwise).
#[test]
fn an_ancestors_fade_names_the_children_it_fades() {
    let old = vec![container(1, 0, 0, 400, 100), at(2, 20, 20, 80, 40), boxed(3, 300, 40)];
    let mut new = old.clone();
    new[1].opacity = 128;
    assert_eq!(new[1].visual, old[1].visual, "the child's own style did not change");
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(18, 18, 84, 44)]));
    // A run on a faded box that paints nothing of its own fades too.
    let (mut label_old, texts) = (old.clone(), vec![run(2, "Wi-Fi")]);
    label_old[1] = container(2, 20, 20, 80, 40);
    let mut label_new = label_old.clone();
    label_new[1].opacity = 128;
    assert_eq!(base(&label_old, &label_new, &texts, &texts), Some(vec![(18, 18, 84, 44)]));
}

/// A box that becomes a scroll layer's content is placed by that layer: its rows change, in
/// whatever columns the layer shows them.
#[test]
fn a_box_that_joins_a_scroll_layer_names_its_rows() {
    let old = vec![container(1, 0, 0, 400, 300), at(2, 20, 20, 80, 40)];
    let mut new = old.clone();
    new[1].scroll_root = Some(1);
    assert_eq!(base(&old, &new, &[], &[]), Some(vec![(0, 18, 1920, 44)]));
}

/// Node ids are positions: a box that comes in before a scroll container moves its number and
/// leaves its content's pixels alone — nothing is named, and the diff says from which root on
/// a band kept by number is no longer known to hold this content.
#[test]
fn a_scroll_root_whose_number_moved_is_said() {
    let list = |root: usize| {
        let mut row = at(0, 20, 60, 200, 40);
        row.scroll_root = Some(root);
        vec![
            container(0, 0, 40, 400, 300),
            row.clone(),
            LayoutBox { rect: at(0, 20, 110, 200, 40).rect, ..row },
        ]
    };
    let old = renumber([vec![boxed(0, 0, 30)], list(2)].concat());
    let mut new = [vec![boxed(0, 0, 30), container(0, 0, 0, 1920, 800)], list(3)].concat();
    new = renumber(new);
    let none = |_: usize| false;
    let changed = changed_rows(&old, &new, (&[], &[]), SIZE, (&none, &none));
    assert_eq!(named(changed.base), Some(vec![]), "no pixel changed");
    assert_eq!(changed.shifted, Some(3), "the list's root is number 3 now");
    // Nothing moved a number: nothing is said, whatever else changed.
    let mut lit = old.clone();
    lit[0].visual.background = Some(Rgba8 { r: 90, g: 90, b: 90, a: 255 });
    let changed = changed_rows(&old, &lit, (&[], &[]), SIZE, (&none, &none));
    assert_eq!((named(changed.base), changed.shifted), (Some(vec![(0, 0, 102, 32)]), None));
}

/// The glyph pass drops a run under a later glass root WHOLE: a panel that comes over one end
/// of a label takes the label's other end with it, so the label's box is named beside the
/// panel's — and again when the panel goes.
#[test]
fn a_glass_root_names_the_text_it_hides() {
    let old = vec![at(1, 0, 0, 1920, 800), container(2, 100, 100, 300, 20)];
    let texts = vec![run(2, "A label the panel covers the end of")];
    let mut panel = at(3, 350, 60, 200, 120);
    panel.visual.material = Some(Material::Panel);
    let mut new = old.clone();
    new.push(panel);
    for (a, b) in [(&old, &new), (&new, &old)] {
        let rects = base(a, b, &texts, &texts).expect("a bound");
        assert!(covers(&rects, (350, 60, 200, 120)), "the panel: {rects:?}");
        assert!(covers(&rects, (100, 100, 300, 20)), "the label's whole box: {rects:?}");
    }
    // A label the panel does not meet stays as it is.
    let apart = vec![at(1, 0, 0, 1920, 800), container(2, 100, 400, 300, 20)];
    let mut with_panel = apart.clone();
    with_panel.push(new[2].clone());
    let rects = base(&apart, &with_panel, &texts, &texts).expect("a bound");
    assert_eq!(rects, vec![(348, 58, 204, 124)]);
    // With no relayout to tell (a motion layer takes the panel's nodes): the label's box, by
    // the same rule — and nothing for nodes that hold no glass root.
    let mut named = Vec::new();
    hidden_runs(&new, &texts, (3, 4), &mut |r| named.push(r));
    assert_eq!(named, vec![(98, 98, 402, 122)], "the label's box and the margin");
    named.clear();
    hidden_runs(&new, &texts, (1, 3), &mut |r| named.push(r));
    assert!(named.is_empty());
}

/// TASK-0354 P5: a modal opening over its scrim — a full-surface root and its sheet, held by
/// the motion layer it becomes — changes no base pixel; closing it neither. A change inside a
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
    let opening = changed_rows(&page, &open, (&[], &[]), SIZE, (&none, &modal));
    assert_eq!(named(opening.base), Some(vec![]), "the base never held the modal");
    assert_eq!(opening.layers, Some((0, 800)), "the layer's rows: its store paints them");
    let closing = changed_rows(&open, &page, (&[], &[]), SIZE, (&modal, &none));
    assert_eq!(named(closing.base), Some(vec![]));
    // A row inside an open panel changes: the panel's store rows, never the base's.
    let panel = |id: usize| id == 2;
    let mut grown = page.clone();
    grown[1] = boxed(2, 40, 700);
    let inside = changed_rows(&page, &grown, (&[], &[]), SIZE, (&panel, &panel));
    assert_eq!(named(inside.base), Some(vec![]));
    assert_eq!(inside.layers, Some((38, 800)), "the panel's rows, cut at the surface");
    // The panel's layer went while its box stays and changes: the base paints it — its
    // rectangle, however much of the frame that is.
    let left = changed_rows(&page, &grown, (&[], &[]), SIZE, (&panel, &none));
    assert_eq!(named(left.base), Some(vec![(0, 38, 102, 704)]));
    assert_eq!(changed_rows(&page, &page, (&[], &[]), SIZE, (&none, &none)).layers, None);
}

/// Runs are aligned like boxes: a run that comes in between two that stay is its box alone,
/// whatever it does to the node numbers behind it.
#[test]
fn an_inserted_run_damages_its_box_alone() {
    let label = |id: usize, y: i32| container(id, 40, y, 200, 20);
    let old = vec![label(1, 0), label(2, 100), label(3, 500)];
    let new = vec![label(1, 0), label(2, 100), label(3, 300), label(4, 500)];
    let old_t = vec![run(1, "one"), run(2, "two"), run(3, "four")];
    let new_t = vec![run(1, "one"), run(2, "two"), run(3, "three"), run(4, "four")];
    assert_eq!(base(&old, &new, &old_t, &new_t), Some(vec![(38, 298, 204, 24)]));
    // A run whose box is not in the list gives no bound: everything.
    let stray = vec![run(1, "one"), run(9, "nine")];
    assert_eq!(base(&old, &old, &old_t, &stray), None);
}
