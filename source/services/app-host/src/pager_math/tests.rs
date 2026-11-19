// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The paged track's decisions against the design's numbers (design artifacts: PagedView —
//! rubber band x0.35, a turn at 16 % of the width or 24 px within 250 ms, the axis after 6 px,
//! the wheel past 40 then 420 ms, a 500 ms glide). A viewport of 1280 px, four pages.

use super::*;

const VIEW: i32 = 1280;
const MAX: i32 = 3 * VIEW; // four pages: 0..=3
const MS: u64 = 1_000_000;

#[test]
fn the_axis_locks_after_6_px() {
    assert_eq!(axis_lock(6, 0), Lock::Undecided);
    assert_eq!(axis_lock(7, 3), Lock::Track);
    assert_eq!(axis_lock(-7, 3), Lock::Track);
    assert_eq!(axis_lock(7, 7), Lock::Cross);
    assert_eq!(axis_lock(3, 7), Lock::Cross);
    assert_eq!(axis_lock(5, 5), Lock::Undecided);
}

#[test]
fn a_drag_follows_1_to_1_between_the_ends() {
    assert_eq!(drag_offset(VIEW, -100, MAX, VIEW), VIEW + 100, "a drag left moves on");
    assert_eq!(drag_offset(VIEW, 100, MAX, VIEW), VIEW - 100);
}

#[test]
fn the_first_page_pulled_back_rubber_bands_at_35_percent() {
    assert_eq!(drag_offset(0, 100, MAX, VIEW), -35);
}

#[test]
fn the_last_page_pulled_on_rubber_bands_at_35_percent() {
    assert_eq!(drag_offset(MAX, -200, MAX, VIEW), MAX + 70);
}

/// The operator's request: with no second page the track still follows and comes back.
#[test]
fn a_single_page_rubber_bands_both_ways() {
    assert_eq!(drag_offset(0, 100, 0, VIEW), -35);
    assert_eq!(drag_offset(0, -100, 0, VIEW), 35);
    assert_eq!(release_page(0, 0, 400, 50 * MS, VIEW), 0);
    assert_eq!(release_page(0, 0, -400, 50 * MS, VIEW), 0);
    assert_eq!(last_page(0, VIEW), 0);
}

#[test]
fn a_page_turns_past_16_percent_of_the_width() {
    assert_eq!(release_page(1, 3, -205, 1_000 * MS, VIEW), 2, "205 px of 1280 turns");
    assert_eq!(release_page(1, 3, -204, 1_000 * MS, VIEW), 1, "204 px does not");
    assert_eq!(release_page(1, 3, 205, 1_000 * MS, VIEW), 0, "a drag right turns back");
}

#[test]
fn a_flick_of_more_than_24_px_within_250_ms_turns() {
    assert_eq!(release_page(1, 3, -25, 249 * MS, VIEW), 2);
    assert_eq!(release_page(1, 3, -25, 250 * MS, VIEW), 1, "at 250 ms it is no flick");
    assert_eq!(release_page(1, 3, -24, 100 * MS, VIEW), 1, "24 px is no flick");
}

#[test]
fn a_turn_stops_at_the_ends() {
    assert_eq!(release_page(0, 3, 600, 50 * MS, VIEW), 0, "the first page pulled back");
    assert_eq!(release_page(3, 3, -600, 50 * MS, VIEW), 3, "the last page pulled on");
    assert_eq!(last_page(MAX, VIEW), 3);
    assert_eq!(last_page(2 * VIEW + 300, VIEW), 3, "a ragged extent has a last part-page");
}

#[test]
fn the_wheel_turns_past_40_then_cools_for_420_ms() {
    assert_eq!(notch_units(-1), 100, "wheel down: toward the next page");
    assert_eq!(wheel_step(0, notch_units(-1), false), (0, Some(1)));
    assert_eq!(wheel_step(0, notch_units(1), false), (0, Some(-1)));
    assert_eq!(wheel_step(17, notch_units(-1), true), (17, None), "cooling drops the notch");
    assert_eq!(wheel_step(0, 30, false), (30, None), "high resolution: 30 does not turn");
    assert_eq!(wheel_step(30, 15, false), (0, Some(1)), "45 turns");
    assert_eq!(wheel_step(0, 40, false), (40, None), "exactly 40 does not");
    assert_eq!(WHEEL_COOLDOWN_NS, 420 * MS);
}

#[test]
fn the_glide_takes_500_ms_on_the_glide_curve() {
    let g = PageGlide { from_px: 0, to_px: VIEW, start_ns: 10 * MS, dur_ns: 500 * MS };
    assert_eq!(g.at(10 * MS), (0, false));
    let (mid, done) = g.at(260 * MS);
    let want = (VIEW as f32 * animation::keyframe::cubic_bezier(GLIDE_CURVE, 0.5)).round() as i32;
    assert!(!done && mid == want, "{mid} vs {want}");
    let share = mid as f32 / VIEW as f32;
    assert!((0.95..=0.97).contains(&share), "most of the way at half time: {share}");
    assert_eq!(g.at(510 * MS), (VIEW, true));
}

#[test]
fn a_released_rubber_band_glides_home() {
    let g = PageGlide { from_px: -35, to_px: 0, start_ns: 0, dur_ns: 500 * MS };
    let mut last = -35;
    for ms in (0..=500u64).step_by(10) {
        let (at, _) = g.at(ms * MS);
        assert!(at >= last && at <= 0, "monotone home: {at} after {last} at {ms} ms");
        last = at;
    }
    assert_eq!(g.at(500 * MS), (0, true));
}

#[test]
fn a_reduced_motion_theme_lands_at_once() {
    let g = PageGlide { from_px: 0, to_px: 2 * VIEW, start_ns: 5, dur_ns: 0 };
    assert_eq!(g.at(0), (2 * VIEW, true));
    assert_eq!(g.at(5), (2 * VIEW, true));
}

#[test]
fn test_reject_an_offset_past_the_rubber_bound() {
    assert_eq!(rubber(i32::MIN, MAX, VIEW), -VIEW, "never past one viewport before the start");
    assert_eq!(rubber(i32::MAX, MAX, VIEW), MAX + VIEW, "never past one viewport after the end");
    assert_eq!(drag_offset(0, i32::MAX, MAX, VIEW), -VIEW);
    assert_eq!(drag_offset(MAX, i32::MIN, MAX, VIEW), MAX + VIEW);
}

/// A press in a paged viewport ends as exactly one of a tap and a track drag.
#[test]
fn test_reject_a_drag_that_is_also_a_tap() {
    for lock in [Lock::Undecided, Lock::Track, Lock::Cross] {
        for held in [false, true] {
            let end = gesture_end(lock, held);
            let tap = end == GestureEnd::Tap;
            let track = end == GestureEnd::Track;
            assert!(!(tap && track));
            assert_eq!(track, lock == Lock::Track, "{lock:?} {held}");
            assert_eq!(tap, lock == Lock::Undecided && held, "{lock:?} {held}");
        }
    }
}

#[test]
fn page_of_rounds_to_nearest() {
    assert_eq!(page_of(0, VIEW), 0);
    assert_eq!(page_of(VIEW - 10, VIEW), 1);
    assert_eq!(page_of(VIEW + 10, VIEW), 1);
    assert_eq!(page_target_px(3, VIEW, 2 * VIEW + 300), 2 * VIEW + 300, "clamped to the extent");
}
