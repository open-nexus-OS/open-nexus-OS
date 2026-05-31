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
