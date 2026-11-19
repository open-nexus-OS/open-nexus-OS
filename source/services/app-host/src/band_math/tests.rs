// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The band and damage decisions' host tests (split out of `band_math.rs`, the module-size
//! ratchet).

use super::*;

/// A layer is covered by what an overlay paints over its viewport: a sheet over the content
/// covers it; a full-bleed host painting nothing (absent from `painted`), an overlay beside
/// the viewport or one drawn before the layer's root does not — board cycle 2026-11-10: every
/// bare overlay counted by its box, and a closed menu's full-window host froze the scrolling.
#[test]
fn a_layer_is_covered_by_what_an_overlay_paints_and_not_by_its_box() {
    let viewport = (300, 60, 1200, 800);
    let sheet = (40, (0, 52, 320, 800));
    assert!(covered(10, viewport, &[sheet]), "the open sheet lies over the content");
    assert!(!covered(10, viewport, &[]), "a host painting nothing covers nothing");
    assert!(!covered(10, viewport, &[(40, (0, 52, 280, 800))]), "beside the viewport");
    assert!(!covered(50, viewport, &[sheet]), "drawn before the layer's root");
}

/// A 1400-position ring for a 700-row viewport.
fn band() -> Band {
    Band { bx: 0, by: 869, strip_len: 1400, strips: 1, valid_start: 0, valid_len: 0 }
}

#[test]
fn an_empty_band_draws_its_visible_window_first() {
    let f = next_fill(&band(), 700, 5000, 300, false).expect("a fill");
    assert_eq!((f.from, f.to, f.visible, f.restart), (300, 1000, true, true));
}

#[test]
fn then_the_margins_ahead_and_behind() {
    let mut b = band();
    grow(&mut b, 300, 1000);
    let ahead = next_fill(&b, 700, 5000, 300, false).expect("ahead");
    assert_eq!((ahead.from, ahead.to, ahead.visible, ahead.forward), (1000, 1350, false, true));
    grow(&mut b, 1000, 1350);
    let behind = next_fill(&b, 700, 5000, 300, false).expect("behind");
    assert_eq!((behind.from, behind.to, behind.forward), (0, 300, false));
    grow(&mut b, 0, 300);
    assert_eq!(next_fill(&b, 700, 5000, 300, false), None, "the target is held");
    assert_eq!((b.valid_start, b.valid_len), (0, 1350));
}

/// Scrolling on: the visible window's new rows come before any margin, and the ring drops
/// what it overwrites at the far end.
#[test]
fn scrolling_fills_the_window_first_and_the_ring_forgets_behind() {
    let mut b = band();
    grow(&mut b, 0, 1050);
    let f = next_fill(&b, 700, 5000, 400, false).expect("the window's gap");
    assert_eq!((f.from, f.to, f.visible), (1050, 1100, true));
    grow(&mut b, 1050, 1100);
    let f = next_fill(&b, 700, 5000, 400, false).expect("the margin");
    assert!(!f.visible && f.forward);
    grow(&mut b, 1100, 1450);
    assert_eq!((b.valid_start, b.valid_end()), (50, 1450), "1400 held, the first 50 overwritten");
}

/// A jump past everything held starts over at the new window — never a stale range shown.
#[test]
fn test_reject_keeping_a_range_the_window_left() {
    let mut b = band();
    grow(&mut b, 0, 1400);
    let f = next_fill(&b, 700, 9000, 6000, false).expect("restart");
    assert!(f.restart && f.visible);
    assert_eq!((f.from, f.to), (6000, 6700));
}

#[test]
fn a_covered_or_empty_layer_draws_nothing() {
    assert_eq!(next_fill(&band(), 700, 5000, 0, true), None);
    assert_eq!(next_fill(&band(), 700, 0, 0, false), None);
    assert_eq!(next_fill(&Band::default(), 700, 5000, 0, false), None);
}

