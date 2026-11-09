// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0117 — the app's window size under the window manager's rule, on the host:
//! the minimum (app ∨ WM, bounded by the work area), the opening size, and the clamp of every
//! resize, tile and restore. The wire path rides the QEMU lanes.
//! OWNERS: @ui

use windowd::window_size::{clamp, minimum, opening};

const WM_MIN: (u32, u32) = (204, 120);
const WORK: (u32, u32) = (1280, 764);

#[test]
fn the_apps_minimum_wins_over_the_wms_when_larger() {
    assert_eq!(minimum((340, 600), WM_MIN, WORK), (340, 600));
    assert_eq!(minimum((100, 0), WM_MIN, WORK), (204, 120), "a smaller or absent one is the WM's");
}

#[test]
fn test_reject_a_minimum_beyond_the_display() {
    // An app cannot demand more than the work area: its minimum becomes the work area.
    assert_eq!(minimum((u16::MAX, u16::MAX), WM_MIN, WORK), WORK);
    assert_eq!(minimum((2000, 600), WM_MIN, WORK), (1280, 600));
}

#[test]
fn a_freeform_window_opens_at_the_apps_default_within_its_bounds() {
    let min = minimum((340, 600), WM_MIN, WORK);
    assert_eq!(opening((340, 600), (960, 636), min, WORK), (340, 600));
    // No default: the WM's, still never below the minimum.
    assert_eq!(opening((0, 0), (960, 636), min, WORK), (960, 636));
    assert_eq!(opening((0, 0), (300, 500), min, WORK), (340, 600));
    // A default beyond the display opens at the work area.
    assert_eq!(opening((4000, 4000), (960, 636), min, WORK), WORK);
}

#[test]
fn test_reject_a_resize_below_the_app_minimum() {
    let min = minimum((340, 600), WM_MIN, WORK);
    assert_eq!(
        clamp((204, 120), min, WORK),
        (340, 600),
        "a drag to the WM floor stops at the app's"
    );
    assert_eq!(clamp((800, 700), min, WORK), (800, 700), "growing is free");
    assert_eq!(clamp((5000, 5000), min, WORK), WORK, "but never past the work area");
    // A quarter tile smaller than the minimum gives the window its minimum on that axis.
    assert_eq!(clamp((640, 382), min, WORK), (640, 600));
}
