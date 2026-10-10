// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's frame-aligned input staging on the host: motion that arrives within one
//! frame coalesces to the newest position, wheel notches sum, one-shot facts survive — and a
//! button edge is never coalesced: a press and its release drained in one busy pass are both
//! applied, each at its own position (the click the usb-visible lane lost on its first try).
//! OWNERS: @ui

use input_live_protocol::VisibleState;
use windowd::input_stage::InputStage;

/// A sample at `(x, y)` with the primary button `held`.
fn at(x: i32, y: i32, held: bool) -> VisibleState {
    VisibleState { cursor_x: x, cursor_y: y, launcher_click_visible: held, ..Default::default() }
}

/// Drives the stage like windowd's loop: every queued push is staged (an edge it hands back is
/// applied at once), then the frame applies what is left. Returns the applied samples in order.
fn drain(stage: &mut InputStage, batch: &[VisibleState]) -> Vec<VisibleState> {
    let mut applied = Vec::new();
    for state in batch {
        applied.extend(stage.stage(*state));
    }
    applied.extend(stage.take());
    applied
}

fn edges(applied: &[VisibleState]) -> Vec<(bool, i32, i32)> {
    applied.iter().map(|s| (s.launcher_click_visible, s.cursor_x, s.cursor_y)).collect()
}

/// A click shorter than one busy pass: press and release arrive in the same drain. Both are
/// applied — the press first, at the press position.
#[test]
fn a_click_inside_one_busy_frame_keeps_its_press() {
    let mut stage = InputStage::new();
    let applied = drain(&mut stage, &[at(1220, 18, true), at(1220, 18, false)]);
    assert_eq!(edges(&applied), vec![(true, 1220, 18), (false, 1220, 18)]);
}

/// Motion within a frame coalesces to the newest position: one sample per frame.
#[test]
fn motion_within_a_frame_coalesces_to_the_newest_position() {
    let mut stage = InputStage::new();
    let applied = drain(&mut stage, &[at(10, 10, false), at(20, 15, false), at(30, 20, false)]);
    assert_eq!(edges(&applied), vec![(false, 30, 20)]);
}

/// Motion queued behind a press does not move the press: the press is applied where the button
/// went down, the drag's motion coalesces, the release carries the final position.
#[test]
fn a_drag_keeps_its_press_position_and_coalesces_its_motion() {
    let mut stage = InputStage::new();
    let applied = drain(
        &mut stage,
        &[at(120, 140, true), at(200, 200, true), at(400, 380, true), at(520, 420, false)],
    );
    assert_eq!(edges(&applied), vec![(true, 120, 140), (false, 520, 420)]);
}

/// A drag across frames: the frame in the middle applies the newest held position only.
#[test]
fn a_drag_across_frames_applies_one_motion_sample_per_frame() {
    let mut stage = InputStage::new();
    assert_eq!(edges(&drain(&mut stage, &[at(120, 140, true)])), vec![(true, 120, 140)]);
    assert_eq!(
        edges(&drain(&mut stage, &[at(200, 200, true), at(300, 260, true)])),
        vec![(true, 300, 260)]
    );
    assert_eq!(edges(&drain(&mut stage, &[at(520, 420, false)])), vec![(false, 520, 420)]);
}

/// A double click inside one pass: all four edges, in order.
#[test]
fn a_double_click_inside_one_pass_keeps_all_four_edges() {
    let mut stage = InputStage::new();
    let applied = drain(
        &mut stage,
        &[at(50, 60, true), at(50, 60, false), at(50, 60, true), at(50, 60, false)],
    );
    assert_eq!(
        edges(&applied),
        vec![(true, 50, 60), (false, 50, 60), (true, 50, 60), (false, 50, 60)]
    );
}

/// Wheel notches sum across coalesced motion and are applied exactly once; a one-shot fact (a
/// tiling chord, a capture key) survives a newer sample without one.
#[test]
fn wheel_notches_sum_and_one_shot_facts_survive_coalescing() {
    let mut stage = InputStage::new();
    let mut first = at(10, 10, false);
    first.wheel_delta_y = 2;
    first.wm_chord = 3;
    let mut second = at(12, 10, false);
    second.wheel_delta_y = -1;
    second.capture = 1;
    let applied = drain(&mut stage, &[first, second, at(14, 10, false)]);
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].wheel_delta_y, 1);
    assert_eq!((applied[0].wm_chord, applied[0].capture), (3, 1));
    assert_eq!(stage.take(), None, "a frame's sample is applied once");
}

/// Wheel notches on a press stay with the press; the release's own notches are not doubled.
#[test]
fn an_edge_keeps_its_own_wheel_notches() {
    let mut stage = InputStage::new();
    let mut press = at(5, 5, true);
    press.wheel_delta_y = 1;
    let mut release = at(5, 5, false);
    release.wheel_delta_y = 2;
    let applied = drain(&mut stage, &[press, release]);
    let wheel: Vec<i32> = applied.iter().map(|s| s.wheel_delta_y).collect();
    assert_eq!(wheel, vec![1, 2]);
}