#[test]
fn a_run_never_crosses_a_strip_end() {
    // Two 960-column strips: positions 900..1100 cross the first strip's end at 960.
    let f = Fill { from: 900, to: 1100, visible: true, forward: true, restart: false };
    assert_eq!(run_of(&f, 960, 1000), (900, 960));
    let back = Fill { from: 900, to: 1100, visible: false, forward: false, restart: false };
    assert_eq!(run_of(&back, 960, 1000), (960, 1100), "backward: the run ends at `to`");
    assert_eq!(run_of(&back, 960, 64), (1036, 1100), "a chunk at a time");
}

#[test]
fn a_backward_run_trims_the_far_end_the_ring_overwrote() {
    let mut b = band();
    grow(&mut b, 600, 2000);
    grow(&mut b, 500, 600);
    assert_eq!((b.valid_start, b.valid_end()), (500, 1900));
}

/// The desktop the damage tests name their rectangles on.
const BASE: (u32, u32) = (1920, 1080);

/// Whether every pixel of `r` lies in one rectangle of `d`.
fn covers(d: &PresentDamage, r: Rect) -> bool {
    d.rects()
        .iter()
        .any(|q| q.0 <= r.0 && q.1 <= r.1 && q.0 + q.2 >= r.0 + r.2 && q.1 + q.3 >= r.1 + r.3)
}

#[test]
fn present_damage_stays_within_four_rectangles() {
    let mut d = PresentDamage::default();
    assert!(d.is_empty());
    let named: Vec<Rect> = (0..6u32).map(|k| (0, k * 100, 1440, 10)).collect();
    for r in &named {
        d.add(*r);
    }
    assert_eq!(d.rects().len(), DAMAGE_RECTS);
    assert!(named.iter().all(|r| covers(&d, *r)), "every named row is still named: {d:?}");
    d.add((0, 0, 0, 10));
    assert_eq!(d.rects().len(), DAMAGE_RECTS, "an empty rectangle names nothing");
}

/// A band's runs, one under the other, are one rectangle: their union is exactly the two.
#[test]
fn rectangles_whose_union_costs_no_more_are_one() {
    let mut d = PresentDamage::default();
    d.add((40, 900, 600, 8));
    d.add((40, 908, 600, 8));
    assert_eq!(d.rects(), &[(40, 900, 600, 16)]);
    // One inside another names nothing new; two that overlap by half do not merge by this rule
    // (their union holds pixels neither names).
    d.add((100, 904, 20, 4));
    assert_eq!(d.rects(), &[(40, 900, 600, 16)]);
    d.add((340, 908, 600, 16));
    assert_eq!(d.rects().len(), 2);
    assert_eq!(d.px(), 600 * 16 * 2);
}

/// TASK-0354 P8: a bar button and a panel far below it are two rectangles — one span through
/// both named every row of the desktop between them.
#[test]
fn two_changes_far_apart_stay_two_rectangles() {
    let mut p = Pending::default();
    assert!(p.is_empty());
    p.add((300, 0, 360, 24), BASE);
    p.add((1500, 900, 1900, 1000), BASE);
    let Some(Change::Rects(d)) = p.take() else { panic!("two rectangles") };
    assert_eq!(d.rects(), &[(300, 0, 60, 24), (1500, 900, 400, 100)]);
    assert!(d.px() < 1920 * 1080 / 40, "{} px of the desktop's 2 Mpx", d.px());
    assert!(p.is_empty() && p.take().is_none(), "taken: nothing is owed");
}

/// Above the bound the cheapest pair merges — among all five, not the newcomer into its
/// nearest: a fifth rectangle far from everything leaves the two neighbours to become one.
#[test]
fn a_fifth_rectangle_merges_the_cheapest_pair() {
    let mut d = PresentDamage::default();
    let (a, b) = ((0, 0, 10, 10), (12, 0, 10, 10));
    let (c, e, f) = ((1000, 0, 10, 10), (0, 1000, 10, 10), (1000, 1000, 10, 10));
    for r in [a, b, c, e] {
        d.add(r);
    }
    assert_eq!(d.rects().len(), 4, "two apart by a gap stay two under the bound");
    d.add(f);
    assert_eq!(d.rects(), &[(0, 0, 22, 10), c, e, f], "the neighbours merged: 20 px more");
    // The damage grew: everything named before is named still.
    assert!([a, b, c, e, f].iter().all(|r| covers(&d, *r)));
    assert!(d.px() >= 500);
}

