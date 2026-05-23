// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for animation::spring.
//! OWNERS: @ui
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use animation::{SpringSim, SpringConfig};

#[test]
fn spring_converges_to_target() {
    let mut sim = SpringSim::new(0.0, 1.0, SpringConfig::default());
    let mut last_pos = 0.0;
    let mut converged = false;
    for _ in 0..1000 {
        last_pos = sim.step(16_666_667); // ~60fps
        if sim.done() {
            converged = true;
            break;
        }
    }
    assert!(converged, "spring should converge within 1000 steps");
    assert!((last_pos - 1.0).abs() < 0.01, "should reach target");
}

#[test]
fn spring_deterministic_same_input_same_output() {
    let config = SpringConfig::default();
    let mut a = SpringSim::new(0.0, 1.0, config);
    let mut b = SpringSim::new(0.0, 1.0, config);

    for _ in 0..100 {
        let pa = a.step(16_666_667);
        let pb = b.step(16_666_667);
        assert_eq!(pa, pb, "same input must produce same output");
    }
}

#[test]
fn spring_cancel_stops_updating() {
    let mut sim = SpringSim::new(0.0, 1.0, SpringConfig::default());
    for _ in 0..500 {
        sim.step(16_666_667);
        if sim.done() {
            break;
        }
    }
    assert!(sim.done());
    let pos = sim.step(16_666_667);
    assert_eq!(pos, 1.0, "done spring returns target");
}

#[test]
fn spring_custom_config_faster() {
    let stiff = SpringConfig { stiffness: 400.0, ..Default::default() };
    let mut sim = SpringSim::new(0.0, 1.0, stiff);
    let mut steps = 0;
    for _ in 0..1000 {
        sim.step(16_666_667);
        steps += 1;
        if sim.done() {
            break;
        }
    }
    assert!(steps < 500, "stiffer spring should converge faster");
}
