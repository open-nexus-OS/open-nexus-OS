// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for animation::timeline.
//! OWNERS: @ui
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use animation::{AnimProp, AnimationDriver, Easing, LayerId, SpringConfig, SpringSim};

#[test]
fn tick_produces_updates() {
    let mut d = AnimationDriver::new();
    d.spring_to(LayerId(1), AnimProp::Opacity, 0.0, 1.0, SpringConfig::default());
    assert!(!d.tick(16_666_667).is_empty());
}

#[test]
fn spring_removed_after_convergence() {
    // Verify SpringSim converges, then verify AnimationDriver removes it
    let mut sim = SpringSim::new(0.0, 1.0, SpringConfig::default());
    for _ in 0..1000 {
        sim.step(16_666_667);
        if sim.done() {
            break;
        }
    }
    assert!(sim.done(), "SpringSim must converge");

    // Now test AnimationDriver removal — progressive time
    let mut d = AnimationDriver::new();
    d.spring_to(
        LayerId(1),
        AnimProp::Opacity,
        0.0,
        1.0,
        SpringConfig { stiffness: 500.0, damping: 50.0, mass: 1.0, initial_velocity: 0.0 },
    );
    let mut t = 16_666_667;
    for _ in 0..400 {
        t += 16_666_667;
        d.tick(t);
        if d.active_count() == 0 {
            break;
        }
    }
    assert_eq!(d.active_count(), 0);
}

#[test]
fn cancel_removes() {
    let mut d = AnimationDriver::new();
    d.spring_to(LayerId(1), AnimProp::Opacity, 0.0, 1.0, SpringConfig::default());
    assert_eq!(d.active_count(), 1);
    d.cancel(LayerId(1), AnimProp::Opacity);
    assert_eq!(d.active_count(), 0);
}

#[test]
fn reduced_motion_caps() {
    let mut d = AnimationDriver::new();
    d.set_reduced_motion(true);
    d.keyframe_to(
        LayerId(1),
        AnimProp::Opacity,
        vec![(0.0, 0.0), (1.0, 1.0)],
        1_000_000_000,
        Easing::Linear,
    );
    assert!(!d.tick(16_666_667).is_empty());
}

#[test]
fn reset_clock_prevents_idle_gap_jump() {
    // Real clocks are nanoseconds-since-boot (billions); an idle driver's
    // `last_tick` is stale, so the first tick's dt would be the whole idle span
    // and a keyframe would jump to its end. reset_clock seeds the clock so the
    // first tick measures ONE frame — the eased motion is visible.
    let base = 5_000_000_000u64; // 5s since boot
    let mut d = AnimationDriver::new();
    d.keyframe_to(
        LayerId(1),
        AnimProp::Opacity,
        vec![(0.0, 0.0), (1.0, 1.0)],
        280_000_000,
        Easing::Linear,
    );
    d.reset_clock(base);
    let updates = d.tick(base + 16_666_667); // ~16.6/280 ≈ 6% progress
    assert_eq!(updates.len(), 1);
    assert!(updates[0].value < 0.2, "eased, not jumped: got {}", updates[0].value);
    assert!(d.active_count() > 0, "still animating one frame in");
}

#[test]
fn idle_gap_without_reset_jumps() {
    // Documents the hazard reset_clock guards: without seeding, the first tick
    // after an idle span collapses the animation into a single frame.
    let base = 5_000_000_000u64;
    let mut d = AnimationDriver::new();
    d.keyframe_to(
        LayerId(1),
        AnimProp::Opacity,
        vec![(0.0, 0.0), (1.0, 1.0)],
        280_000_000,
        Easing::Linear,
    );
    let updates = d.tick(base + 16_666_667); // dt ≈ 5s (huge) → jumps to end
    assert_eq!(updates.len(), 1);
    assert!((updates[0].value - 1.0).abs() < 0.001, "jumped to end: {}", updates[0].value);
    assert_eq!(d.active_count(), 0, "completed in one tick");
}
