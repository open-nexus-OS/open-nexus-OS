// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.motion`'s FLIP arithmetic against the cases the shell has: a page dot that stretches to a
//! capsule and brightens, its neighbours that shift, a chevron that turns, a retarget mid-flight.

use super::*;

fn rest(x: i32, w: i32, opacity: u8) -> Rest {
    Rest { x, y: 10, w, h: 8, rot: 0, opacity }
}

#[test]
fn a_first_layout_or_an_unchanged_one_is_no_motion() {
    let r = rest(100, 8, 77);
    assert_eq!(deviation(r, r, Deviation::IDENTITY), None);
    let rounded = Rest { x: 100, ..r };
    assert_eq!(deviation(r, rounded, Deviation::IDENTITY), None);
}

#[test]
fn a_dot_that_becomes_the_capsule_starts_where_the_dot_was() {
    // The inactive dot (8 wide at x 100, 30 %) becomes the active capsule (20 wide at x 94, 92 %).
    let dev = deviation(rest(100, 8, 77), rest(94, 20, 235), Deviation::IDENTITY).expect("moves");
    assert!((dev.dx - 0.0).abs() < 0.01, "same centre: {}", dev.dx);
    assert!((dev.sx - 0.4).abs() < 0.001, "8 of 20 wide");
    assert!((dev.opacity - 77.0 / 235.0).abs() < 0.001, "up from the share it was seen at");
    assert_eq!(dev.base, None);
}

#[test]
fn a_capsule_that_becomes_a_dot_dims_over_a_base_of_where_it_was() {
    let dev = deviation(rest(94, 20, 235), rest(100, 8, 77), Deviation::IDENTITY).expect("moves");
    assert_eq!(dev.base, Some(235));
    assert_eq!(dev.opacity, 1.0);
    assert!((opacity_target(&dev, 77) - 77.0 / 235.0).abs() < 0.001);
}

#[test]
fn a_neighbour_that_shifts_translates_only() {
    let dev = deviation(rest(120, 8, 77), rest(132, 8, 77), Deviation::IDENTITY).expect("moves");
    assert_eq!((dev.dx, dev.sx, dev.opacity, dev.base), (-12.0, 1.0, 1.0, None));
}

#[test]
fn a_chevron_turns_back_from_where_it_was() {
    let old = Rest { rot: 0, ..rest(10, 12, 255) };
    let new = Rest { rot: 180, ..old };
    let dev = deviation(old, new, Deviation::IDENTITY).expect("turns");
    assert_eq!(dev.rot, -180.0, "it starts at its old angle and turns to 180");
}

/// A change while the previous one still travels starts from where the node is SEEN.
#[test]
fn a_retarget_mid_flight_keeps_what_is_seen() {
    let (a, b, c) = (rest(100, 8, 255), rest(140, 8, 255), rest(60, 8, 255));
    let first = deviation(a, b, Deviation::IDENTITY).expect("moves");
    assert_eq!(first.dx, -40.0);
    // Halfway home the node is seen at x 120 (centre 124); now it is laid out at 60.
    let carry = Deviation { dx: -20.0, ..first };
    let second = deviation(b, c, carry).expect("moves");
    assert_eq!(second.dx, 60.0, "seen at 124, laid out at 64: no jump");
}

#[test]
fn test_reject_a_zero_size_rest_as_a_scale_divisor() {
    let dev = deviation(rest(10, 8, 255), rest(10, 0, 255), Deviation::IDENTITY);
    assert!(dev.is_none_or(|d| d.sx.is_finite() && d.sy.is_finite()));
    let dev =
        deviation(rest(10, 8, 255), Rest { opacity: 0, ..rest(10, 8, 255) }, Deviation::IDENTITY)
            .expect("fades");
    assert!(dev.opacity.is_finite() && dev.base == Some(255));
}