/// A widget loop or a field's selection knows rows alone: every column of them — and a
/// rectangle those rows hold is theirs.
#[test]
fn a_rows_only_contributor_names_the_full_width() {
    let mut p = Pending::default();
    p.add((300, 700, 630, 900), BASE); // an animated panel's rectangle
    p.rows((100, 120), BASE.0);
    assert_eq!(p.take(), Some(Change::Rects(damage(&[(300, 700, 330, 200), (0, 100, 1920, 20)]))));
    p.add((300, 44, 630, 650), BASE);
    p.rows((40, 700), BASE.0);
    assert_eq!(p.take(), Some(Change::Rects(damage(&[(0, 40, 1920, 660)]))));
    // Rows above the base start at its first row.
    p.rows((-8, 4), BASE.0);
    assert_eq!(p.take(), Some(Change::Rects(damage(&[(0, 0, 1920, 4)]))));
}

fn damage(rects: &[Rect]) -> PresentDamage {
    let mut d = PresentDamage::default();
    for r in rects {
        d.add(*r);
    }
    d
}

#[test]
fn a_rectangle_is_cut_to_the_base_and_one_outside_it_names_nothing() {
    let mut p = Pending::default();
    p.add((-5, -5, 2000, 30), BASE);
    assert_eq!(p.take(), Some(Change::Rects(damage(&[(0, 0, 1920, 30)]))));
    p.add((1930, 10, 1940, 20), BASE);
    p.add((10, 1080, 20, 1200), BASE);
    p.add((50, 50, 50, 90), BASE);
    assert!(p.is_empty(), "nothing of them lies on the base");
}

/// Everything is never narrowed: a rectangle added after it, or merged into it, changes nothing,
/// and merging everything into rectangles is everything.
#[test]
fn test_reject_narrowing_a_whole_change() {
    let mut p = Pending::default();
    p.whole();
    p.add((0, 0, 10, 10), BASE);
    p.rows((5, 9), BASE.0);
    let mut rects = Pending::default();
    rects.add((0, 0, 10, 10), BASE);
    p.merge(rects);
    assert!(!p.is_empty());
    assert_eq!(p.take(), Some(Change::Whole));
    let mut whole = Pending::default();
    whole.whole();
    rects.merge(whole);
    assert_eq!(rects.take(), Some(Change::Whole));
}

#[test]
fn merging_two_changes_names_both() {
    let (mut a, mut b) = (Pending::default(), Pending::default());
    a.add((0, 0, 10, 10), BASE);
    b.add((500, 500, 520, 520), BASE);
    a.merge(b);
    a.merge(Pending::default());
    assert_eq!(a.take(), Some(Change::Rects(damage(&[(0, 0, 10, 10), (500, 500, 20, 20)]))));
}

/// The rows a set covers, once each: two rectangles side by side share their rows, one far
/// below has its own.
#[test]
fn a_sets_rows_are_disjoint_intervals() {
    let d = damage(&[(0, 100, 5, 5), (50, 15, 5, 10), (0, 10, 5, 10), (900, 25, 5, 5)]);
    let (rows, n) = d.row_intervals();
    assert_eq!(&rows[..n], &[(10, 30), (100, 105)]);
    assert_eq!(PresentDamage::default().row_intervals().1, 0);
}

#[test]
fn a_trace_line_says_the_count_and_the_pixels() {
    let mut p = Pending::default();
    assert_eq!(format!("{p}"), "none");
    p.add((300, 0, 360, 24), BASE);
    p.add((1500, 900, 1900, 1000), BASE);
    assert_eq!(format!("{p}"), "rects(n=2 px=41440)");
    p.whole();
    assert_eq!(format!("{p}"), "whole");
}
