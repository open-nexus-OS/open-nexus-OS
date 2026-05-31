// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for animation::keyframe.
//! OWNERS: @ui
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use animation::{Easing, KeyframeTrack};

#[test]
fn linear_reaches_target() {
    let mut track = KeyframeTrack::new(
        vec![(0.0, 0.0), (1.0, 100.0)],
        1_000_000_000, // 1 second
        Easing::Linear,
    );
    let mut last = 0.0;
    for _ in 0..60 {
        last = track.step(16_666_667); // 60fps
    }
    assert!(track.done());
    assert!((last - 100.0).abs() < 1.0);
}

#[test]
fn ease_out_starts_fast_ends_slow() {
    let mut track =
        KeyframeTrack::new(vec![(0.0, 0.0), (1.0, 100.0)], 1_000_000_000, Easing::EaseOut);
    let v1 = track.step(16_666_667); // first frame
                                     // EaseOut: early frames have larger steps
    assert!(v1 > 1.0, "ease-out starts fast, got {v1}");
}

#[test]
fn ease_in_starts_slow_ends_fast() {
    let mut track =
        KeyframeTrack::new(vec![(0.0, 0.0), (1.0, 100.0)], 1_000_000_000, Easing::EaseIn);
    let v1 = track.step(16_666_667);
    assert!(v1 < 5.0, "ease-in starts slow, got {v1}");
}

#[test]
fn multi_keyframe_interpolation() {
    let mut track = KeyframeTrack::new(
        vec![(0.0, 0.0), (0.5, 50.0), (1.0, 100.0)],
        1_000_000_000,
        Easing::Linear,
    );
    // At 500ms (halfway), should be near 50.0
    for _ in 0..30 {
        track.step(16_666_667);
    }
    assert!((track.value() - 50.0).abs() < 5.0);
}
